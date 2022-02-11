use std::cell::RefCell;
use std::io;
use std::path::PathBuf;
use ahash::{AHashMap, AHashSet};
use image::GenericImageView;
use crate::fname::FName;
use crate::{CommonFNames, minecraft, ResourceLocation};
use crate::resources;
use crate::resources::builtin::{BuiltinResourcePack, PARENT_INJECTS};
use crate::resources::resource_packs::{get_resource, get_resource_pack, ResourcePack};
use crate::resources::{atlas, Resources};
use crate::resources::atlas::MAX_SUPPORTED_TEXTURE_SIZE;
use crate::resources::structs::{Animation, BlockModel, BlockstateFile, PartialBlockModel, TextureVariable};

#[profiling::function]
fn load_resource_pack(_mc_version: &str, resource_pack: &mut dyn ResourcePack, resources: &mut Resources) {
    for namespace in resource_pack.get_sub_files("assets/", "/") {
        for block_name in resource_pack.get_sub_files(&format!("assets/{}/blockstates/", &namespace), ".json") {
            let blockstate_path = format!("assets/{}/blockstates/{}.json", namespace, block_name);
            let blockstate_reader = match resource_pack.get_reader(&blockstate_path) {
                Ok(Some(reader)) => reader,
                _ => continue
            };
            let blockstate: BlockstateFile = match serde_json::from_reader(blockstate_reader) {
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

#[profiling::function]
fn load_resources(mc_version: &str, resource_packs: &mut [Box<dyn ResourcePack>], resources: &mut Resources) {
    load_models(mc_version, resource_packs, resources);
    load_textures(mc_version, resource_packs, resources);
}

#[profiling::function]
fn load_models(_mc_version: &str, resource_packs: &mut [Box<dyn ResourcePack>], resources: &mut Resources) {
    let mut models_to_load = AHashSet::new();
    for blockstate in resources.blockstates.values() {
        match blockstate {
            BlockstateFile::Variants(variants) => {
                for pair in &variants.pairs {
                    for variant in &*pair.value {
                        models_to_load.insert(variant.model.clone());
                    }
                }
            }
            BlockstateFile::Multipart(multipart) => {
                for multipart in multipart {
                    for apply in &*multipart.apply {
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

        let model_reader = match get_resource(resource_packs, format!("assets/{}/models/{}.json", model_id.namespace, model_id.name).as_str()) {
            Ok(Some(reader)) => reader,
            _ => {
                eprintln!("Error loading model {}", model_id);
                loaded_models.insert(model_id, RefCell::new(None));
                continue
            }
        };
        let mut model: PartialBlockModel = match serde_json::from_reader(model_reader) {
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

#[profiling::function]
fn load_textures(_mc_version: &str, resource_packs: &mut [Box<dyn ResourcePack>], resources: &mut Resources) {
    let mut textures = AHashMap::new();
    for model in resources.block_models.values() {
        for texture in model.textures.values() {
            if textures.contains_key(texture) {
                continue;
            }
            let png_data = {
                let mut texture_reader = match get_resource(resource_packs, format!("assets/{}/textures/{}.png", texture.namespace, texture.name).as_str()) {
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
            let animation: Option<Animation> = match get_resource(resource_packs, format!("assets/{}/textures/{}.png.mcmeta", texture.namespace, texture.name).as_str()) {
                Ok(Some(reader)) => {
                    match serde_json::from_reader(reader) {
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

    textures.insert(CommonFNames.MISSINGNO.clone(), image::load_from_memory_with_format(resources::builtin::MISSINGNO_DATA, image::ImageFormat::Png).unwrap().to_rgba8());

    resources.mipmap_levels = 4;
    resources.block_atlas = atlas::stitch(&textures, &mut resources.mipmap_levels, *MAX_SUPPORTED_TEXTURE_SIZE, *MAX_SUPPORTED_TEXTURE_SIZE).unwrap();
}

#[profiling::function]
fn load_colormap(resource_packs: &mut [Box<dyn ResourcePack>], typ: &str) -> Option<image::RgbaImage> {
    match get_resource(resource_packs, format!("assets/minecraft/textures/colormap/{}.png", typ).as_str()) {
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

    resources.grass_colormap = load_colormap(resource_packs, "grass");
    resources.foliage_colormap = load_colormap(resource_packs, "foliage");
}

#[profiling::function]
pub fn load(mc_version: &str, resource_packs: &[&PathBuf], interaction_handler: &mut dyn minecraft::DownloadInteractionHandler) -> Option<Resources> {
    let mut resources = Resources::default();
    let mut resource_pack_list: Vec<Box<dyn ResourcePack>> = vec![Box::new(BuiltinResourcePack)];
    for resource_pack in resource_packs.iter().rev() {
        let resource_pack = match get_resource_pack(resource_pack) {
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
    resource_pack_list.push(get_resource_pack(&minecraft_jar).ok()?);

    for pack in &mut resource_pack_list {
        load_resource_pack(mc_version, &mut **pack, &mut resources);
    }
    load_resources(mc_version, &mut resource_pack_list, &mut resources);

    load_data(mc_version, &mut resource_pack_list, &mut resources);

    Some(resources)
}
