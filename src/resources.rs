use std::{fmt, fs, io};
use std::cell::RefCell;
use std::io::Read;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use ahash::{AHashMap, AHashSet};
use path_slash::{PathBufExt, PathExt};
use crate::{minecraft, ResourceLocation, util, world};
use serde::{Deserialize, Deserializer};
use serde::de::{Error, IntoDeserializer};

pub struct Resources {
    blockstates: AHashMap<ResourceLocation, BlockstateFile>,
    block_models: AHashMap<ResourceLocation, BlockModel>,
    textures: AHashMap<ResourceLocation, (Vec<u8>, u32, u32)>,
}

trait ResourcePack {
    fn get_reader<'a>(&'a mut self, path: &str) -> io::Result<Option<Box<dyn io::Read + 'a>>>;
    fn get_sub_files(&self, path: &str, suffix: &str) -> Vec<String>;
}

struct ZipResourcePack {
    zip: zip::ZipArchive<fs::File>,
    dirs: Vec<String>,
}

impl ZipResourcePack {
    fn new(file: fs::File) -> io::Result<Self> {
        let zip = zip::ZipArchive::new(file)?;
        let mut dirs = AHashSet::new();
        for filename in zip.file_names() {
            let parts: Vec<_> = filename.split('/').collect();
            for i in 0..parts.len() - 1 {
                dirs.insert(parts[..=i].join("/") + "/");
            }
        }
        Ok(Self { zip, dirs: dirs.into_iter().collect() })
    }
}

impl ResourcePack for ZipResourcePack {
    fn get_reader<'a>(&'a mut self, path: &str) -> io::Result<Option<Box<dyn io::Read + 'a>>> {
        let file = match self.zip.by_name(path) {
            Ok(file) => file,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(zip::result::ZipError::Io(e)) => return Err(e),
            Err(e) => return Err(io::Error::new(io::ErrorKind::Other, e)),
        };
        Ok(Some(Box::new(file)))
    }

    fn get_sub_files(&self, path: &str, suffix: &str) -> Vec<String> {
        let mut files = Vec::new();
        for file in self.zip.file_names().chain(self.dirs.iter().map(|d| d.as_str())) {
            if file.starts_with(path) && file[path.len()..].ends_with(suffix) && !file[path.len()..file.len()-suffix.len()].contains('/') {
                files.push(file[path.len()..file.len()-suffix.len()].to_string());
            }
        }
        files
    }
}

struct DirectoryResourcePack {
    path: PathBuf,
}

impl DirectoryResourcePack {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl ResourcePack for DirectoryResourcePack {
    fn get_reader(&mut self, path: &str) -> io::Result<Option<Box<dyn io::Read>>> {
        let file = match fs::File::open(self.path.join(PathBuf::from_slash(path))) {
            Ok(file) => file,
            Err(e) => {
                return if e.kind() == io::ErrorKind::NotFound {
                    Ok(None)
                } else {
                    Err(e)
                }
            }
        };
        Ok(Some(Box::new(file)))
    }

    fn get_sub_files(&self, path: &str, suffix: &str) -> Vec<String> {
        let mut files = Vec::new();
        let read_dir = match fs::read_dir(self.path.join(path)) {
            Ok(read_dir) => read_dir,
            Err(_) => return files,
        };
        for entry in read_dir.flatten() {
            if let Ok(relpath) = entry.path().strip_prefix(&self.path) {
                if let Some(filename) = relpath.to_slash() {
                    if let Some(filename) = filename.strip_suffix(suffix) {
                        files.push(filename.to_string());
                    }
                }
            }
        }
        files
    }
}

impl Resources {
    fn get_resource_pack(path: &Path) -> io::Result<Box<dyn ResourcePack>> {
        if util::is_dir(path) {
            Ok(Box::new(DirectoryResourcePack::new(path.to_path_buf())))
        } else {
            let file = fs::File::open(path)?;
            Ok(Box::new(ZipResourcePack::new(file)?))
        }
    }

    fn load_resource_pack(_mc_version: &str, resource_pack: &mut dyn ResourcePack, resources: &mut Resources) {
        for namespace in resource_pack.get_sub_files("assets/", "/") {
            for block_name in resource_pack.get_sub_files(&format!("assets/{}/blockstates/", &namespace), ".json") {
                let blockstate_path = format!("assets/{}/blockstates/{}.json", namespace, block_name);
                let mut blockstate_reader = match resource_pack.get_reader(&blockstate_path) {
                    Ok(Some(reader)) => reader,
                    _ => continue
                };
                let blockstate: BlockstateFile = match serde_json::from_reader(util::ReadDelegate::new(&mut *blockstate_reader)) {
                    Ok(blockstate) => blockstate,
                    Err(e) => {
                        eprintln!("Error parsing blockstate {}: {}", block_name, e);
                        continue;
                    }
                };
                resources.blockstates.entry(ResourceLocation::new(&namespace, block_name)).or_insert(blockstate);
            }
        }
    }

