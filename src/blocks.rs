use ahash::AHashMap;
use glam::IVec3;
use lazy_static::lazy_static;
use crate::fname;
use crate::fname::FName;
use crate::world::{World, IBlockState, Dimension};
use crate::geom::BlockPos;
use crate::{CommonFNames, make_a_hash_map};

type ColorProvider = fn(&World, &Dimension, BlockPos, &IBlockState) -> glam::IVec3;

lazy_static! {
    static ref COLOR_PROVIDERS: AHashMap<FName, ColorProvider> = make_a_hash_map!(
        fname::from_str("large_fern") => get_tall_grass_color as ColorProvider,
        fname::from_str("tall_grass") => get_tall_grass_color as ColorProvider,
        fname::from_str("grass_block") => get_grass_color_with_possible_snow as ColorProvider,
        fname::from_str("fern") => get_grass_color as ColorProvider,
        fname::from_str("grass") => get_grass_color_with_possible_snow as ColorProvider,
        fname::from_str("potted_fern") => get_grass_color as ColorProvider,
        fname::from_str("spruce_leaves") => get_spruce_color as ColorProvider,
        fname::from_str("birch_leaves") => get_birch_color as ColorProvider,
        fname::from_str("oak_leaves") => get_foliage_color as ColorProvider,
        fname::from_str("jungle_leaves") => get_foliage_color as ColorProvider,
        fname::from_str("leaves") => get_112_leaves_color as ColorProvider,
        fname::from_str("acacia_leaves") => get_foliage_color as ColorProvider,
        fname::from_str("dark_oak_leaves") => get_foliage_color as ColorProvider,
        fname::from_str("leaves2") => get_foliage_color as ColorProvider,
        fname::from_str("vine") => get_foliage_color as ColorProvider,
        fname::from_str("water") => get_water_color as ColorProvider,
        fname::from_str("flowing_water") => get_water_color as ColorProvider,
        fname::from_str("bubble_column") => get_water_color as ColorProvider,
        fname::from_str("water_cauldron") => get_water_color as ColorProvider,
        fname::from_str("cauldron") => get_112_cauldron_color as ColorProvider,
        fname::from_str("redstone_wire") => get_redstone_wire_color as ColorProvider,
        fname::from_str("sugar_cane") => get_sugar_cane_color as ColorProvider,
        fname::from_str("attached_melon_stem") => get_attached_stem_color as ColorProvider,
        fname::from_str("attached_pumpkin_stem") => get_attached_stem_color as ColorProvider,
        fname::from_str("melon_stem") => get_stem_color as ColorProvider,
        fname::from_str("pumpkin_stem") => get_stem_color as ColorProvider,
        fname::from_str("lily_pad") => get_lily_pad_color as ColorProvider,
    );
}

fn get_tall_grass_color(world: &World, dimension: &Dimension, pos: BlockPos, state: &IBlockState) -> glam::IVec3 {
    if state.properties.get(&CommonFNames.HALF) == Some(&CommonFNames.UPPER) {
        get_grass_color(world, dimension, pos + BlockPos::new(0, -1, 0), state)
    } else {
        get_grass_color(world, dimension, pos, state)
    }
}

fn get_grass_color_with_possible_snow(world: &World, dimension: &Dimension, pos: BlockPos, state: &IBlockState) -> glam::IVec3 {
    if state.properties.get(&CommonFNames.SNOWY) == Some(&CommonFNames.ONE) {
        glam::IVec3::new(255, 255, 255)
    } else {
        get_grass_color(world, dimension, pos, state)
    }
}

fn get_grass_color(world: &World, dimension: &Dimension, pos: BlockPos, _state: &IBlockState) -> glam::IVec3 {
    get_grass_color_with_default(world, dimension, pos, glam::IVec3::new(0xff, 0, 0xff))
}

fn get_grass_color_with_default(world: &World, dimension: &Dimension, pos: BlockPos, default: glam::IVec3) -> glam::IVec3 {
    let biome = dimension.get_biome(pos);
    if let Some(biome) = &biome {
        if let Some(tint_data) = world.resources.get_tint_data(biome) {
            return tint_data.grass;
        }
    }

    let (temperature, rainfall) = biome
        .and_then(|b| world.resources.get_biome_data(&b))
        .map_or_else(|| (0.5, 1.0), |b| (b.temperature, b.rainfall));
    let temperature = temperature.clamp(0.0, 1.0);
    let rainfall = rainfall.clamp(0.0, 1.0);
    let humidity = temperature * rainfall;
    let x = ((1.0 - temperature) * 255.0) as u32;
    let y = ((1.0 - humidity) * 255.0) as u32;
    world.resources.get_grass_color(x, y).unwrap_or(default)
}

fn get_spruce_color(_world: &World, _dimension: &Dimension, _pos: BlockPos, _state: &IBlockState) -> glam::IVec3 {
    return IVec3::new(0x61, 0x99, 0x61);
}

fn get_birch_color(_world: &World, _dimension: &Dimension, _pos: BlockPos, _state: &IBlockState) -> glam::IVec3 {
    return IVec3::new(0x80, 0xa7, 0x55);
}

