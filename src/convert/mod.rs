pub use quickedit_convert_macro::*;

#[derive(Debug, Clone)]
pub struct Error(String);

impl Error {
    pub fn new<T: ToString>(msg: T) -> Self {
        Error(msg.to_string())
    }

    pub fn msg(&self) -> &str {
        &self.0
    }
}

impl From<std::convert::Infallible> for Error {
    fn from(_: std::convert::Infallible) -> Self {
        unreachable!();
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub trait Up {
    type UpInput;
    type UpResult;
    fn up(older: Self::UpInput) -> Self::UpResult;
}

pub trait Down {
    type DownInput;
    type DownResult;
    fn down(newer: Self::DownInput) -> Self::DownResult;
}

pub trait VersionedSerde<'de> where Self: Sized {
    fn deserialize<D>(version: u32, deserializer: D) -> std::result::Result<Self, D::Error>
    where D: serde::Deserializer<'de>;
    fn serialize<S>(self, version: u32, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where S: serde::Serializer;
}