    fn load_resources(mc_version: &str, resource_packs: &mut [Box<dyn ResourcePack>], resources: &mut Resources) {
        Resources::load_models(mc_version, resource_packs, resources);
        Resources::load_textures(mc_version, resource_packs, resources);
    }

    fn load_models(_mc_version: &str, resource_packs: &mut [Box<dyn ResourcePack>], resources: &mut Resources) {
        let mut models_to_load = AHashSet::new();
        for blockstate in resources.blockstates.values() {
            match blockstate {
                BlockstateFile::Variants(variants) => {
                    for variants in variants.values() {
                        for variant in variants.deref() {
                            models_to_load.insert(variant.model.clone());
                        }
                    }
                }
                BlockstateFile::Multipart(multipart) => {
                    for multipart in multipart {
                        for apply in multipart.apply.deref() {
                            models_to_load.insert(apply.model.clone());
                        }
                    }
                }
            }
        }
        let concrete_models = models_to_load.clone();

        let mut loaded_models = AHashMap::new();

        while !models_to_load.is_empty() {
            let model_id = models_to_load.iter().next().unwrap().clone();
            models_to_load.remove(&model_id);

            let mut model_reader = match Resources::get_resource(resource_packs, format!("assets/{}/models/{}.json", model_id.namespace, model_id.name).as_str()) {
                Ok(Some(reader)) => reader,
                _ => {
                    eprintln!("Error loading model {}", model_id);
                    loaded_models.insert(model_id, RefCell::new(None));
                    continue
                }
            };
            let model: PartialBlockModel = match serde_json::from_reader(util::ReadDelegate::new(&mut *model_reader)) {
                Ok(model) => model,
                Err(e) => {
                    eprintln!("Error parsing model {}: {}", model_id, e);
                    loaded_models.insert(model_id, RefCell::new(None));
                    continue
                }
            };

            if let Some(parent) = &model.parent {
                if !loaded_models.contains_key(parent) {
                    models_to_load.insert(parent.clone());
                }
            }

            loaded_models.insert(model_id, RefCell::new(Some(model)));
        }

        fn flatten_model(model_id: &ResourceLocation, partial_models: &AHashMap<ResourceLocation, RefCell<Option<PartialBlockModel>>>, visited_models: &mut AHashSet<ResourceLocation>) {
            let model_ref = &partial_models[model_id];
            if visited_models.contains(model_id) {
                *model_ref.borrow_mut() = None;
                return;
            }

            let parent = {
                let model = partial_models[model_id].borrow();
                if model.is_none() {
                    return;
                }
                let model = model.as_ref().unwrap();
                if model.parent.is_none() {
                    return;
                }
                model.parent.as_ref().unwrap().clone()
            };

            visited_models.insert(model_id.clone());
            flatten_model(&parent, partial_models, visited_models);
            visited_models.remove(model_id);

            let parent_model = partial_models[&parent].borrow();
            if parent_model.is_none() {
                *model_ref.borrow_mut() = None;
                return;
            }

            let parent_model = parent_model.as_ref().unwrap();
            let mut model_mut_ref = model_ref.borrow_mut();
            let model = model_mut_ref.as_mut().unwrap();
            if model.elements.is_none() {
                model.elements = parent_model.elements.clone();
            }
            for (k, v) in &parent_model.textures {
                model.textures.entry(k.clone()).or_insert_with(|| v.clone());
            }
            model.parent = None;
        }

        let mut visited_models = AHashSet::new();
        for model_id in loaded_models.keys() {
            flatten_model(model_id, &loaded_models, &mut visited_models);
        }

        for model_id in concrete_models {
            let mut model_cell = loaded_models[&model_id].borrow_mut();
            let model = match model_cell.as_mut() {
                Some(model) => model,
                None => {
                    eprintln!("Error loading model {}", model_id);
                    continue
                }
            };

            fn flatten_textures(texture_id: &str, textures: &mut AHashMap<String, TextureVariable>, visited_textures: &mut AHashSet<String>) {
                if visited_textures.contains(texture_id) {
                    return;
                }
                if let TextureVariable::Ref(texture_ref) = &textures[texture_id] {
                    let texture_ref = texture_ref.clone();
                    if textures.contains_key(&texture_ref) {
                        visited_textures.insert(texture_id.to_string());
                        flatten_textures(&texture_ref, textures, visited_textures);
                        visited_textures.remove(texture_id);
                        textures.insert(texture_id.to_string(), textures[&texture_ref].clone());
                    }
                }
            }
            let mut new_textures = AHashMap::new();
            let mut visited_textures = AHashSet::new();
            let textures_copy: Vec<_> = model.textures.keys().cloned().collect();
            for texture_id in textures_copy {
                flatten_textures(&texture_id, &mut model.textures, &mut visited_textures);
                match &model.textures[&texture_id] {
                    TextureVariable::Imm(texture) => new_textures.insert(texture_id.to_string(), texture.clone()),
                    TextureVariable::Ref(_) => {
                        eprintln!("Error loading texture {}", texture_id);
                        continue;
                    }
                };
            }
            if model.elements.is_none() {
                eprintln!("Error loading model {}", model_id);
                continue;
            }

            resources.block_models.insert(model_id.clone(), BlockModel {
                ambient_occlusion: model.ambient_occlusion,
                textures: new_textures,
                elements: model.elements.as_ref().unwrap().clone(),
            });
        }
    }

