use std::sync::Arc;
use ahash::AHashMap;
use glam::Vec4Swizzles;
use crate::fname::FName;
use crate::minecraft;
use crate::renderer::BakedModel;
use crate::resources::atlas::TextureAtlas;
use crate::resources::structs::{BlockModel, BlockstateFile, MultipartWhen, TintData, TransformedModel};
use crate::util::FastDashMap;
use crate::world::IBlockState;

pub mod atlas;
mod builtin;
pub mod loader;
mod resource_packs;
pub mod structs;

#[derive(Default)]
pub struct Resources {
    pub baked_model_cache: FastDashMap<IBlockState, Arc<BakedModel>>,

    blockstates: AHashMap<FName, BlockstateFile>,
    block_models: AHashMap<FName, BlockModel>,
    pub block_atlas: TextureAtlas,
    pub mipmap_levels: u32,

    biomes: AHashMap<FName, minecraft::BiomeData>,
    tint_data: AHashMap<FName, TintData>,
    grass_colormap: Option<image::RgbaImage>,
    foliage_colormap: Option<image::RgbaImage>,
}


impl Resources {

        pub fn get_block_model(&self, state: &IBlockState) -> Option<Vec<TransformedModel>> {
        let blockstate = self.blockstates.get(&state.block)?;

        let model_variants = match blockstate {
            BlockstateFile::Variants(variants) => {
                let mut model_variant = None;
                for pair in &variants.pairs {
                    if pair.properties.iter().all(|(k, v)| state.properties.get(k).map(|v2| v == v2).unwrap_or(true)) {
                        model_variant = Some((*pair.value).first()?);
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
                    for apply in &*case.apply {
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

