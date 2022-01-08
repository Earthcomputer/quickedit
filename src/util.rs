use std::fmt;
use std::fmt::Formatter;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use dashmap::DashMap;
use delegate::delegate;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResourceLocation {
    pub namespace: String,
    pub name: String,
}

impl ResourceLocation {
    pub fn minecraft<T: Into<String>>(name: T) -> ResourceLocation {
        ResourceLocation {
            namespace: "minecraft".to_string(),
            name: name.into(),
        }
    }

    pub fn new<T: Into<String>, U: Into<String>>(namespace: T, name: U) -> Self {
        ResourceLocation {
            namespace: namespace.into(),
            name: name.into(),
        }
    }

    pub fn to_nice_string(&self) -> String {
        if self.namespace == "minecraft" {
            self.name.clone()
        } else {
            format!("{}:{}", self.namespace, self.name)
        }
    }
}

impl FromStr for ResourceLocation {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.contains(':') {
            let mut parts = s.split(':');
            let namespace = parts.next().unwrap().to_string();
            let name = parts.next().unwrap().to_string();
            Ok(ResourceLocation { namespace, name })
        } else {
            Ok(ResourceLocation {
                namespace: "minecraft".to_string(),
                name: s.to_string(),
            })
        }
    }
}

impl fmt::Display for ResourceLocation {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.name)
    }
}

pub type FastDashMap<K, V> = DashMap<K, V, ahash::RandomState>;
pub fn make_fast_dash_map<K, V>() -> FastDashMap<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    DashMap::with_hasher(ahash::RandomState::default())
}

pub fn is_dir(path: &Path) -> bool {
    if path.is_dir() {
        return true;
    }
    let mut path: PathBuf = path.to_path_buf();
    while let Ok(linked) = path.read_link() {
        path = linked;
    }
    path.is_dir()
}

pub struct ReadDelegate<'a> {
    delegate: &'a mut dyn std::io::Read
}

//noinspection RsTraitImplementation
impl<'a> std::io::Read for ReadDelegate<'a> {
    delegate! {
        to self.delegate {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize>;
            fn read_vectored(&mut self, bufs: &mut [std::io::IoSliceMut<'_>]) -> std::io::Result<usize>;
            fn is_read_vectored(&self) -> bool;
            fn read_to_end(&mut self, buf: &mut Vec<u8>) -> std::io::Result<usize>;
            fn read_to_string(&mut self, buf: &mut String) -> std::io::Result<usize>;
            fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()>;
            fn read_buf(&mut self, buf: &mut std::io::ReadBuf<'_>) -> std::io::Result<()>;
            fn read_buf_exact(&mut self, buf: &mut std::io::ReadBuf<'_>) -> std::io::Result<()>;
        }
    }
}

impl<'a> ReadDelegate<'a> {
    pub fn new(delegate: &'a mut dyn std::io::Read) -> Self {
        ReadDelegate { delegate }
    }
}
