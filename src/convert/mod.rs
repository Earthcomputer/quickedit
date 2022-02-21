use std::collections::BTreeMap;
use std::hash::{BuildHasher, Hash};
pub use quickedit_convert_macro::*;
use serde::Deserialize;

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

pub trait ConvertFrom<T>: Sized {
    fn convert_from(input: T) -> Result<Self>;
}

impl<T, U> ConvertFrom<Option<T>> for Option<U>
    where
        U: ConvertFrom<T>,
{
    fn convert_from(input: Option<T>) -> Result<Self> {
        input.map(ConvertFrom::convert_from).transpose()
    }
}

impl<T, U> ConvertFrom<Vec<T>> for Vec<U>
    where
        U: ConvertFrom<T>,
{
    fn convert_from(input: Vec<T>) -> Result<Self> {
        input.into_iter().map(ConvertFrom::convert_from).collect()
    }
}

#[allow(clippy::disallowed_types)]
impl<K, T, U, S> ConvertFrom<std::collections::HashMap<K, T, S>> for std::collections::HashMap<K, U, S>
    where
        K: Eq + Hash,
        U: ConvertFrom<T> + Eq,
        S: BuildHasher + Default,
{
    fn convert_from(input: std::collections::HashMap<K, T, S>) -> Result<Self> {
        input.into_iter().map(|(k, v)| ConvertFrom::convert_from(v).map(|v| (k, v))).collect()
    }
}

impl<K, T, U> ConvertFrom<BTreeMap<K, T>> for BTreeMap<K, U>
    where
        K: Ord,
        U: ConvertFrom<T>
{
    fn convert_from(input: BTreeMap<K, T>) -> Result<Self> {
        input.into_iter().map(|(k, v)| ConvertFrom::convert_from(v).map(|v| (k, v))).collect()
    }
}

pub fn get_version<'de, D>(deserializer: D) -> std::result::Result<u32, D::Error>
where D: serde::Deserializer<'de> {
    #[derive(Deserialize)]
    struct VersionFinder {
        #[serde(rename = "DataVersion")]
        data_version: u32,
    }
    let version_finder: VersionFinder = serde::Deserialize::deserialize(deserializer)?;
    Ok(version_finder.data_version)
}