    fn load_textures(_mc_version: &str, resource_packs: &mut [Box<dyn ResourcePack>], resources: &mut Resources) {
        for model in resources.block_models.values() {
            for texture in model.textures.values() {
                if resources.textures.contains_key(texture) {
                    continue;
                }
                let mut texture_reader = match Resources::get_resource(resource_packs, format!("assets/{}/textures/{}.png", texture.namespace, texture.name).as_str()) {
                    Ok(Some(reader)) => reader,
                    _ => {
                        eprintln!("Texture not found: {}", texture);
                        continue
                    }
                };
                let mut png_data = Vec::new();
                if texture_reader.read_to_end(&mut png_data).is_err() {
                    eprintln!("Error reading texture: {}", texture);
                    continue
                }
                let image = match image::load(io::Cursor::new(png_data), image::ImageFormat::Png) {
                    Ok(image) => image,
                    Err(err) => {
                        eprintln!("Error loading texture: {}", err);
                        continue
                    }
                }.to_rgb8();
                resources.textures.insert(texture.clone(), (image.as_raw().clone(), image.width(), image.height()));
            }
        }
    }

    fn get_resource<'a>(resource_packs: &'a mut [Box<dyn ResourcePack>], path: &str) -> io::Result<Option<Box<dyn io::Read + 'a>>> {
        for resource_pack in resource_packs {
            match resource_pack.get_reader(path) {
                Ok(Some(reader)) => return Ok(Some(reader)),
                Ok(None) => continue,
                Err(e) => return Err(e)
            }
        }
        Ok(None)
    }

    pub fn load(mc_version: &str, resource_packs: &[&PathBuf], interaction_handler: &mut dyn minecraft::DownloadInteractionHandler) -> Option<Resources> {
        let mut resources = Resources {
            blockstates: AHashMap::new(),
            block_models: AHashMap::new(),
            textures: AHashMap::new(),
        };
        let mut resource_pack_list = Vec::new();
        for resource_pack in resource_packs.iter().rev() {
            let resource_pack = match Resources::get_resource_pack(resource_pack) {
                Ok(resource_pack) => resource_pack,
                Err(e) => {
                    if let Some(string) = resource_pack.to_str() {
                        eprintln!("Error loading resource pack {}: {}", string, e);
                    } else {
                        eprintln!("Error loading resource pack: {}", e);
                    }
                    continue;
                }
            };
            resource_pack_list.push(resource_pack);
        }
        let minecraft_jar = match minecraft::get_existing_jar(mc_version) {
            Some(jar) => jar,
            None => {
                if !interaction_handler.show_download_prompt(mc_version) {
                    return None;
                }
                interaction_handler.on_start_download();
                let result = minecraft::download_jar(mc_version).ok()?;
                interaction_handler.on_finish_download();
                result
            },
        };
        resource_pack_list.push(Resources::get_resource_pack(&minecraft_jar).ok()?);

        for pack in &mut resource_pack_list {
            Resources::load_resource_pack(mc_version, &mut **pack, &mut resources);
        }
        Resources::load_resources(mc_version, &mut resource_pack_list, &mut resources);

        Some(resources)
    }
}

