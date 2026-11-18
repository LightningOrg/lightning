//! Declaración de comandos y su registro.
//!
//! Un [`Command`] es una descripción *estática* (apta para `const`) de un
//! comando: nombre, descripción, opciones y un puntero al handler. Eso permite
//! registrarlos automáticamente con `inventory` sin escribir listas a mano.

use std::sync::Arc;

use crate::{BoxFuture, Context, Middleware, Reply, Result};

/// Dónde puede ejecutarse un comando.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    /// Solo como comando de barra (`/ping`).
    Slash,
    /// Solo como comando de prefijo (`!ping`).
    Prefix,
    /// En ambas superficies. **Es el valor por defecto.**
    Both,
}

impl CommandKind {
    /// `true` si esta configuración admite la superficie indicada.
    pub const fn allows(self, slash: bool) -> bool {
        match self {
            Self::Slash => slash,
            Self::Prefix => !slash,
            Self::Both => true,
        }
    }
}

/// Firma del handler generado por `#[command]`.
pub type RunFn = for<'a> fn(&'a Context) -> BoxFuture<'a, Result<Reply>>;

/// Fábrica de middleware declarada en un comando.
///
/// Se ejecuta **una vez al arrancar**, para que capas como [`crate::Cooldown`]
/// mantengan estado compartido entre invocaciones.
pub type LayerFn = fn() -> Vec<Arc<dyn Middleware>>;

/// Descripción completa de un comando.
///
/// Normalmente no lo construyes a mano: lo genera `#[lightning::command]`.
#[derive(Clone)]
pub struct Command {
    /// Nombre sin prefijo (`ping`).
    pub name: &'static str,
    /// Descripción para Discord.
    pub description: &'static str,
    /// Superficies en las que se puede usar.
    pub kind: CommandKind,
    /// Opciones declaradas (generadas a partir de `Args<T>`).
    pub options: &'static [crate::OptionSpec],
    /// Alias para comandos de prefijo.
    pub aliases: &'static [&'static str],
    /// Si solo puede usarse en servidores.
    pub guild_only: bool,
    /// Si se marca como NSFW en Discord.
    pub nsfw: bool,
    /// Permisos por defecto (bits de `Permissions`).
    pub default_permissions: Option<u64>,
    run: Option<RunFn>,
    layers: Option<LayerFn>,
}

impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Command")
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("options", &self.options.len())
            .finish_non_exhaustive()
    }
}

impl Command {
    /// Crea un comando vacío. Pensado para las macros.
    #[doc(hidden)]
    pub const fn new(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            kind: CommandKind::Both,
            options: &[],
            aliases: &[],
            guild_only: false,
            nsfw: false,
            default_permissions: None,
            run: None,
            layers: None,
        }
    }

    /// Cambia la descripción.
    pub const fn description(mut self, description: &'static str) -> Self {
        self.description = description;
        self
    }

    /// Cambia las superficies admitidas.
    pub const fn kind(mut self, kind: CommandKind) -> Self {
        self.kind = kind;
        self
    }

    /// Declara las opciones (las genera `derive(CommandArgs)`).
    pub const fn options(mut self, options: &'static [crate::OptionSpec]) -> Self {
        self.options = options;
        self
    }

    /// Declara alias de prefijo.
    pub const fn aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.aliases = aliases;
        self
    }

    /// Restringe el comando a servidores.
    pub const fn guild_only(mut self) -> Self {
        self.guild_only = true;
        self
    }

    /// Marca el comando como NSFW.
    pub const fn nsfw(mut self) -> Self {
        self.nsfw = true;
        self
    }

    /// Permisos por defecto (bits crudos de Discord).
    pub const fn permissions(mut self, bits: u64) -> Self {
        self.default_permissions = Some(bits);
        self
    }

    /// Asocia el handler. Lo usa `#[command]`.
    #[doc(hidden)]
    pub const fn handler(mut self, run: RunFn) -> Self {
        self.run = Some(run);
        self
    }

    /// Asocia la fábrica de middleware. Lo usa `#[command]`.
    #[doc(hidden)]
    pub const fn layers(mut self, layers: LayerFn) -> Self {
        self.layers = Some(layers);
        self
    }

    /// Handler asociado, si lo hay.
    pub fn run(&self) -> Option<RunFn> {
        self.run
    }

    /// Construye las capas de middleware de este comando.
    pub fn build_layers(&self) -> Vec<Arc<dyn Middleware>> {
        self.layers.map(|factory| factory()).unwrap_or_default()
    }

    /// `true` si el comando se llama (o se apoda) así.
    pub fn matches(&self, name: &str) -> bool {
        self.name == name || self.aliases.contains(&name)
    }
}

inventory::collect!(Command);
