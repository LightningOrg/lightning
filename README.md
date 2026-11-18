# ⚡ Lightning

[![CI](https://github.com/ImDaiDev/lightning/actions/workflows/ci.yml/badge.svg)](https://github.com/ImDaiDev/lightning/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#licencia)
[![Rust 1.85+](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org)

**Bots de Discord a la velocidad del rayo.**

Lightning es un framework de Rust para construir bots de Discord con una idea
central: si ya sabes escribir una función `async`, ya sabes escribir un comando.

```rust
/// Responde con pong ⚡
#[command]
async fn ping(ctx: Context) -> Reply {
    Reply::text(format!("pong ⚡ hola, {}", ctx.user.name))
}
```

Eso es todo. Ese comando ya responde a `/ping` **y** a `!ping`, aparece
registrado en Discord y se puede testear sin conexión.

> **Estado:** alpha (`0.1.0`). La API puede cambiar antes de `1.0`.
> Motor: [twilight](https://twilight.rs) `0.18`.

---

## ¿Por qué otro framework?

| | Lightning | Serenity | Twilight puro |
|---|---|---|---|
| Comandos slash | ✅ macro `#[command]` | ✅ macro | ❌ a mano |
| Comandos de prefijo | ✅ **el mismo comando** | ⚠️ otro sistema | ❌ a mano |
| Registro automático | ✅ `inventory` | ❌ lista manual | ❌ lista manual |
| Argumentos tipados | ✅ `Args<T>` + derive | ⚠️ parseo manual | ❌ a mano |
| Extractores tipo axum | ✅ `Context`, `State<T>`, `Guild`… | ❌ | ❌ |
| Middleware estilo tower | ✅ `next.run(ctx)` | ⚠️ hooks | ❌ |
| Testing sin Discord | ✅ `Responder` es un trait | ⚠️ mocks manuales | ❌ |
| Sharding | ✅ automático | ✅ | ⚠️ manual |

La personalidad de Lightning está en tres decisiones:

1. **Un comando, dos superficies.** Una sola función, una sola struct de
   argumentos, pero responde a `/eco` y a `!eco`.
2. **Todo se descubre solo.** Escribes la función y el framework la encuentra.
3. **Todo es testeable.** El envío de mensajes es un trait: en producción habla
   HTTP con Discord; en tests apunta a un `Vec`.

---

## Instalación

Lightning todavía no está en crates.io. Usa el workspace como dependencia de
ruta:

```toml
# Cargo.toml
[dependencies]
lightning = { path = "../Lightning/crates/lightning" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Rust **1.85+** (edición 2024).

---

## 30 segundos

```rust
use lightning::prelude::*;

/// Responde con pong ⚡
#[command]
async fn ping(ctx: Context) -> Reply {
    Reply::text(format!("pong ⚡ hola, {}", ctx.user.name))
}

#[lightning::main]
async fn main() {
    Lightning::new()
        .token_from_env()   // lee DISCORD_TOKEN
        .prefix("!")        // activa también los comandos de prefijo
        .run()
        .await
        .expect("el bot se ha caído");
}
```

```powershell
$env:DISCORD_TOKEN = "tu-token"
cargo run -p examples-basics
```

Al arrancar verás el banner:

```
  ⚡ Lightning v0.1.0
  ─────────────────────────────────────
  Bot         Lightning#1234
  Servidores  3
  Comandos    6
  Shards      1
```

---

## El modelo mental

### Un comando, dos superficies

```rust
#[command]
async fn ping(ctx: Context) -> Reply { /* /ping y !ping */ }
```

El atributo `kind` cambia esto cuando lo necesites:

```rust
#[command(kind = "slash")]   // solo /ping
#[command(kind = "prefix")]  // solo !ping
```

Los alias solo afectan al prefijo:

```rust
#[command(aliases = ["rep", "say"])]
async fn eco(/* … */) {}
```

### Argumentos

Declara una struct normal y deriva `CommandArgs`. Los **doc comments de los
campos se convierten en las descripciones de Discord**:

```rust
#[derive(CommandArgs)]
struct Eco {
    /// Qué debe repetir el bot
    texto: String,
    /// Cuántas veces repetirlo
    veces: Option<u8>,
}

#[command]
async fn eco(args: Args<Eco>) -> Reply {
    let veces = args.veces.unwrap_or(1);
    Reply::text(args.texto.repeat(veces as usize))
}
```

- `/eco texto: hola veces: 2` → opciones de Discord tipadas.
- `!eco hola 2` → parser posicional con comillas (`!eco "hola mundo" 2`).
- Los campos `Option<T>` son opcionales en ambas superficies.
- Marca `#[arg(rest)]` para capturar el resto de la línea en un `String`.

Tipos soportados: `String`, enteros, `f32`/`f64`, `bool`, `char`, `Id<T>`,
`Option<T>`, `Vec<T>` y `serde_json::Value`.

### Extractores

La firma de la función es la documentación: pide lo que necesites y Lightning lo
inyecta.

| Extractor | Qué te da |
|---|---|
| `Context` | usuario, guild, canal, interacción… |
| `Args<T>` | argumentos tipados |
| `State<T>` | un estado inyectado con `.state(…)` |
| `Guild` | id del servidor (falla en DM) |
| `Member` | miembro que invocó |
| `Author` | autor como `User` |
| `ChannelId` | canal actual |
| `Message` | mensaje de origen (prefijo) |
| `Interaction` | interacción original |
| `Values` | valores de un menú de selección |

### Middleware

Capas que envuelven la ejecución, con la misma idea que tower.

```rust
use std::time::Duration;

let bot = Lightning::new()
    .middleware(Cooldown::per_user(Duration::from_secs(3)));
```

Y puedes escribir las tuyas con un closure…

```rust
let logging = |ctx: Context, next: Next| async move {
    println!("→ {}", ctx.command);
    next.run(ctx).await
};
```

…o con el trait, si necesitas estado:

```rust
#[lightning::async_trait]
impl Middleware for SoloAdmins {
    async fn handle(&self, ctx: Context, next: Next) -> Result<Reply> {
        if !es_admin(&ctx) {
            return Err(Error::Forbidden);
        }
        next.run(ctx).await
    }
}
```

También por comando:

```rust
#[command(middleware = [Cooldown::per_user(Duration::from_secs(30))])]
async fn lento() -> Reply { /* … */ }
```

### Estado

```rust
struct Db(sqlx::PgPool);

let pool = /* tu pool */;

Lightning::new().state(Db(pool));
// o, si prefieres compartir el Arc:
// Lightning::new().state_arc(std::sync::Arc::new(Db(pool)));
```

```rust
#[command]
async fn usuarios(db: State<Db>) -> Reply {
    let total = sqlx::query_scalar("select count(*) from users")
        .fetch_one(&db.0)
        .await?;
    Reply::text(format!("{total} usuarios"))
}
```

### Botones y modales

```rust
#[command]
async fn confirmar() -> Reply {
    Reply::text("¿Seguro?").components(vec![
        ActionRow::new()
            .button(Button::success(r#"confirm:{"choice":"yes"}"#, "Sí"))
            .button(Button::danger(r#"confirm:{"choice":"no"}"#, "No")),
    ])
}

#[component("confirm")]  // atiende cualquier custom_id que empiece por `confirm`
async fn on_confirm(args: Args<Choice>) -> Reply {
    match args.choice.as_str() {
        "yes" => Reply::text("¡hecho! ⚡"),
        _ => Reply::text("cancelado"),
    }
}
```

El trozo después de `:` se parsea como JSON y llega como `Args<T>`, así que el
estado de un botón viaja **tipado**. Los modales se responden con
`Reply::modal(Modal::new("id", "Título").text_input(...))`.

### Eventos

```rust
use lightning::model::incoming::MemberAdd;

#[event(MemberAdd)]
async fn bienvenida(evento: &MemberAdd) {
    println!("entró {}", evento.user.name);
}
```

Si añades un segundo parámetro, recibes un `EventContext` con HTTP y estados:

```rust
#[event(MessageCreate)]
async fn log(evento: &MessageCreate, ctx: EventContext) {
    let _ = ctx.http;
}
```

### Sharding

Lightning pide a Discord el número recomendado de shards y arranca uno por shard,
compartiendo el mismo estado y registro de comandos. Si quieres forzarlo:

```rust
Lightning::new().shards(4)
```

### Testing

```rust
use lightning::testing::mock_context;

#[tokio::test]
async fn ping_responde() {
    let (ctx, responder) = mock_context("ping");

    let reply = ping(ctx.clone()).await;
    ctx.reply(reply).await.unwrap();

    assert_eq!(
        responder.last().unwrap().content.as_deref(),
        Some("pong test"),
    );
}
```

Cero red, cero token, cero Discord.

---

## Referencia de `#[command]`

| Atributo | Ejemplo | Descripción |
|---|---|---|
| `name` | `name = "eco"` | Nombre del comando |
| `description` | `description = "Repite"` | Si falta, usa el doc comment |
| `kind` | `kind = "slash"` | `slash`, `prefix` o `both` (defecto) |
| `aliases` | `aliases = ["rep"]` | Alias de prefijo |
| `guild_only` | `guild_only` | Solo en servidores |
| `nsfw` | `nsfw` | Marca el comando como NSFW |
| `permissions` | `permissions = 8` | Permisos por defecto (bits) |
| `middleware` | `middleware = [MiCapa]` | Capas propias del comando |
| `auto_register` | `auto_register = "false"` | Desactiva el auto-registro |

---

## Estructura del repo

```
Lightning/
├── crates/
│   ├── lightning-core/    # modelo, contexto, extractores, middleware, Reply
│   ├── lightning-macros/  # #[command], #[event], #[component], #[main], derive
│   └── lightning/         # runtime: Gateway, HTTP, sharding, registro
├── examples/
│   └── basics/            # bot de ejemplo completo
├── ARCHITECTURE.md        # cómo funciona por dentro y por qué
└── Cargo.toml             # workspace
```

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets
cargo run -p examples-basics
```

---

## Roadmap

- [ ] Autocompletado tipado (`#[autocomplete]`).
- [ ] Subcomandos y grupos (`#[subcommand]`).
- [ ] Menús de selección de primera clase.
- [ ] Sharding por `ShardManager` y `twilight-gateway-queue` (hoy: shards propios).
- [ ] `lightning dev` con recarga en caliente.
- [ ] Publicación en crates.io.

---

## Licencia

MIT OR Apache-2.0. Puedes elegir la que prefieras:

- [`LICENSE-MIT`](LICENSE-MIT)
- [`LICENSE-APACHE`](LICENSE-APACHE)
