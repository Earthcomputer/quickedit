use std::{fmt, fs, io};
use std::cell::RefCell;
use std::io::Read;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use ahash::{AHashMap, AHashSet};
use image::{GenericImage, GenericImageView};
use lazy_static::lazy_static;
use path_slash::{PathBufExt, PathExt};
use crate::{fname, gl, minecraft, ResourceLocation, util, world};
use serde::{Deserialize, Deserializer};
use serde::de::{Error, IntoDeserializer};
use crate::fname::FName;
use crate::fname::CommonFNames;
use crate::world::IBlockState;

lazy_static! {
    static ref MAX_SUPPORTED_TEXTURE_SIZE: u32 = {
        let mut max_supported_texture_size = 0;
        unsafe {
            gl::GetIntegerv(gl::MAX_TEXTURE_SIZE, &mut max_supported_texture_size);
        }
        let mut actual_max = 32768.max(max_supported_texture_size);
        while actual_max >= 1024 {
            unsafe {
                gl::TexImage2D(gl::PROXY_TEXTURE_2D, 0, gl::RGBA as i32, actual_max, actual_max, 0, gl::RGBA, gl::UNSIGNED_BYTE, std::ptr::null_mut())
            };
            let mut result = 0;
            unsafe {
                gl::GetTexLevelParameteriv(gl::PROXY_TEXTURE_2D, 0, gl::TEXTURE_WIDTH, &mut result)
            };
            if result != 0 {
                return result as u32;
            }
            actual_max >>= 1;
        }
        max_supported_texture_size.max(1024) as u32
    };
}

pub const MISSINGNO_DATA: &[u8] = include_bytes!("../res/missingno.png");

