//! Application-wide error type.
//!
//! Every Tauri command returns `AppResult<T>`. Errors are serialised to the
//! frontend as `{ kind, message }` so the UI can show an honest, specific
//! failure instead of a generic "something went wrong".

use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Message(String),

    #[error("Database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Data format error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Network error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("WMI query failed: {0}")]
    Wmi(String),

    #[error("Not permitted: {0}")]
    Forbidden(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid input: {0}")]
    Invalid(String),

    #[error("Administrator approval was declined or failed: {0}")]
    Elevation(String),

    #[error("Cancelled")]
    Cancelled,
}

impl AppError {
    pub fn msg(s: impl Into<String>) -> Self {
        AppError::Message(s.into())
    }

    fn kind(&self) -> &'static str {
        match self {
            AppError::Message(_) => "error",
            AppError::Db(_) => "database",
            AppError::Io(_) => "io",
            AppError::Json(_) => "format",
            AppError::Http(_) => "network",
            AppError::Wmi(_) => "wmi",
            AppError::Forbidden(_) => "forbidden",
            AppError::NotFound(_) => "not_found",
            AppError::Invalid(_) => "invalid",
            AppError::Elevation(_) => "elevation",
            AppError::Cancelled => "cancelled",
        }
    }
}

impl From<wmi::WMIError> for AppError {
    fn from(e: wmi::WMIError) -> Self {
        AppError::Wmi(e.to_string())
    }
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        AppError::Message(e.to_string())
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;
