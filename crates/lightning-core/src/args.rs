//! Argumentos tipados de un comando.
//!
//! Declara una `struct` con `#[derive(CommandArgs)]` y pídela como `Args<T>`.
//! Lightning genera las opciones de Discord **y** el parser de prefijo a partir
//! de los mismos campos: una sola fuente de verdad para `/eco` y `!eco`.

use std::ops::{Deref, DerefMut};

use serde_json::{Map, Value};
use twilight_model::id::Id;

use crate::error::{Error, Result};

/// Tipo de una opción de Discord.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionKind {
    /// Texto.
    String,
    /// Entero de 64 bits.
    Integer,
    /// Número decimal.
    Number,
    /// Booleano.
    Boolean,
}

impl OptionKind {
    /// Código numérico que usa la API de Discord.
    pub const fn code(self) -> u8 {
        match self {
            Self::String => 3,
            Self::Integer => 4,
            Self::Boolean => 5,
            Self::Number => 10,
        }
    }
}

/// Metadatos de una opción, generados por `#[derive(CommandArgs)]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptionSpec {
    /// Nombre de la opción (minúsculas, sin espacios).
    pub name: &'static str,
    /// Descripción visible en Discord.
    pub description: &'static str,
    /// Tipo de la opción.
    pub kind: OptionKind,
    /// Si es obligatoria.
    pub required: bool,
}

/// Un conjunto de argumentos tipados.
///
/// Lo implementa `#[derive(CommandArgs)]`; también puedes implementarlo a mano
/// si necesitas algo exótico.
pub trait CommandArgs: Sized + Send + 'static {
    /// Opciones que se publican en Discord.
    const OPTIONS: &'static [OptionSpec];

    /// Construye el valor desde opciones de una interacción.
    ///
    /// # Errors
    ///
    /// Devuelve error si falta un campo obligatorio o el tipo no encaja.
    fn from_options(options: Map<String, Value>) -> Result<Self>;

    /// Construye el valor desde los tokens de un comando de prefijo.
    ///
    /// # Errors
    ///
    /// Devuelve error si falta un token obligatorio o no se puede convertir.
    fn from_prefix(tokens: &[String]) -> Result<Self>;
}

/// Envuelve los argumentos de un handler: `Args<MiEstructura>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Args<T>(pub T);

impl<T> Args<T> {
    /// Desenvuelve los argumentos.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Deref for Args<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for Args<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> From<T> for Args<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl CommandArgs for () {
    const OPTIONS: &'static [OptionSpec] = &[];

    fn from_options(_: Map<String, Value>) -> Result<Self> {
        Ok(())
    }

    fn from_prefix(_: &[String]) -> Result<Self> {
        Ok(())
    }
}

/// Utilidad para las macros: error de argumento obligatorio.
#[doc(hidden)]
pub fn missing(name: &str) -> Error {
    Error::MissingArgument(name.to_owned())
}

/// Utilidad para las macros: error de conversión.
#[doc(hidden)]
pub fn invalid(name: &str, reason: impl std::fmt::Display) -> Error {
    Error::InvalidArgument {
        name: name.to_owned(),
        reason: reason.to_string(),
    }
}

