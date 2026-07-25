use thiserror::Error;

#[derive(Debug, Error)]
pub enum ModEngineError {
    #[error("i/o error reading archive: {0}")]
    Io(#[from] std::io::Error),
    #[error("archive is not a valid zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("could not read the 7z archive: {0}")]
    SevenZip(#[from] sevenz_rust2::Error),
    #[error("could not read the rar archive: {0}")]
    Rar(#[from] unrar::error::UnrarError),
    #[error("{0} is not a zip, 7z or rar archive")]
    UnknownFormat(String),
    #[error("archive contains no recognizable mod content")]
    Empty,
    #[error("refusing unsafe archive path (traversal or absolute): {0}")]
    UnsafePath(String),
    #[error("archive entry not found: {0}")]
    EntryNotFound(String),
}

pub type Result<T> = std::result::Result<T, ModEngineError>;
