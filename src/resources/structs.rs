use std::fmt;
use std::iter::FilterMap;
use ahash::AHashMap;
use lazy_static::lazy_static;
use serde::{Deserialize, Deserializer};
use serde::de::Error;
use crate::fname::FName;
use crate::{fname, geom, util};

#[derive(Deserialize)]
pub(super) enum BlockstateFile {
    #[serde(rename = "variants")]
    Variants(VariantPairs),
    #[serde(rename = "multipart")]
    Multipart(Vec<MultipartCase>),
}

pub(super) struct VariantPairs {
    pub(super) pairs: Vec<VariantPair>,
}

pub(super) struct VariantPair {
    pub(super) properties: AHashMap<FName, FName>,
    pub(super) value: util::ListOrSingleT<ModelVariant>,
}

impl<'de> Deserialize<'de> for VariantPairs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        struct MyVisitor;
        impl<'de> serde::de::Visitor<'de> for MyVisitor {
            type Value = VariantPairs;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a map of variants")
            }

            fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error> where V: serde::de::MapAccess<'de> {
                let mut pairs = Vec::new();
                while let Some((key, value)) = map.next_entry::<String, util::ListOrSingleT<ModelVariant>>()? {
                    let mut properties = AHashMap::new();
                    for prop in key.split(',') {
                        if prop.is_empty() {
                            continue;
                        }
                        let (prop, val) = prop.split_at(prop.find('=').ok_or_else(|| V::Error::custom("Invalid variant key format"))?);
                        let val = val.strip_prefix('=').unwrap();
                        properties.insert(fname::from_str(prop), fname::from_str(val));
                    }
                    pairs.push(VariantPair{ properties, value });
                }
                Ok(VariantPairs{pairs})
            }
        }
        deserializer.deserialize_map(MyVisitor{})
    }
}

#[derive(Deserialize)]
pub(super) struct ModelVariant {
    pub(super) model: FName,
    #[serde(default)]
    pub(super) x: i32,
    #[serde(default)]
    pub(super) y: i32,
    #[serde(default)]
    pub(super) uvlock: bool,
    #[serde(default = "default_one")]
    weight: i32,
}

pub struct TransformedModel<'a> {
    pub model: &'a BlockModel,
    pub x_rotation: i32,
    pub y_rotation: i32,
    pub uvlock: bool,
}

fn default_one<T: num_traits::PrimInt>() -> T {
    T::one()
}

#[derive(Deserialize)]
pub(super) struct MultipartCase {
    pub(super) when: Option<MultipartWhen>,
    pub(super) apply: util::ListOrSingleT<ModelVariant>,
}

pub(super) enum MultipartWhen {
    Union(Vec<MultipartWhen>),
    Intersection(AHashMap<FName, Vec<FName>>),
}

impl<'de> Deserialize<'de> for MultipartWhen {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        struct MyVisitor;
        impl<'de> serde::de::Visitor<'de> for MyVisitor {
            type Value = MultipartWhen;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a multipart when")
            }

            fn visit_map<V>(self, mut map: V) -> Result<MultipartWhen, V::Error> where V: serde::de::MapAccess<'de> {
                let mut result = MultipartWhen::Intersection(AHashMap::new());
                while let Some(key) = map.next_key()? {
                    lazy_static! {
                        static ref OR: FName = fname::from_str("OR");
                    }
                    if key == *OR {
                        let value: Vec<MultipartWhen> = map.next_value()?;
                        let allowed = match &result {
                            MultipartWhen::Union(_) => false,
                            MultipartWhen::Intersection(map) => map.is_empty(),
                        };
                        if !allowed {
                            return Err(serde::de::Error::custom("OR cannot be mixed with other conditions"));
                        }
                        result = MultipartWhen::Union(value);
                    } else {
                        let value: String = map.next_value()?;
                        match &mut result {
                            MultipartWhen::Union(_) => return Err(serde::de::Error::custom("OR cannot be mixed with other conditions")),
                            MultipartWhen::Intersection(map) => {
                                // deserialize value to string
                                map.insert(key, value.split('|').map(fname::from_str).collect());
                            },
                        }
                    }
                }
                Ok(result)
            }
        }
        deserializer.deserialize_any(MyVisitor{})
    }
}

pub struct BlockModel {
    pub ambient_occlusion: bool,
    pub textures: AHashMap<String, FName>,
    pub elements: Vec<ModelElement>,
}

#[derive(Deserialize)]
pub(super) struct PartialBlockModel {
    pub(super) parent: Option<FName>,
    #[serde(rename = "ambientocclusion", default = "default_true")]
    pub(super) ambient_occlusion: bool,
    #[serde(default)]
    pub(super) textures: AHashMap<String, TextureVariable>,
    pub(super) elements: Option<Vec<ModelElement>>,
}

#[derive(Clone)]
pub enum TextureVariable {
    Ref(String),
    Imm(util::ResourceLocation),
}

#[derive(Clone, Deserialize)]
pub struct ModelElement {
    #[serde(deserialize_with = "deserialize_float_coord")]
    pub from: glam::Vec3,
    #[serde(deserialize_with = "deserialize_float_coord")]
    pub to: glam::Vec3,
    #[serde(default)]
    pub rotation: ElementRotation,
    #[serde(default = "default_true")]
    pub shade: bool,
    #[serde(default)]
    pub faces: ElementFaces,
}