/// Conversión desde el `Value` JSON que llega en una interacción.
///
/// Lo usa `#[derive(CommandArgs)]`; no suele implementarse a mano.
pub trait FromOptionValue: Sized {
    /// Convierte el valor; `name` se usa para los mensajes de error.
    fn from_option(value: Value, name: &'static str) -> Result<Self>;
}

/// Conversión desde un token de comando de prefijo.
///
/// Lo usa `#[derive(CommandArgs)]`; no suele implementarse a mano.
pub trait FromToken: Sized {
    /// Convierte el token; `name` se usa para los mensajes de error.
    fn from_token(token: &str, name: &'static str) -> Result<Self>;
}

impl FromOptionValue for Value {
    fn from_option(value: Value, _: &'static str) -> Result<Self> {
        Ok(value)
    }
}

impl FromOptionValue for String {
    fn from_option(value: Value, name: &'static str) -> Result<Self> {
        match value {
            Value::String(text) => Ok(text),
            Value::Number(number) => Ok(number.to_string()),
            Value::Bool(flag) => Ok(flag.to_string()),
            Value::Null => Err(missing(name)),
            other => Err(invalid(name, format!("se esperaba texto, llegó {other}"))),
        }
    }
}

impl FromToken for String {
    fn from_token(token: &str, _: &'static str) -> Result<Self> {
        Ok(token.to_owned())
    }
}

impl<T: FromOptionValue> FromOptionValue for Option<T> {
    fn from_option(value: Value, name: &'static str) -> Result<Self> {
        match value {
            Value::Null => Ok(None),
            other => T::from_option(other, name).map(Some),
        }
    }
}

impl<T: FromOptionValue> FromOptionValue for Vec<T> {
    fn from_option(value: Value, name: &'static str) -> Result<Self> {
        match value {
            Value::Array(values) => values
                .into_iter()
                .map(|value| T::from_option(value, name))
                .collect(),
            Value::Null => Err(missing(name)),
            other => Ok(vec![T::from_option(other, name)?]),
        }
    }
}

impl<T> FromOptionValue for Id<T> {
    fn from_option(value: Value, name: &'static str) -> Result<Self> {
        let raw = match value {
            Value::Number(number) => number
                .as_u64()
                .ok_or_else(|| invalid(name, "id fuera de rango"))?,
            Value::String(text) => text.parse().map_err(|error| invalid(name, error))?,
            Value::Null => return Err(missing(name)),
            other => return Err(invalid(name, format!("se esperaba un id, llegó {other}"))),
        };

        Ok(Id::new(raw))
    }
}

impl<T> FromToken for Id<T> {
    fn from_token(token: &str, name: &'static str) -> Result<Self> {
        let raw = token.parse().map_err(|error| invalid(name, error))?;

        Ok(Id::new(raw))
    }
}

impl FromOptionValue for bool {
    fn from_option(value: Value, name: &'static str) -> Result<Self> {
        match value {
            Value::Bool(flag) => Ok(flag),
            Value::String(text) => parse_bool(&text, name),
            Value::Number(number) => Ok(number.as_u64().unwrap_or(0) != 0),
            Value::Null => Err(missing(name)),
            other => Err(invalid(
                name,
                format!("se esperaba un booleano, llegó {other}"),
            )),
        }
    }
}

impl FromToken for bool {
    fn from_token(token: &str, name: &'static str) -> Result<Self> {
        parse_bool(token, name)
    }
}

impl FromOptionValue for char {
    fn from_option(value: Value, name: &'static str) -> Result<Self> {
        let text = String::from_option(value, name)?;
        single_char(&text, name)
    }
}

impl FromToken for char {
    fn from_token(token: &str, name: &'static str) -> Result<Self> {
        single_char(token, name)
    }
}

fn parse_bool(text: &str, name: &'static str) -> Result<bool> {
    match text.to_ascii_lowercase().as_str() {
        "true" | "1" | "si" | "sí" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(invalid(name, "se esperaba true o false")),
    }
}

fn single_char(text: &str, name: &'static str) -> Result<char> {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(character), None) => Ok(character),
        _ => Err(invalid(name, "se esperaba un único carácter")),
    }
}

macro_rules! impl_number {
    ($($ty:ty),* $(,)?) => {
        $(
            impl FromOptionValue for $ty {
                fn from_option(value: Value, name: &'static str) -> Result<Self> {
                    match value {
                        Value::Number(number) => number
                            .to_string()
                            .parse()
                            .map_err(|error| invalid(name, error)),
                        Value::String(text) => text
                            .parse()
                            .map_err(|error| invalid(name, error)),
                        Value::Null => Err(missing(name)),
                        other => Err(invalid(name, format!("se esperaba un número, llegó {other}"))),
                    }
                }
            }

            impl FromToken for $ty {
                fn from_token(token: &str, name: &'static str) -> Result<Self> {
                    token.parse().map_err(|error| invalid(name, error))
                }
            }
        )*
    };
}

impl_number!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64
);
