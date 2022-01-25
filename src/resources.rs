use std::{fmt, fs, io};
use std::cell::RefCell;
use std::io::{Cursor, Read};
use std::iter::FilterMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use ahash::{AHashMap, AHashSet};
use glam::Vec4Swizzles;
use image::{GenericImage, GenericImageView};
use lazy_static::lazy_static;
use path_slash::{PathBufExt, PathExt};
use crate::{fname, geom, gl, minecraft, ResourceLocation, util, renderer};
use serde::{Deserialize, Deserializer};
use serde::de::{Error, IntoDeserializer};
use crate::make_a_hash_map;
use crate::fname::FName;
use crate::fname::CommonFNames;
use crate::util::make_fast_dash_map;
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

    static ref BUILTIN_MODELS: AHashMap<FName, &'static str> = make_a_hash_map!(
        FName::new(ResourceLocation::quickedit("block/empty")) => include_str!("../res/pack/empty.json"),
        FName::new(ResourceLocation::quickedit("block/white_shulker_box")) => include_str!("../res/pack/white_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/orange_shulker_box")) => include_str!("../res/pack/orange_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/magenta_shulker_box")) => include_str!("../res/pack/magenta_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/light_blue_shulker_box")) => include_str!("../res/pack/light_blue_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/yellow_shulker_box")) => include_str!("../res/pack/yellow_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/lime_shulker_box")) => include_str!("../res/pack/lime_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/pink_shulker_box")) => include_str!("../res/pack/pink_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/gray_shulker_box")) => include_str!("../res/pack/gray_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/light_gray_shulker_box")) => include_str!("../res/pack/light_gray_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/cyan_shulker_box")) => include_str!("../res/pack/cyan_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/purple_shulker_box")) => include_str!("../res/pack/purple_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/blue_shulker_box")) => include_str!("../res/pack/blue_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/brown_shulker_box")) => include_str!("../res/pack/brown_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/green_shulker_box")) => include_str!("../res/pack/green_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/red_shulker_box")) => include_str!("../res/pack/red_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/black_shulker_box")) => include_str!("../res/pack/black_shulker_box.json"),
    );

    static ref PARENT_INJECTS: AHashMap<FName, FName> = make_a_hash_map!(
        fname::from_str("block/air") => FName::new(ResourceLocation::quickedit("block/empty")),
        fname::from_str("block/water") => FName::new(ResourceLocation::quickedit("block/empty")),
        fname::from_str("block/lava") => FName::new(ResourceLocation::quickedit("block/empty")),
        fname::from_str("block/moving_piston") => FName::new(ResourceLocation::quickedit("block/empty")),

        fname::from_str("block/light_00") => FName::new(ResourceLocation::quickedit("block/empty")),
        fname::from_str("block/light_01") => FName::new(ResourceLocation::quickedit("block/empty")),
        fname::from_str("block/light_02") => FName::new(ResourceLocation::quickedit("block/empty")),
        fname::from_str("block/light_03") => FName::new(ResourceLocation::quickedit("block/empty")),
        fname::from_str("block/light_04") => FName::new(ResourceLocation::quickedit("block/empty")),
        fname::from_str("block/light_05") => FName::new(ResourceLocation::quickedit("block/empty")),
        fname::from_str("block/light_06") => FName::new(ResourceLocation::quickedit("block/empty")),
        fname::from_str("block/light_07") => FName::new(ResourceLocation::quickedit("block/empty")),
        fname::from_str("block/light_08") => FName::new(ResourceLocation::quickedit("block/empty")),
        fname::from_str("block/light_09") => FName::new(ResourceLocation::quickedit("block/empty")),
        fname::from_str("block/light_10") => FName::new(ResourceLocation::quickedit("block/empty")),
        fname::from_str("block/light_11") => FName::new(ResourceLocation::quickedit("block/empty")),
        fname::from_str("block/light_12") => FName::new(ResourceLocation::quickedit("block/empty")),
        fname::from_str("block/light_13") => FName::new(ResourceLocation::quickedit("block/empty")),
        fname::from_str("block/light_14") => FName::new(ResourceLocation::quickedit("block/empty")),
        fname::from_str("block/light_15") => FName::new(ResourceLocation::quickedit("block/empty")),

        fname::from_str("block/white_shulker_box") => FName::new(ResourceLocation::quickedit("block/white_shulker_box")),
        fname::from_str("block/orange_shulker_box") => FName::new(ResourceLocation::quickedit("block/orange_shulker_box")),
        fname::from_str("block/magenta_shulker_box") => FName::new(ResourceLocation::quickedit("block/magenta_shulker_box")),
        fname::from_str("block/light_blue_shulker_box") => FName::new(ResourceLocation::quickedit("block/light_blue_shulker_box")),
        fname::from_str("block/yellow_shulker_box") => FName::new(ResourceLocation::quickedit("block/yellow_shulker_box")),
        fname::from_str("block/lime_shulker_box") => FName::new(ResourceLocation::quickedit("block/lime_shulker_box")),
        fname::from_str("block/pink_shulker_box") => FName::new(ResourceLocation::quickedit("block/pink_shulker_box")),
        fname::from_str("block/gray_shulker_box") => FName::new(ResourceLocation::quickedit("block/gray_shulker_box")),
        fname::from_str("block/light_gray_shulker_box") => FName::new(ResourceLocation::quickedit("block/light_gray_shulker_box")),
        fname::from_str("block/cyan_shulker_box") => FName::new(ResourceLocation::quickedit("block/cyan_shulker_box")),
        fname::from_str("block/purple_shulker_box") => FName::new(ResourceLocation::quickedit("block/purple_shulker_box")),
        fname::from_str("block/blue_shulker_box") => FName::new(ResourceLocation::quickedit("block/blue_shulker_box")),
        fname::from_str("block/brown_shulker_box") => FName::new(ResourceLocation::quickedit("block/brown_shulker_box")),
        fname::from_str("block/green_shulker_box") => FName::new(ResourceLocation::quickedit("block/green_shulker_box")),
        fname::from_str("block/red_shulker_box") => FName::new(ResourceLocation::quickedit("block/red_shulker_box")),
        fname::from_str("block/black_shulker_box") => FName::new(ResourceLocation::quickedit("block/black_shulker_box")),
    );

    static ref BUILTIN_TEXTURES: AHashMap<FName, Vec<u8>> = make_a_hash_map!(
        FName::new(ResourceLocation::quickedit("block/white_shulker_box_side")) => include_bytes!("../res/pack/white_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/white_shulker_box_bottom")) => include_bytes!("../res/pack/white_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/orange_shulker_box_side")) => include_bytes!("../res/pack/orange_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/orange_shulker_box_bottom")) => include_bytes!("../res/pack/orange_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/magenta_shulker_box_side")) => include_bytes!("../res/pack/magenta_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/magenta_shulker_box_bottom")) => include_bytes!("../res/pack/magenta_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/light_blue_shulker_box_side")) => include_bytes!("../res/pack/light_blue_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/light_blue_shulker_box_bottom")) => include_bytes!("../res/pack/light_blue_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/yellow_shulker_box_side")) => include_bytes!("../res/pack/yellow_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/yellow_shulker_box_bottom")) => include_bytes!("../res/pack/yellow_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/lime_shulker_box_side")) => include_bytes!("../res/pack/lime_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/lime_shulker_box_bottom")) => include_bytes!("../res/pack/lime_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/pink_shulker_box_side")) => include_bytes!("../res/pack/pink_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/pink_shulker_box_bottom")) => include_bytes!("../res/pack/pink_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/gray_shulker_box_side")) => include_bytes!("../res/pack/gray_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/gray_shulker_box_bottom")) => include_bytes!("../res/pack/gray_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/light_gray_shulker_box_side")) => include_bytes!("../res/pack/light_gray_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/light_gray_shulker_box_bottom")) => include_bytes!("../res/pack/light_gray_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/cyan_shulker_box_side")) => include_bytes!("../res/pack/cyan_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/cyan_shulker_box_bottom")) => include_bytes!("../res/pack/cyan_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/purple_shulker_box_side")) => include_bytes!("../res/pack/purple_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/purple_shulker_box_bottom")) => include_bytes!("../res/pack/purple_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/blue_shulker_box_side")) => include_bytes!("../res/pack/blue_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/blue_shulker_box_bottom")) => include_bytes!("../res/pack/blue_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/brown_shulker_box_side")) => include_bytes!("../res/pack/brown_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/brown_shulker_box_bottom")) => include_bytes!("../res/pack/brown_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/green_shulker_box_side")) => include_bytes!("../res/pack/green_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/green_shulker_box_bottom")) => include_bytes!("../res/pack/green_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/red_shulker_box_side")) => include_bytes!("../res/pack/red_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/red_shulker_box_bottom")) => include_bytes!("../res/pack/red_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/black_shulker_box_side")) => include_bytes!("../res/pack/black_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/black_shulker_box_bottom")) => include_bytes!("../res/pack/black_shulker_box_bottom.png").to_vec(),
    );
}

