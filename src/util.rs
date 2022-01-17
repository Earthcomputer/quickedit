use std::fmt;
use std::fmt::Formatter;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use dashmap::DashMap;
use delegate::delegate;
use glium::implement_vertex;
use rayon::prelude::*;
use serde::Deserialize;
use serde_with::DeserializeFromStr;

#[derive(Copy, Clone)]
pub struct Vertex {
    pub position: [f32; 3],
    pub tex_coords: [f32; 2],
    pub lightmap_coords: [f32; 2],
}
implement_vertex!(Vertex, position, tex_coords, lightmap_coords);

#[derive(Debug, Clone, PartialEq, Eq, Hash, DeserializeFromStr)]
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
{
    DashMap::with_hasher(ahash::RandomState::default())
}
#[macro_export]
macro_rules! make_fast_dash_map {
    ($($k:expr => $v: expr),* $(,)?) => {
        {
            let mut map = make_fast_dash_map();
            $(
                map.insert($k.clone(), $v.clone());
            )*
            map
        }
    };
}
#[macro_export]
macro_rules! make_a_hash_map {
    ($($k:expr => $v: expr),* $(,)?) => {
        {
            let mut map = AHashMap::new();
            $(
                map.insert($k.clone(), $v.clone());
            )*
            map
        }
    };
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

pub trait DeserializeFromString {
    fn deserialize_from_string(s: &str) -> Self;
}

fn string_or_struct<'de, T, D>(deserializer: D) -> Result<T, D::Error>
    where
        T: serde::Deserialize<'de> + DeserializeFromString,
        D: serde::Deserializer<'de>,
{
    struct StringOrStruct<T>(std::marker::PhantomData<fn() -> T>);

    impl<'de, T> serde::de::Visitor<'de> for StringOrStruct<T>
    where
        T: serde::Deserialize<'de> + DeserializeFromString,
    {
        type Value = T;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("string or map")
        }

        fn visit_str<E>(self, value: &str) -> Result<T, E>
        where
            E: serde::de::Error,
        {
            Ok(DeserializeFromString::deserialize_from_string(value))
        }

        fn visit_map<M>(self, map: M) -> Result<T, M::Error>
        where
            M: serde::de::MapAccess<'de>,
        {
            serde::Deserialize::deserialize(serde::de::value::MapAccessDeserializer::new(map))
        }
    }

    deserializer.deserialize_any(StringOrStruct(std::marker::PhantomData))
}

fn list_or_single<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
    where
        T: serde::Deserialize<'de>,
        D: serde::Deserializer<'de>,
{
    struct ListOrSingle<T>(std::marker::PhantomData<fn() -> T>);

    impl<'de, T> serde::de::Visitor<'de> for ListOrSingle<T>
    where
        T: serde::Deserialize<'de>,
    {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("list or single")
        }

        fn visit_seq<S>(self, seq: S) -> Result<Vec<T>, S::Error>
        where
            S: serde::de::SeqAccess<'de>,
        {
            serde::Deserialize::deserialize(serde::de::value::SeqAccessDeserializer::new(seq))
        }

        fn visit_map<M>(self, map: M) -> Result<Vec<T>, M::Error>
        where
            M: serde::de::MapAccess<'de>,
        {
            let value: T = serde::Deserialize::deserialize(serde::de::value::MapAccessDeserializer::new(map))?;
            Ok(vec![value])
        }
    }

    deserializer.deserialize_any(ListOrSingle(std::marker::PhantomData))
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct StringOrStructT<T>
where
    T: serde::de::DeserializeOwned + DeserializeFromString,
{
    #[serde(deserialize_with = "string_or_struct")]
    value: T
}

impl<T: serde::de::DeserializeOwned + DeserializeFromString> std::ops::Deref for StringOrStructT<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T: serde::de::DeserializeOwned + DeserializeFromString> std::ops::DerefMut for StringOrStructT<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
pub struct ListOrSingleT<T>
where
    T: serde::de::DeserializeOwned,
{
    #[serde(deserialize_with = "list_or_single")]
    value: Vec<T>
}

impl<T: serde::de::DeserializeOwned> std::ops::Deref for ListOrSingleT<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Vec<T> {
        &self.value
    }
}

impl<T: serde::de::DeserializeOwned> std::ops::DerefMut for ListOrSingleT<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.value
    }
}

pub fn round_up_power_of_two<T: num_traits::PrimInt>(n: T) -> T {
    if n.is_zero() {
        return T::one();
    }
    T::one() << (T::zero().count_zeros() - (n - T::one()).leading_zeros()) as usize
}

pub unsafe fn parallel_iter_to_output<'data, D, O, F>(data: &'data D, output: &mut O, func: F)
where
    D: IntoParallelRefIterator<'data>,
    F: Fn(D::Item, &mut O) + Sync + Send,
{
    struct OutputWrapper<O>(*mut O);
    unsafe impl<O> Send for OutputWrapper<O> {}
    unsafe impl<O> Sync for OutputWrapper<O> {}
    impl<O> Clone for OutputWrapper<O> {
        fn clone(&self) -> Self {
            OutputWrapper(self.0)
        }
    }

    let wrapper = OutputWrapper(output);
    data.par_iter().for_each_with(wrapper, |wrapper, item| {
        func(item, &mut *wrapper.0);
    });
}

pub trait Lerp {
    type Output;
    fn lerp(self, other: Self, t: Self) -> Self::Output;
}

impl<T> Lerp for T
where
    T: std::ops::Add<Output=T> + std::ops::Sub<Output=T> + std::ops::Mul<Output=T> + Copy
{
    type Output = T;
    fn lerp(self, other: Self, t: Self) -> T {
        (other - self) * t + self
    }
}