fn get_foliage_color(world: &World, dimension: &Dimension, pos: BlockPos, _state: &IBlockState) -> glam::IVec3 {
    let biome = dimension.get_biome(pos);
    if let Some(biome) = &biome {
        if let Some(tint_data) = world.resources.get_tint_data(biome) {
            return tint_data.foliage;
        }
    }

    if let Some(biome_data) = biome.and_then(|b| world.resources.get_biome_data(&b))
    {
        let temperature = biome_data.temperature.clamp(0.0, 1.0);
        let rainfall = biome_data.rainfall.clamp(0.0, 1.0);
        let humidity = temperature * rainfall;
        let x = ((1.0 - temperature) * 255.0) as u32;
        let y = ((1.0 - humidity) * 255.0) as u32;
        world.resources.get_foliage_color(x, y).unwrap_or_else(|| glam::IVec3::new(0x48, 0xb5, 0x18))
    } else {
        glam::IVec3::new(0x48, 0xb5, 0x18)
    }
}

fn get_112_leaves_color(world: &World, dimension: &Dimension, pos: BlockPos, state: &IBlockState) -> glam::IVec3 {
    if let Some(variant) = state.properties.get(&CommonFNames.VARIANT) {
        if variant == &CommonFNames.BIRCH {
            return get_birch_color(world, dimension, pos, state);
        } else if variant == &CommonFNames.SPRUCE {
            return get_spruce_color(world, dimension, pos, state);
        }
    }
    get_foliage_color(world, dimension, pos, state)
}

fn get_water_color(world: &World, dimension: &Dimension, pos: BlockPos, _state: &IBlockState) -> glam::IVec3 {
    dimension.get_biome(pos)
        .and_then(|biome| world.resources.get_tint_data(&biome))
        .map_or_else(|| glam::IVec3::new(0xff, 0xff, 0xff), |tint_data| tint_data.water)
}

fn get_112_cauldron_color(world: &World, dimension: &Dimension, pos: BlockPos, state: &IBlockState) -> glam::IVec3 {
    if state.properties.get(&CommonFNames.LEVEL) == Some(&CommonFNames.ZERO) {
        IVec3::new(255, 255, 255)
    } else {
        get_water_color(world, dimension, pos, state)
    }
}

lazy_static! {
    static ref REDSTONE_WIRE_COLOR: AHashMap<FName, IVec3> = {
        let mut map = AHashMap::new();
        for i in 0..16 {
            let power = i as f32 / 15.0f32;
            let red = power * 0.6 + if i == 0 { 0.3 } else { 0.4 };
            let green = (power * power * 0.7 - 0.5).clamp(0.0, 1.0);
            let blue = (power * power * 0.6 - 0.7).clamp(0.0, 1.0);
            map.insert(fname::from_str(format!("{}", i)), IVec3::new((red * 255.0) as i32, (green * 255.0) as i32, (blue * 255.0) as i32));
        }
        map
    };
}

fn get_redstone_wire_color(_world: &World, _dimension: &Dimension, _pos: BlockPos, state: &IBlockState) -> glam::IVec3 {
    state.properties.get(&CommonFNames.POWER)
        .and_then(|power| REDSTONE_WIRE_COLOR.get(power).cloned())
        .unwrap_or_else(|| IVec3::new(255, 255, 255))
}

fn get_sugar_cane_color(world: &World, dimension: &Dimension, pos: BlockPos, _state: &IBlockState) -> glam::IVec3 {
    get_grass_color_with_default(world, dimension, pos, IVec3::new(0xff, 0xff, 0xff))
}

fn get_attached_stem_color(_world: &World, _dimension: &Dimension, _pos: BlockPos, _state: &IBlockState) -> glam::IVec3 {
    return IVec3::new(0xe0, 0xc7, 0x1c);
}

lazy_static! {
    static ref STEM_COLOR: AHashMap<FName, IVec3> = {
        let mut map = AHashMap::new();
        for age in 0..=7 {
            let red = age * 32;
            let green = 255 - age * 8;
            let blue = age * 4;
            map.insert(fname::from_str(format!("{}", age)), IVec3::new(red, green, blue));
        }
        map
    };
}

fn get_stem_color(_world: &World, _dimension: &Dimension, _pos: BlockPos, state: &IBlockState) -> glam::IVec3 {
    state.properties.get(&CommonFNames.AGE)
        .and_then(|age| STEM_COLOR.get(age).cloned())
        .unwrap_or_else(|| IVec3::new(255, 255, 255))
}

fn get_lily_pad_color(_world: &World, _dimension: &Dimension, _pos: BlockPos, _state: &IBlockState) -> glam::IVec3 {
    return IVec3::new(0x20, 0x80, 0x30);
}

pub fn get_block_color(world: &World, dimension: &Dimension, pos: BlockPos, state: &IBlockState) -> glam::IVec3 {
    COLOR_PROVIDERS.get(&state.block)
        .map(|provider| provider(world, dimension, pos, state))
        .unwrap_or_else(|| glam::IVec3::new(0xff, 0xff, 0xff))
}
