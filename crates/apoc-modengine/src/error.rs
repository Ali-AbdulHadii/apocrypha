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
    /// The FOMOD manifest could not be read. Carries what was wrong in terms a
    /// user can act on, because the alternative to reading it is refusing the
    /// mod, and a refusal without a reason is indistinguishable from a bug.
    #[error("this mod's FOMOD installer could not be read: {0}")]
    FomodMalformed(String),
    /// The manifest was understood but asks for something this build cannot do
    /// safely. Refusing is the point: guessing would install files the author
    /// did not choose, and nobody would find out until the game misbehaved.
    #[error(
        "this mod's FOMOD installer uses {feature}, which Apocrypha cannot install safely yet. \
         Install it by hand for now. ({detail})"
    )]
    FomodUnsupported { feature: String, detail: String },
}

pub type Result<T> = std::result::Result<T, ModEngineError>;
