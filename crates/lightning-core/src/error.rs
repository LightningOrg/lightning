//! Errores del framework.
//!
//! Todo handler puede devolver `Result<_, Error>`: Lightning convierte el error
//! en una respuesta efímera legible para el usuario y lo registra en `tracing`.

use crate::reply::Reply;

/// Alias de resultado que usa todo el framework.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Error de un comando, extractor o middleware.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Error definido por el usuario.
    #[error("{0}")]
    Custom(String),

    /// Falta un argumento obligatorio.
    #[error("falta el argumento `{0}`")]
    MissingArgument(String),

    /// Un argumento no se pudo convertir al tipo esperado.
    #[error("el argumento `{name}` no es válido: {reason}")]
    InvalidArgument {
        /// Nombre del argumento.
        name: String,
        /// Motivo del fallo.
        reason: String,
    },

    /// El comando solo funciona dentro de un servidor.
    #[error("este comando solo funciona dentro de un servidor")]
    GuildOnly,

    /// El comando solo funciona por mensaje directo.
    #[error("este comando solo funciona por mensaje directo")]
    DmOnly,

    /// El usuario no tiene permiso para ejecutar el comando.
    #[error("no tienes permiso para usar este comando")]
    Forbidden,

    /// Se ha superado un límite de uso.
    #[error("demasiadas invocaciones: inténtalo de nuevo en {retry_after_ms} ms")]
    Cooldown {
        /// Milisegundos que hay que esperar.
        retry_after_ms: u64,
    },

    /// El comando no existe en esta superficie (slash, prefijo, componente...).
    #[error("este comando no está disponible en esta superficie")]
    UnsupportedSurface,

    /// Un extractor no encontró el recurso que necesitaba.
    #[error("falta el recurso `{0}` en el contexto")]
    MissingResource(&'static str),

    /// No se ha inyectado un estado con ese tipo.
    #[error("no se ha inyectado el estado `{0}`")]
    MissingState(&'static str),

    /// Fallo de red o de la API de Discord.
    #[error("error hablando con Discord: {0}")]
    Transport(String),

    /// Fallo interno del framework.
    #[error("error interno: {0}")]
    Internal(String),
}

impl Error {
    /// Crea un error propio a partir de cualquier cosa convertible a `String`.
    pub fn custom(message: impl Into<String>) -> Self {
        Self::Custom(message.into())
    }

    /// Construye el [`Reply`] que verá el usuario cuando el handler falle.
    pub fn to_reply(&self) -> Reply {
        Reply::text(format!("❌ {self}")).ephemeral()
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::Internal(value.to_string())
    }
}
