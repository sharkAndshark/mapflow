use thiserror::Error;

#[derive(Debug, Error, Clone)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("IO error: {0}")]
    Io(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("File upload error: {0}")]
    FileUpload(String),

    #[error("Tile generation error: {0}")]
    TileGeneration(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Internal server error: {0}")]
    Internal(String),
}

impl AppError {
    pub fn code(&self) -> u16 {
        match self {
            AppError::Database(_) => 5000,
            AppError::Io(_) => 5001,
            AppError::Parse(_) => 5002,
            AppError::Validation(_) => 4000,
            AppError::Configuration(_) => 5003,
            AppError::FileUpload(_) => 5004,
            AppError::TileGeneration(_) => 5005,
            AppError::NotFound(_) => 4004,
            AppError::Internal(_) => 5006,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            AppError::Database(msg) => msg,
            AppError::Io(msg) => msg,
            AppError::Parse(msg) => msg,
            AppError::Validation(msg) => msg,
            AppError::Configuration(msg) => msg,
            AppError::FileUpload(msg) => msg,
            AppError::TileGeneration(msg) => msg,
            AppError::NotFound(msg) => msg,
            AppError::Internal(msg) => msg,
        }
    }

    pub fn http_status(&self) -> u16 {
        match self {
            AppError::Database(_)
            | AppError::Io(_)
            | AppError::Configuration(_)
            | AppError::FileUpload(_)
            | AppError::TileGeneration(_)
            | AppError::Internal(_) => 500,
            AppError::Validation(_) => 400,
            AppError::Parse(_) => 400,
            AppError::NotFound(_) => 404,
        }
    }

    pub fn detail(&self) -> Option<String> {
        match self {
            AppError::Database(msg) => Some(format!("DB operation failed: {}", msg)),
            AppError::Io(msg) => Some(format!("File operation failed: {}", msg)),
            AppError::Parse(msg) => Some(format!("Parsing failed: {}", msg)),
            AppError::Validation(msg) => Some(format!("Validation failed: {}", msg)),
            AppError::Configuration(msg) => Some(format!("Configuration error: {}", msg)),
            AppError::FileUpload(msg) => Some(format!("Upload failed: {}", msg)),
            AppError::TileGeneration(msg) => Some(format!("Tile generation failed: {}", msg)),
            AppError::NotFound(msg) => Some(format!("Resource not found: {}", msg)),
            AppError::Internal(msg) => Some(format!("Internal error: {}", msg)),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::Io(err.to_string())
    }
}

impl From<duckdb::Error> for AppError {
    fn from(err: duckdb::Error) -> Self {
        AppError::Database(err.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::Parse(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
