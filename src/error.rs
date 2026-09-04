use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Parse(String),
    Runtime(String),
    Alloc(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Parse(m) => write!(f, "parse error: {m}"),
            Error::Runtime(m) => write!(f, "runtime error: {m}"),
            Error::Alloc(m) => write!(f, "allocation error: {m}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