#[derive(Deserialize)]
pub enum BlockstateFile {
    #[serde(rename = "variants")]
    Variants(AHashMap<String, util::ListOrSingleT<ModelVariant>>),
    #[serde(rename = "multipart")]
    Multipart(Vec<MultipartCase>),
}

#[derive(Deserialize)]
pub struct ModelVariant {
    model: util::ResourceLocation,
    #[serde(default)]
    x: i32,
    #[serde(default)]
    y: i32,
    #[serde(default)]
    uvlock: bool,
    #[serde(default = "default_weight")]
    weight: i32,
}

const fn default_weight() -> i32 {
    1
}

#[derive(Deserialize)]
pub struct MultipartCase {
    when: Option<MultipartWhen>,
    apply: util::ListOrSingleT<ModelVariant>,
}

pub enum MultipartWhen {
    Union(Vec<MultipartWhen>),
    Intersection(AHashMap<String, Vec<String>>),
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
                    if key == "OR" {
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
                                map.insert(key, value.split('|').map(|s| s.to_string()).collect());
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
    ambient_occlusion: bool,
    textures: AHashMap<String, util::ResourceLocation>,
    elements: Vec<ModelElement>,
}

#[derive(Deserialize)]
struct PartialBlockModel {
    parent: Option<util::ResourceLocation>,
    #[serde(rename = "ambientocclusion", default = "default_true")]
    ambient_occlusion: bool,
    #[serde(default)]
    textures: AHashMap<String, TextureVariable>,
    elements: Option<Vec<ModelElement>>,
}

#[derive(Clone)]
pub enum TextureVariable {
    Ref(String),
    Imm(util::ResourceLocation),
}

#[derive(Clone, Deserialize)]
pub struct ModelElement {
    #[serde(deserialize_with = "deserialize_float_coord")]
    from: world::Pos<f32>,
    #[serde(deserialize_with = "deserialize_float_coord")]
    to: world::Pos<f32>,
    #[serde(default)]
    rotation: ElementRotation,
    #[serde(default = "default_true")]
    shade: bool,
    #[serde(default)]
    faces: ElementFaces,
}

#[derive(Clone, Deserialize)]
pub struct ElementRotation {
    #[serde(deserialize_with = "deserialize_float_coord")]
    origin: world::Pos<f32>,
    axis: world::Axis,
    angle: f32,
    #[serde(default)]
    rescale: bool,
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

#[derive(Clone, Deserialize)]
pub struct ElementFace {
    uv: Option<Uv>,
    texture: String,
    #[serde(default, deserialize_with = "deserialize_cullface")]
    cullface: Option<world::Direction>,
    #[serde(default)]
    rotation: u16,
    #[serde(rename = "tintindex", default = "default_tint_index")]
    tint_index: i32,
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
            origin: world::Pos::new(0.0, 0.0, 0.0),
            axis: world::Axis::X,
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

fn deserialize_float_coord<'de, D>(deserializer: D) -> Result<world::Pos<f32>, D::Error> where D: Deserializer<'de> {
    struct MyVisitor;
    impl<'de> serde::de::Visitor<'de> for MyVisitor {
        type Value = world::Pos<f32>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a float coordinate")
        }

        fn visit_seq<V>(self, mut seq: V) -> Result<world::Pos<f32>, V::Error> where V: serde::de::SeqAccess<'de> {
            let x = seq.next_element()?.ok_or_else(|| serde::de::Error::invalid_length(0, &self))?;
            let y = seq.next_element()?.ok_or_else(|| serde::de::Error::invalid_length(1, &self))?;
            let z = seq.next_element()?.ok_or_else(|| serde::de::Error::invalid_length(2, &self))?;
            Ok(world::Pos::new(x, y, z))
        }
    }
    deserializer.deserialize_any(MyVisitor{})
}

fn deserialize_cullface<'de, D>(deserializer: D) -> Result<Option<world::Direction>, D::Error> where D: Deserializer<'de> {
    struct MyVisitor;
    impl<'de> serde::de::Visitor<'de> for MyVisitor {
        type Value = Option<world::Direction>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a cullface")
        }

        fn visit_str<E>(self, v: &str) -> Result<Option<world::Direction>, E> where E: Error {
            return if v == "bottom" {
                Ok(Some(world::Direction::Down))
            } else {
                Ok(Some(Deserialize::deserialize(v.into_deserializer())?))
            }
        }
    }
    deserializer.deserialize_any(MyVisitor{})
}
