use thiserror::Error;

#[derive(Debug, Error)]
pub enum ModEngineError {
    #[error("i/o error reading archive: {0}")]
    Io(#[from] std::io::Error),
    #[error("archive is not a valid zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("archive contains no recognizable mod content")]
    Empty,
    #[error("refusing unsafe archive path (traversal or absolute): {0}")]
    UnsafePath(String),
    #[error("archive entry not found: {0}")]
    EntryNotFound(String),
}

pub type Result<T> = std::result::Result<T, ModEngineError>;
