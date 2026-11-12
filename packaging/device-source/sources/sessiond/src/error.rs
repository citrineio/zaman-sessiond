use std::error::Error;
use std::io;

pub type DynError = Box<dyn Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, DynError>;

pub fn message(message: impl Into<String>) -> DynError {
    io::Error::new(io::ErrorKind::Other, message.into()).into()
}

pub fn invalid_input(message: impl Into<String>) -> DynError {
    io::Error::new(io::ErrorKind::InvalidInput, message.into()).into()
}
