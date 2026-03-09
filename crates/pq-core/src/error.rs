use thiserror::Error;

#[derive(Error, Debug)]
pub enum PqError {
    #[error("Failed to open file '{path}': {source}")]
    FileOpen {
        path: String,
        source: std::io::Error,
    },

    #[error("Failed to read parquet file '{path}': {source}")]
    ParquetRead {
        path: String,
        source: parquet::errors::ParquetError,
    },

    #[error("Parquet error: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),

    #[error("Arrow error: {0}")]
    Arrow(#[from] arrow::error::ArrowError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Column '{name}' not found in schema")]
    ColumnNotFound { name: String },

    #[error("Invalid row range: offset {offset} exceeds total rows {total}")]
    InvalidRowRange { offset: usize, total: usize },

    #[error("Object store error: {0}")]
    ObjectStore(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, PqError>;
