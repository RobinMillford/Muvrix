use thiserror::Error;

#[derive(Error, Debug)]
pub enum MuvrixError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Core engine error: {0}")]
    Engine(String),

    #[error("Database error: {0}")]
    Database(#[from] crate::database::DatabaseError),

    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),
}

pub type Result<T> = std::result::Result<T, MuvrixError>;
