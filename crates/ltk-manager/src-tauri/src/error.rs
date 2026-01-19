use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Error codes that can be communicated across the IPC boundary.
/// These are serialized as SCREAMING_SNAKE_CASE for TypeScript consumption.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// File system I/O error
    Io,
    /// JSON serialization/deserialization error
    Serialization,
    /// Error processing a .modpkg file
    Modpkg,
    /// League of Legends installation not found
    LeagueNotFound,
    /// Invalid file or directory path
    InvalidPath,
    /// Requested mod was not found
    ModNotFound,
    /// Validation failed (e.g., invalid settings)
    ValidationFailed,
    /// Internal state error (e.g., mutex poisoned)
    InternalState,
    /// Mutex lock failed (poisoned)
    MutexLockFailed,
    /// Unknown/unclassified error
    Unknown,
}

/// Structured error response sent over IPC.
/// This provides rich error information to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppErrorResponse {
    /// Machine-readable error code for pattern matching
    pub code: ErrorCode,
    /// Human-readable error message
    pub message: String,
    /// Optional contextual data (e.g., the invalid path, missing mod ID)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
}

impl AppErrorResponse {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            context: None,
        }
    }

    pub fn with_context(mut self, context: impl Serialize) -> Self {
        self.context = serde_json::to_value(context).ok();
        self
    }
}

/// Result type for IPC commands.
/// This is always "successful" at the Tauri level - the actual success/failure
/// is encoded in the response payload for type-safe error handling on the frontend.
///
/// Serializes to `{ "ok": true, "value": T }` or `{ "ok": false, "error": AppErrorResponse }`.
#[derive(Debug, Clone)]
pub enum IpcResult<T> {
    Ok { value: T },
    Err { error: AppErrorResponse },
}

// Custom serialization to use actual boolean values for the `ok` field
impl<T: Serialize> Serialize for IpcResult<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        match self {
            IpcResult::Ok { value } => {
                let mut state = serializer.serialize_struct("IpcResult", 2)?;
                state.serialize_field("ok", &true)?;
                state.serialize_field("value", value)?;
                state.end()
            }
            IpcResult::Err { error } => {
                let mut state = serializer.serialize_struct("IpcResult", 2)?;
                state.serialize_field("ok", &false)?;
                state.serialize_field("error", error)?;
                state.end()
            }
        }
    }
}

impl<T> IpcResult<T> {
    pub fn ok(value: T) -> Self {
        IpcResult::Ok { value }
    }

    #[allow(dead_code)]
    pub fn err(error: impl Into<AppErrorResponse>) -> Self {
        IpcResult::Err {
            error: error.into(),
        }
    }
}

impl<T, E: Into<AppErrorResponse>> From<Result<T, E>> for IpcResult<T> {
    fn from(result: Result<T, E>) -> Self {
        match result {
            Ok(value) => IpcResult::Ok { value },
            Err(e) => IpcResult::Err { error: e.into() },
        }
    }
}

/// Internal application error type with rich error information.
/// This is converted to `AppErrorResponse` when crossing the IPC boundary.
#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum AppError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Modpkg error: {0}")]
    Modpkg(String),

    #[error("League installation not found")]
    LeagueNotFound,

    #[error("Invalid path: {0}")]
    InvalidPath(String),

    #[error("Mod not found: {0}")]
    ModNotFound(String),

    #[error("Validation failed: {0}")]
    ValidationFailed(String),

    #[error("Internal state error: {0}")]
    InternalState(String),

    #[error("Failed to acquire mutex lock")]
    MutexLockFailed,

    #[error("{0}")]
    Other(String),
}

impl From<AppError> for AppErrorResponse {
    fn from(error: AppError) -> Self {
        match error {
            AppError::Io(e) => AppErrorResponse::new(ErrorCode::Io, e.to_string()),

            AppError::Serialization(e) => {
                AppErrorResponse::new(ErrorCode::Serialization, e.to_string())
            }

            AppError::Modpkg(msg) => AppErrorResponse::new(ErrorCode::Modpkg, msg),

            AppError::LeagueNotFound => {
                AppErrorResponse::new(ErrorCode::LeagueNotFound, "League installation not found")
            }

            AppError::InvalidPath(path) => {
                AppErrorResponse::new(ErrorCode::InvalidPath, format!("Invalid path: {}", path))
                    .with_context(serde_json::json!({ "path": path }))
            }

            AppError::ModNotFound(id) => {
                AppErrorResponse::new(ErrorCode::ModNotFound, format!("Mod not found: {}", id))
                    .with_context(serde_json::json!({ "modId": id }))
            }

            AppError::ValidationFailed(msg) => {
                AppErrorResponse::new(ErrorCode::ValidationFailed, msg)
            }

            AppError::InternalState(msg) => AppErrorResponse::new(ErrorCode::InternalState, msg),

            AppError::MutexLockFailed => {
                AppErrorResponse::new(ErrorCode::MutexLockFailed, "Failed to acquire mutex lock")
            }

            AppError::Other(msg) => AppErrorResponse::new(ErrorCode::Unknown, msg),
        }
    }
}

/// Convenience type alias for internal Result usage
pub type AppResult<T> = Result<T, AppError>;

/// Extension trait for converting `Result<T, PoisonError>` to `AppResult<T>`.
pub trait MutexResultExt<T> {
    fn mutex_err(self) -> AppResult<T>;
}

impl<T, E> MutexResultExt<T> for Result<T, std::sync::PoisonError<E>> {
    fn mutex_err(self) -> AppResult<T> {
        self.map_err(|_| AppError::MutexLockFailed)
    }
}
