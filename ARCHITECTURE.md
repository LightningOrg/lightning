# Arquitectura de ⚡ Lightning

Este documento explica cómo está montado el framework y por qué. Si vienes a
tocar el código, empieza por aquí.

## Las tres crates

```
┌─────────────────────────────────────────────────────────────┐
│ lightning                (runtime)                          │
│   Gateway, HTTP, sharding, registro, conversión a Discord    │
│   ┌──────────────────────────────────────────────────────┐  │
│   │ lightning-core          (núcleo, sin red)            │  │
│   │   Command, Context, extractores, Middleware, Reply    │  │
│   │   Responder (trait) · StateMap · Error                │  │
│   └──────────────────────────────────────────────────────┘  │
│   lightning-macros       (procedural macros)                │
│   #[command] #[event] #[component] #[main] derive(CommandArgs)│
└─────────────────────────────────────────────────────────────┘
```

- **`lightning-core`** no depende de `tokio` ni de `twilight-http`: solo de los
  tipos de datos de `twilight-model`. Se puede usar en tests, en un runtime
  distinto o incluso como base para otro transporte.
- **`lightning`** es la fachada que usa el usuario (`use lightning::prelude::*`).
- **`lightning-macros`** genera código que siempre apunta a `::lightning::…`,
  por eso el usuario solo depende de la fachada.

## El viaje de un evento

```
        Gateway (twilight-gateway, un Shard por tarea)
                                │  Event
                                ▼
                     Runtime::dispatch
             ┌──────────────────┴───────────────────┐
             ▼                                      ▼
    InteractionCreate                        MessageCreate
             │                                      │
    ┌────────┼─────────┐                    ¿empieza por prefijo?
    ▼        ▼         ▼                            │
 slash   componente  modal                    tokenize + buscar
    │        │         │                            │
    └────────┴─────────┘                            │
             │                                      │
             ▼                                      ▼
   Context { user, guild, channel, options, tokens, state, responder }
                                │
                                ▼
        run_pipeline(global ++ capas del comando)
                                │
                    ┌───────────┴───────────┐
                    ▼                       ▼
              Middleware::handle         Next::run
                    │                       │
                    └───────────┬───────────┘
                                ▼
                     Command::run (RunFn)
                                │
                     extractores + handler
                                │
                                ▼
                         Result<Reply>
                                │
                                ▼
                  Responder (HTTP real o mock)
```

## Decisiones de diseño

### 1. Twilight como motor, Lightning como capa

Twilight ya resuelve lo aburrido y difícil de acertar: Gateway con resume y
heartbeat, ratelimits por bucket, caché opcional, validación de payloads. Lo que
no trae es ergonomía: registrar comandos, enrutar interacciones, parsear
opciones y responder son tareas manuales.

Lightning no reimplementa el transporte: lo envuelve. Así heredamos los arreglos
de Twilight y nos centramos en la capa de producto.

### 2. `inventory` para el auto-registro

`#[command]` emite, además de la función, una entrada estática:

```rust
inventory::submit! {
    lightning::Command::new("ping", "Responde pong")
        .kind(CommandKind::Both)
        .options(<() as CommandArgs>::OPTIONS)
        .handler(__lightning_run_ping)
}
```

`Command` es `const`-construible (solo `&'static str`, slices y punteros a
función), así que el registro no tiene coste en runtime: es una sección más del
binario. En `run()` se recogen todas las entradas y se publican en Discord.

Ventajas: cero listas, cero `use` olvidados, los comandos viven donde se usan.
Contras: los comandos no pueden capturar estado del entorno (para eso está el
sistema de `State`), y algunos enlazadores agresivos pueden descartar entradas
(se mitiga con `#[used]`, que ya aplica `inventory`).

### 3. El responder es un trait

```rust
#[async_trait]
pub trait Responder: Send + Sync + 'static {
    async fn respond(&self, reply: Reply) -> Result<()>;
    async fn defer(&self, ephemeral: bool) -> Result<()>;
    async fn followup(&self, reply: Reply) -> Result<()>;
}
```

En producción lo implementa `TwilightResponder`, que sabe si la invocación viene
de una interacción (token, `create_response`, followups) o de un mensaje
(`create_message`). En tests lo implementa `MockResponder`, que guarda los
`Reply` en un `Vec`.

Es la decisión que hace que el framework sea testeable de verdad: la misma
función de handler, el mismo pipeline y los mismos extractores, sin red.

### 4. JSON como representación intermedia

`Reply` guarda textos, embeds y componentes como `serde_json::Value` con el
formato exacto de la API de Discord. El runtime los convierte a los tipos de
`twilight-model` al enviarlos (`serde_json::from_value::<Embed>`).

Por qué:

- Los tipos de Discord cambian a menudo; el JSON de la API es el contrato estable.
- Un solo lugar que arreglar si Twilight cambia una firma.
- Los tests validan de verdad el JSON final (ver `convert.rs`).

El coste es una deserialización por mensaje: irrelevante frente a la latencia de
la API.

### 5. El pipeline se construye una vez

`Command::build_layers()` se ejecuta al arrancar, no en cada invocación. Eso
permite que capas con estado (`Cooldown` con su `DashMap`) compartan ese estado
entre mensajes sin variables globales.

Las capas globales se concatenan con las del comando en el mismo orden en que se
declararon.

### 6. Una función, dos superficies

El puente es la struct de argumentos:

- `CommandDataOption[]` de Discord → `Map<String, Value>` → `T::from_options`.
- `!eco hola 2` → tokenizer → `T::from_prefix`.

`#[derive(CommandArgs)]` genera ambos parsers desde los mismos campos, así que no
hay dos definiciones que se puedan desincronizar. El precio es que solo soporta
tipos "razonables"; para lo exótico se puede implementar `CommandArgs` a mano.

## Flujo de una respuesta

1. El handler devuelve `impl IntoReply` (`Reply`, `String`, `()`,
   `Result<T, Error>`…).
2. `IntoReply::into_reply` lo normaliza a `Reply`.
3. Si hubo `defer`, el responder manda un *followup*; si no, la respuesta
   inicial (tipo 4, o tipo 7 para botones).
4. Un `Reply` vacío no envía nada: así puedes hacer `ctx.defer().await?` y luego
   decidir no responder.
5. Un `Err` se convierte en `❌ mensaje` efímero y se registra con `tracing`.

## Límites conocidos (MVP)

- Autocompletado: los payloads se ignoran de momento.
- Subcomandos: el enrutado existe para opciones planas; los grupos se aplanan.
- Sharding: shards propios, sin `ShardManager` ni cola coordinada.
- No hay caché de guilds: si necesitas miembros o roles, usa HTTP o añade
  `twilight-cache-inmemory` en una capa propia.
- `Reply` no soporta adjuntos (attachments) ni `payload_json`.

## Cómo añadir un comando al framework

1. `#[command]` genera `RunFn` y la entrada de `inventory`.
2. `Runtime::dispatch` encuentra el comando por nombre.
3. Construye el `Context` (interacción o mensaje).
4. Ejecuta `run_pipeline` y responde con el `Responder`.

Si añades una superficie nueva (por ejemplo, menús), reutiliza `RunFn`, el
pipeline y `Context`: solo cambia la forma de construir el contexto y el
`custom_id`.