#[derive(Default)]
pub struct Resources {
    blockstates: AHashMap<FName, BlockstateFile>,
    block_models: AHashMap<FName, BlockModel>,
    pub block_atlas: TextureAtlas,
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
                resources.blockstates.entry(FName::new(ResourceLocation::new(&namespace, block_name))).or_insert(blockstate);
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
                    for pair in &variants.pairs {
                        for variant in pair.value.deref() {
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

        fn flatten_model(model_id: &FName, partial_models: &AHashMap<FName, RefCell<Option<PartialBlockModel>>>, visited_models: &mut AHashSet<FName>) {
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
                    TextureVariable::Imm(texture) => new_textures.insert(texture_id.to_string(), FName::new(texture.clone())),
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
        let mut textures = AHashMap::new();
        for model in resources.block_models.values() {
            for texture in model.textures.values() {
                if textures.contains_key(texture) {
                    continue;
                }
                let png_data = {
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
                    png_data
                };
                let image = match image::load(io::Cursor::new(png_data), image::ImageFormat::Png) {
                    Ok(image) => image,
                    Err(err) => {
                        eprintln!("Error loading texture: {}", err);
                        continue
                    }
                }.to_rgba8();
                let animation: Option<Animation> = match Resources::get_resource(resource_packs, format!("assets/{}/textures/{}.png.mcmeta", texture.namespace, texture.name).as_str()) {
                    Ok(Some(mut reader)) => {
                        match serde_json::from_reader(util::ReadDelegate::new(&mut *reader)) {
                            Ok(animation) => Some(animation),
                            Err(err) => {
                                eprintln!("Error loading texture animation: {}", err);
                                continue
                            }
                        }
                    }
                    Ok(None) => None,
                    Err(err) => {
                        eprintln!("Error loading texture animation: {}", err);
                        continue
                    }
                };
                let image = if let Some(animation) = animation {
                    if animation.width > 0 {
                        let height = image.width() * animation.height / animation.width;
                        if height <= image.height() {
                            image.view(0, 0, image.width(), height).to_image()
                        } else {
                            eprintln!("Invalid texture animation: {}", texture);
                            continue
                        }
                    } else {
                        eprintln!("Invalid texture animation: {}", texture);
                        continue
                    }
                } else {
                    image
                };
                textures.insert(texture.clone(), image);
            }
        }

        textures.insert(CommonFNames.MISSINGNO.clone(), image::load_from_memory_with_format(MISSINGNO_DATA, image::ImageFormat::Png).unwrap().to_rgba8());

        resources.block_atlas = stitch(&textures, &mut 4, *MAX_SUPPORTED_TEXTURE_SIZE, *MAX_SUPPORTED_TEXTURE_SIZE).unwrap();
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
        let mut resources = Resources::default();
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

    pub fn get_block_model(&self, state: &IBlockState) -> Option<Vec<TransformedModel>> {
        let blockstate = self.blockstates.get(&state.block)?;

        let model_variants = match blockstate {
            BlockstateFile::Variants(variants) => {
                let mut model_variant = None;
                for pair in &variants.pairs {
                    if pair.properties.iter().all(|(k, v)| state.properties.get(k).map(|v2| v == v2).unwrap_or(true)) {
                        model_variant = Some(pair.value.deref().first()?);
                        break;
                    }
                }
                vec![model_variant?] // TODO: pick a random one
            }
            BlockstateFile::Multipart(cases) => {
                let mut model_variants = Vec::new();
                'case_loop:
                for case in cases {
                    fn does_when_match(when: &MultipartWhen, state: &IBlockState) -> bool {
                        match when {
                            MultipartWhen::Union(union) => {
                                union.iter().any(|when| does_when_match(when, state))
                            }
                            MultipartWhen::Intersection(intersection) => {
                                intersection.iter().all(|(prop, values)| state.properties.get(prop).map(|v| values.contains(v)).unwrap_or(true))
                            }
                        }
                    }
                    for when in &case.when {
                        if !does_when_match(when, state) {
                            continue 'case_loop;
                        }
                    }
                    for apply in case.apply.deref() {
                        model_variants.push(apply);
                    }
                }
                model_variants
            }
        };

        if model_variants.is_empty() {
            return None;
        }

        let mut transformed_models = Vec::new();
        for model_variant in model_variants {
            let model = self.block_models.get(&model_variant.model)?;
            let transformed_model = TransformedModel {
                model,
                x_rotation: model_variant.x,
                y_rotation: model_variant.y,
                uvlock: model_variant.uvlock
            };
            transformed_models.push(transformed_model);
        }
        Some(transformed_models)
    }
}

#[derive(Deserialize)]
enum BlockstateFile {
    #[serde(rename = "variants")]
    Variants(VariantPairs),
    #[serde(rename = "multipart")]
    Multipart(Vec<MultipartCase>),
}

struct VariantPairs {
    pairs: Vec<VariantPair>,
}

struct VariantPair {
    properties: AHashMap<FName, FName>,
    value: util::ListOrSingleT<ModelVariant>,
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
struct ModelVariant {
    model: FName,
    #[serde(default)]
    x: i32,
    #[serde(default)]
    y: i32,
    #[serde(default)]
    uvlock: bool,
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
struct MultipartCase {
    when: Option<MultipartWhen>,
    apply: util::ListOrSingleT<ModelVariant>,
}

enum MultipartWhen {
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
    ambient_occlusion: bool,
    textures: AHashMap<String, FName>,
    elements: Vec<ModelElement>,
}

#[derive(Deserialize)]
struct PartialBlockModel {
    parent: Option<FName>,
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

#[derive(Deserialize)]
struct Animation {
    #[serde(default = "default_one")]
    width: u32,
    #[serde(default = "default_one")]
    height: u32,
}

// ===== TEXTURE STITCHING ===== //

#[derive(Default)]
pub struct TextureAtlas {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    sprites: AHashMap<FName, Sprite>,
}

impl TextureAtlas {
    pub fn get_sprite(&self, name: &FName) -> Option<&Sprite> {
        self.sprites.get(name)
    }
}

pub struct Sprite {
    pub u1: u32,
    pub v1: u32,
    pub u2: u32,
    pub v2: u32,
}

fn stitch<P: image::Pixel<Subpixel=u8> + 'static, I: image::GenericImageView<Pixel=P>>(
    textures: &AHashMap<FName, I>,
    mipmap_level: &mut u8,
    max_width: u32,
    max_height: u32
) -> Option<TextureAtlas> {
    let mut textures: Vec<_> = textures.iter().collect();
    textures.sort_by_key(|&(_, texture)| (!texture.width(), !texture.height()));
    for (_, texture) in &textures {
        *mipmap_level = (*mipmap_level).min(texture.width().trailing_zeros().min(texture.height().trailing_zeros()) as u8);
    }

    struct Slot<'a, P: image::Pixel<Subpixel=u8>, I: image::GenericImageView<Pixel=P>> {
        x: u32, y: u32, width: u32, height: u32, data: SlotData<'a, P, I>,
    }
    unsafe impl<P: image::Pixel<Subpixel=u8>, I: image::GenericImageView<Pixel=P>> Sync for Slot<'_, P, I> {}
    enum SlotData<'a, P: image::Pixel<Subpixel=u8>, I: image::GenericImageView<Pixel=P>> {
        Empty,
        Leaf(&'a FName, &'a I),
        Node(Vec<Slot<'a, P, I>>),
    }
    impl<'a, P: image::Pixel<Subpixel=u8>, I: image::GenericImageView<Pixel=P>> Slot<'a, P, I> {
        fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
            Slot { x, y, width, height, data: SlotData::Empty }
        }

        fn fit(&mut self, name: &'a FName, sprite: &'a I) -> bool {
            if let SlotData::Leaf(_, _) = self.data {
                return false;
            }

            let sprite_width = sprite.width();
            let sprite_height = sprite.height();
            if self.width < sprite_width || self.height < sprite_height {
                return false;
            }
            if self.width == sprite_width && self.height == sprite_height {
                self.data = SlotData::Leaf(name, sprite);
                return true;
            }
            if let SlotData::Empty = self.data {
                let mut sub_slots = vec![Slot::new(self.x, self.y, sprite_width, sprite_height)];
                let leftover_x = self.width - sprite_width;
                let leftover_y = self.height - sprite_height;
                if leftover_x == 0 {
                    sub_slots.push(Slot::new(self.x, self.y + sprite_height, sprite_width, leftover_y));
                } else if leftover_y == 0 {
                    sub_slots.push(Slot::new(self.x + sprite_width, self.y, leftover_x, sprite_height));
                } else if self.height < self.width {
                    sub_slots.push(Slot::new(self.x + sprite_width, self.y, leftover_x, sprite_height));
                    sub_slots.push(Slot::new(self.x, self.y + sprite_height, self.width, leftover_y));
                } else {
                    sub_slots.push(Slot::new(self.x, self.y + sprite_height, sprite_width, leftover_y));
                    sub_slots.push(Slot::new(self.x + sprite_width, self.y, leftover_x, self.height));
                }

                self.data = SlotData::Node(sub_slots);
            }
            if let SlotData::Node(ref mut sub_slots) = self.data {
                for sub_slot in sub_slots {
                    if sub_slot.fit(name, sprite) {
                        return true;
                    }
                }
            } else {
                unreachable!();
            }
            false
        }

        fn add_leafs(&'a self, leafs: &mut Vec<&Slot<'a, P, I>>) {
            if let SlotData::Leaf(_, _) = self.data {
                leafs.push(self);
            } else if let SlotData::Node(ref sub_slots) = self.data {
                for slot in sub_slots {
                    slot.add_leafs(leafs);
                }
            }
        }
    }
    let mut width = 0;
    let mut height = 0;
    let mut slots: Vec<Slot<P, I>> = Vec::with_capacity(256);

    'texture_loop:
    for (name, texture) in &textures {
        for slot in &mut slots {
            if slot.fit(name, texture) {
                continue 'texture_loop;
            }
        }

        // grow
        let current_effective_width = util::round_up_power_of_two(width);
        let current_effective_height = util::round_up_power_of_two(height);
        let expanded_width = util::round_up_power_of_two(width + texture.width());
        let expanded_height = util::round_up_power_of_two(height + texture.height());
        let can_expand_x = expanded_width <= max_width;
        let can_expand_y = expanded_height <= max_height;
        if !can_expand_x && !can_expand_y {
            return None;
        }
        let x_has_space_without_expanding = can_expand_x && current_effective_width != expanded_width;
        let y_has_space_without_expanding = can_expand_y && current_effective_height != expanded_height;
        let use_x = if x_has_space_without_expanding ^ y_has_space_without_expanding {
            x_has_space_without_expanding
        } else {
            can_expand_x && current_effective_width <= current_effective_height
        };

        let mut slot = if use_x {
            if height == 0 {
                height = texture.height();
            }
            let slot = Slot::new(width, 0, texture.width(), height);
            width += texture.width();
            slot
        } else {
            let slot = Slot::new(0, height, width, texture.height());
            height += texture.height();
            slot
        };

        slot.fit(name, *texture);
        slots.push(slot);
    }

    width = util::round_up_power_of_two(width);
    height = util::round_up_power_of_two(height);

    let mut leafs = Vec::with_capacity(textures.len());
    for slot in &slots {
        slot.add_leafs(&mut leafs);
    }
    let mut atlas: image::ImageBuffer<P, _> = image::ImageBuffer::new(width, height);
    unsafe {
        util::parallel_iter_to_output(&leafs, &mut atlas, |leaf, atlas| {
            if let SlotData::Leaf(_, texture) = leaf.data {
                atlas.copy_from(texture, leaf.x, leaf.y).unwrap();
            } else {
                unreachable!();
            }
        });
    }
    let sprites = leafs.iter().map(|leaf| {
        if let SlotData::Leaf(name, _) = leaf.data {
            (name.clone(), Sprite {
                u1: leaf.x,
                v1: leaf.y,
                u2: leaf.x + leaf.width,
                v2: leaf.y + leaf.height,
            })
        } else {
            unreachable!()
        }
    }).collect();

    Some(TextureAtlas {
        width,
        height,
        data: atlas.into_raw(),
        sprites,
    })
}