#[derive(Clone, Deserialize)]
pub struct ElementRotation {
    #[serde(deserialize_with = "deserialize_float_coord")]
    pub origin: glam::Vec3,
    pub axis: geom::Axis,
    pub angle: f32,
    #[serde(default)]
    pub rescale: bool,
}

#[derive(Clone, Default, Deserialize)]
pub struct ElementFaces {
    up: Option<ElementFace>,
    down: Option<ElementFace>,
    north: Option<ElementFace>,
    south: Option<ElementFace>,
    west: Option<ElementFace>,
    east: Option<ElementFace>,
}

impl<'a> IntoIterator for &'a ElementFaces {
    type Item = (geom::Direction, &'a ElementFace);
    type IntoIter = FilterMap<<Vec<(geom::Direction, &'a Option<ElementFace>)> as IntoIterator>::IntoIter, fn((geom::Direction, &'a Option<ElementFace>)) -> Option<(geom::Direction, &'a ElementFace)>>;

    fn into_iter(self) -> Self::IntoIter {
        vec![
            (geom::Direction::Up, &self.up),
            (geom::Direction::Down, &self.down),
            (geom::Direction::North, &self.north),
            (geom::Direction::South, &self.south),
            (geom::Direction::West, &self.west),
            (geom::Direction::East, &self.east),
        ].into_iter().filter_map(|(dir, face)| face.as_ref().map(|face| (dir, face)))
    }
}

#[derive(Clone, Deserialize)]
pub struct ElementFace {
    pub uv: Option<Uv>,
    pub texture: String,
    #[serde(default)]
    pub rotation: u16,
    #[serde(rename = "tintindex", default = "default_tint_index")]
    pub tint_index: i32,
}

fn default_tint_index() -> i32 {
    -1
}

#[derive(Clone)]
pub struct Uv {
    pub u1: f32,
    pub v1: f32,
    pub u2: f32,
    pub v2: f32,
}

impl<'de> Deserialize<'de> for Uv {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        struct MyVisitor;
        impl<'de> serde::de::Visitor<'de> for MyVisitor {
            type Value = Uv;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a uv")
            }

            fn visit_seq<V>(self, mut seq: V) -> Result<Uv, V::Error> where V: serde::de::SeqAccess<'de> {
                let u1 = seq.next_element()?.ok_or_else(|| serde::de::Error::invalid_length(0, &self))?;
                let v1 = seq.next_element()?.ok_or_else(|| serde::de::Error::invalid_length(1, &self))?;
                let u2 = seq.next_element()?.ok_or_else(|| serde::de::Error::invalid_length(2, &self))?;
                let v2 = seq.next_element()?.ok_or_else(|| serde::de::Error::invalid_length(3, &self))?;
                Ok(Uv { u1, v1, u2, v2, })
            }
        }
        deserializer.deserialize_any(MyVisitor{})
    }
}

impl Default for ElementRotation {
    fn default() -> Self {
        ElementRotation {
            origin: glam::Vec3::new(0.0, 0.0, 0.0),
            axis: geom::Axis::X,
            angle: 0.0,
            rescale: false,
        }
    }
}

impl<'de> Deserialize<'de> for TextureVariable {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: Deserializer<'de> {
        struct MyVisitor;
        impl<'de> serde::de::Visitor<'de> for MyVisitor {
            type Value = TextureVariable;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a texture variable")
            }

            fn visit_str<E>(self, value: &str) -> Result<TextureVariable, E> where E: serde::de::Error {
                if let Some(reference) = value.strip_prefix('#') {
                    Ok(TextureVariable::Ref(reference.to_string()))
                } else {
                    Ok(TextureVariable::Imm(value.parse().map_err(|e| serde::de::Error::custom(e))?))
                }
            }
        }
        deserializer.deserialize_any(MyVisitor{})
    }
}

const fn default_true() -> bool {
    true
}

fn deserialize_float_coord<'de, D>(deserializer: D) -> Result<glam::Vec3, D::Error> where D: Deserializer<'de> {
    struct MyVisitor;
    impl<'de> serde::de::Visitor<'de> for MyVisitor {
        type Value = glam::Vec3;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a float coordinate")
        }

        fn visit_seq<V>(self, mut seq: V) -> Result<glam::Vec3, V::Error> where V: serde::de::SeqAccess<'de> {
            let x = seq.next_element()?.ok_or_else(|| serde::de::Error::invalid_length(0, &self))?;
            let y = seq.next_element()?.ok_or_else(|| serde::de::Error::invalid_length(1, &self))?;
            let z = seq.next_element()?.ok_or_else(|| serde::de::Error::invalid_length(2, &self))?;
            Ok(glam::Vec3::new(x, y, z))
        }
    }
    deserializer.deserialize_any(MyVisitor{})
}

#[derive(Deserialize)]
pub(super) struct Animation {
    #[serde(default = "default_one")]
    pub(super) width: u32,
    #[serde(default = "default_one")]
    pub(super) height: u32,
}

#[derive(Default)]
pub struct TintData {
    pub grass: Option<glam::IVec3>,
    pub foliage: Option<glam::IVec3>,
    pub water: Option<glam::IVec3>,
}