pub const MISSINGNO_DATA: &[u8] = include_bytes!("../res/missingno.png");

#[derive(Default)]
pub struct Resources {
    blockstates: AHashMap<FName, BlockstateFile>,
    block_models: AHashMap<FName, BlockModel>,
    pub block_atlas: TextureAtlas,
    pub mipmap_levels: u32,

    biomes: AHashMap<FName, minecraft::BiomeData>,
    tint_data: AHashMap<FName, TintData>,
    grass_colormap: Option<image::RgbaImage>,
    foliage_colormap: Option<image::RgbaImage>,
}

trait ResourcePack {
    fn get_reader<'a>(&'a mut self, path: &str) -> io::Result<Option<Box<dyn io::Read + 'a>>>;
    fn get_sub_files(&self, path: &str, suffix: &str) -> Vec<String>;
}

struct BuiltinResourcePack;

impl ResourcePack for BuiltinResourcePack {
    fn get_reader<'a>(&'a mut self, path: &str) -> io::Result<Option<Box<dyn Read + 'a>>> {
        fn do_get_reader<'a>(path: &str) -> Option<Box<dyn Read + 'a>> {
            let path = path.strip_prefix("assets/")?;
            let (namespace, path) = path.split_at(path.find('/')?);
            let path = path.strip_prefix('/').unwrap();
            let (typ, path) = path.split_at(path.find('/')?);
            let path = path.strip_prefix('/').unwrap();
            if typ == "models" {
                let path = path.strip_suffix(".json")?;
                let text = BUILTIN_MODELS.get(&FName::new(ResourceLocation::new(namespace, path)))?;
                Some(Box::new(Cursor::new(text.as_bytes())))
            } else if typ == "textures" {
                let path = path.strip_suffix(".png")?;
                let bytes = BUILTIN_TEXTURES.get(&FName::new(ResourceLocation::new(namespace, path)))?;
                Some(Box::new(Cursor::new(bytes)))
            } else {
                None
            }
        }
        Ok(do_get_reader(path))
    }

    fn get_sub_files(&self, _path: &str, _suffix: &str) -> Vec<String> {
        Vec::new()
    }
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
            let mut model: PartialBlockModel = match serde_json::from_reader(util::ReadDelegate::new(&mut *model_reader)) {
                Ok(model) => model,
                Err(e) => {
                    eprintln!("Error parsing model {}: {}", model_id, e);
                    loaded_models.insert(model_id, RefCell::new(None));
                    continue
                }
            };
            if let Some(injected_parent) = PARENT_INJECTS.get(&model_id) {
                model.parent = Some(injected_parent.clone());
            }

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

        resources.mipmap_levels = 4;
        resources.block_atlas = stitch(&textures, &mut resources.mipmap_levels, *MAX_SUPPORTED_TEXTURE_SIZE, *MAX_SUPPORTED_TEXTURE_SIZE).unwrap();
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

    fn load_colormap(resource_packs: &mut [Box<dyn ResourcePack>], typ: &str) -> Option<image::RgbaImage> {
        match Resources::get_resource(resource_packs, format!("assets/minecraft/textures/colormap/{}.png", typ).as_str()) {
            Ok(Some(mut reader)) => {
                let mut image_data = Vec::new();
                if reader.read_to_end(&mut image_data).is_err() {
                    eprintln!("Error reading {} colormap", typ);
                } else {
                    match image::load_from_memory_with_format(&image_data, image::ImageFormat::Png) {
                        Ok(image) => return Some(image.to_rgba8()),
                        Err(e) => eprintln!("Error loading {} colormap: {}", typ, e),
                    }
                }
            }
            Ok(None) => eprintln!("Error loading {} colormap", typ),
            Err(e) => eprintln!("Error loading {} colormap: {}", typ, e)
        }
        None
    }

    fn load_data(mc_version: &str, resource_packs: &mut [Box<dyn ResourcePack>], resources: &mut Resources) {
        match minecraft::get_biome_data(mc_version) {
            Ok(biome_data) => resources.biomes = biome_data,
            Err(e) => eprintln!("Error loading biome data: {}", e)
        }

        match minecraft::get_tint_data(mc_version) {
            Ok(tint_data) => {
                for grass_data in tint_data.grass.data {
                    for biome in grass_data.keys {
                        resources.tint_data.entry(biome).or_default().grass = glam::IVec3::new((grass_data.color) >> 16 & 0xff, (grass_data.color >> 8) & 0xff, grass_data.color & 0xff);
                    }
                }
                for foliage_data in tint_data.foliage.data {
                    for biome in foliage_data.keys {
                        resources.tint_data.entry(biome).or_default().foliage = glam::IVec3::new((foliage_data.color) >> 16 & 0xff, (foliage_data.color >> 8) & 0xff, foliage_data.color & 0xff);
                    }
                }
                for water_data in tint_data.water.data {
                    for biome in water_data.keys {
                        resources.tint_data.entry(biome).or_default().water = glam::IVec3::new((water_data.color) >> 16 & 0xff, (water_data.color >> 8) & 0xff, water_data.color & 0xff);
                    }
                }
            }
            Err(e) => eprintln!("Error loading tint data: {}", e)
        }

        resources.grass_colormap = Resources::load_colormap(resource_packs, "grass");
        resources.foliage_colormap = Resources::load_colormap(resource_packs, "foliage");
    }

    pub fn load(mc_version: &str, resource_packs: &[&PathBuf], interaction_handler: &mut dyn minecraft::DownloadInteractionHandler) -> Option<Resources> {
        let mut resources = Resources::default();
        let mut resource_pack_list: Vec<Box<dyn ResourcePack>> = vec![Box::new(BuiltinResourcePack{})];
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

        Resources::load_data(mc_version, &mut resource_pack_list, &mut resources);

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

    pub fn get_biome_data(&self, biome: &FName) -> Option<&minecraft::BiomeData> {
        self.biomes.get(biome)
    }

    pub fn get_tint_data(&self, tint: &FName) -> Option<&TintData> {
        self.tint_data.get(tint)
    }

    fn get_from_colormap(colormap: &Option<image::RgbaImage>, x: u32, y: u32) -> Option<glam::IVec3> {
        colormap.as_ref().and_then(|colormap| {
            if x >= colormap.width() || y >= colormap.height() {
                None
            } else {
                Some(glam::IVec4::from(colormap.get_pixel(x, y).0.map(|i| i as i32)).xyz())
            }
        })
    }

    pub fn get_grass_color(&self, x: u32, y: u32) -> Option<glam::IVec3> {
        Resources::get_from_colormap(&self.grass_colormap, x, y)
    }

    pub fn get_foliage_color(&self, x: u32, y: u32) -> Option<glam::IVec3> {
        Resources::get_from_colormap(&self.foliage_colormap, x, y)
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
    pub ambient_occlusion: bool,
    pub textures: AHashMap<String, FName>,
    pub elements: Vec<ModelElement>,
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
    #[serde(default, deserialize_with = "deserialize_cullface")]
    pub cullface: Option<geom::Direction>,
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

fn deserialize_cullface<'de, D>(deserializer: D) -> Result<Option<geom::Direction>, D::Error> where D: Deserializer<'de> {
    struct MyVisitor;
    impl<'de> serde::de::Visitor<'de> for MyVisitor {
        type Value = Option<geom::Direction>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a cullface")
        }

        fn visit_str<E>(self, v: &str) -> Result<Option<geom::Direction>, E> where E: Error {
            return if v == "bottom" {
                Ok(Some(geom::Direction::Down))
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
    pub transparency: renderer::Transparency,
}

fn stitch<P: image::Pixel<Subpixel=u8> + 'static, I: image::GenericImageView<Pixel=P>>(
    textures: &AHashMap<FName, I>,
    mipmap_level: &mut u32,
    max_width: u32,
    max_height: u32
) -> Option<TextureAtlas> {
    let mut textures: Vec<_> = textures.iter().collect();
    textures.sort_by_key(|&(_, texture)| (!texture.width(), !texture.height()));
    for (_, texture) in &textures {
        *mipmap_level = (*mipmap_level).min(texture.width().trailing_zeros().min(texture.height().trailing_zeros()));
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
    let transparencies = make_fast_dash_map();
    unsafe {
        util::parallel_iter_to_output(&leafs, &mut atlas, |leaf, atlas| {
            if let SlotData::Leaf(name, texture) = leaf.data {
                atlas.copy_from(texture, leaf.x, leaf.y).unwrap();
                let transparency = calc_transparency(texture);
                transparencies.insert(name, transparency);
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
                transparency: *transparencies.get(name).unwrap(),
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

fn calc_transparency<P: image::Pixel<Subpixel=u8>, I: image::GenericImageView<Pixel=P>>(texture: &I) -> renderer::Transparency {
    let mut seen_transparent_pixel = false;
    for (_, _, pixel) in texture.pixels() {
        let alpha = pixel.to_rgba()[3];
        if alpha != 255 {
            if alpha != 0 {
                return renderer::Transparency::Translucent;
            }
            seen_transparent_pixel = true;
        }
    }
    if seen_transparent_pixel {
        renderer::Transparency::Transparent
    } else {
        renderer::Transparency::Opaque
    }
}

pub struct TintData {
    pub grass: glam::IVec3,
    pub foliage: glam::IVec3,
    pub water: glam::IVec3,
}

impl Default for TintData {
    fn default() -> Self {
        TintData {
            grass: glam::IVec3::new(255, 255, 255),
            foliage: glam::IVec3::new(255, 255, 255),
            water: glam::IVec3::new(255, 255, 255),
        }
    }
}
