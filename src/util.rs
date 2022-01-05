use std::fmt;
use std::fmt::Formatter;
use std::hash::Hash;
use std::str::FromStr;
use dashmap::DashMap;

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
