use std::cell::RefCell;
use ahash::AHashMap;
use lazy_static::lazy_static;
use crate::{CommonFNames, convert, fname};
use crate::convert::{data_versions, registries};
use crate::make_a_bi_map;
use crate::fname::FName;
use crate::util::ABiMap;
use crate::world::io::*;

thread_local! {
    pub(super) static CURRENT_DIMENSION: RefCell<FName> = RefCell::new(CommonFNames.OVERWORLD.clone());
}

lazy_static! {
    static ref BIOME_IDS_17: ABiMap<i32, FName> = make_a_bi_map!(
        0 => fname::from_str("ocean"),
        1 => fname::from_str("plains"),
        2 => fname::from_str("desert"),
        3 => fname::from_str("mountains"),
        4 => fname::from_str("forest"),
        5 => fname::from_str("taiga"),
        6 => fname::from_str("swamp"),
        7 => fname::from_str("river"),
        8 => fname::from_str("nether_wastes"),
        9 => fname::from_str("the_end"),
        10 => fname::from_str("frozen_ocean"),
        11 => fname::from_str("frozen_river"),
        12 => fname::from_str("snowy_tundra"),
        13 => fname::from_str("snowy_mountains"),
        14 => fname::from_str("mushroom_fields"),
        15 => fname::from_str("mushroom_field_shore"),
        16 => fname::from_str("beach"),
        17 => fname::from_str("desert_hills"),
        18 => fname::from_str("wooded_hills"),
        19 => fname::from_str("taiga_hills"),
        20 => fname::from_str("mountain_edge"),
        21 => fname::from_str("jungle"),
        22 => fname::from_str("jungle_hills"),
        23 => fname::from_str("jungle_edge"),
        24 => fname::from_str("deep_ocean"),
        25 => fname::from_str("stone_shore"),
        26 => fname::from_str("snowy_beach"),
        27 => fname::from_str("birch_forest"),
        28 => fname::from_str("birch_forest_hills"),
        29 => fname::from_str("dark_forest"),
        30 => fname::from_str("snowy_taiga"),
        31 => fname::from_str("snowy_taiga_hills"),
        32 => fname::from_str("giant_tree_taiga"),
        33 => fname::from_str("giant_tree_taiga_hills"),
        34 => fname::from_str("wooded_mountains"),
        35 => fname::from_str("savanna"),
        36 => fname::from_str("savanna_plateau"),
        37 => fname::from_str("badlands"),
        38 => fname::from_str("wooded_badlands_plateau"),
        39 => fname::from_str("badlands_plateau"),
        40 => fname::from_str("small_end_islands"),
        41 => fname::from_str("end_midlands"),
        42 => fname::from_str("end_highlands"),
        43 => fname::from_str("end_barrens"),
        44 => fname::from_str("warm_ocean"),
        45 => fname::from_str("lukewarm_ocean"),
        46 => fname::from_str("cold_ocean"),
        47 => fname::from_str("deep_warm_ocean"),
        48 => fname::from_str("deep_lukewarm_ocean"),
        49 => fname::from_str("deep_cold_ocean"),
        50 => fname::from_str("deep_frozen_ocean"),
        127 => fname::from_str("the_void"),
        129 => fname::from_str("sunflower_plains"),
        130 => fname::from_str("desert_lakes"),
        131 => fname::from_str("gravelly_mountains"),
        132 => fname::from_str("flower_forest"),
        133 => fname::from_str("taiga_mountains"),
        134 => fname::from_str("swamp_hills"),
        140 => fname::from_str("ice_spikes"),
        149 => fname::from_str("modified_jungle"),
        151 => fname::from_str("modified_jungle_edge"),
        155 => fname::from_str("tall_birch_forest"),
        156 => fname::from_str("tall_birch_hills"),
        157 => fname::from_str("dark_forest_hills"),
        158 => fname::from_str("snowy_taiga_mountains"),
        160 => fname::from_str("giant_spruce_taiga"),
        161 => fname::from_str("giant_spruce_taiga_hills"),
        162 => fname::from_str("modified_gravelly_mountains"),
        163 => fname::from_str("shattered_savanna"),
        164 => fname::from_str("shattered_savanna_plateau"),
        165 => fname::from_str("eroded_badlands"),
        166 => fname::from_str("modified_wooded_badlands_plateau"),
        167 => fname::from_str("modified_badlands_plateau"),
        168 => fname::from_str("bamboo_jungle"),
        169 => fname::from_str("bamboo_jungle_hills"),
        170 => fname::from_str("soul_sand_valley"),
        171 => fname::from_str("crimson_forest"),
        172 => fname::from_str("warped_forest"),
        173 => fname::from_str("basalt_deltas"),
        174 => fname::from_str("dripstone_caves"),
        175 => fname::from_str("lush_caves"),
        177 => fname::from_str("meadow"),
        178 => fname::from_str("grove"),
        179 => fname::from_str("snowy_slopes"),
        180 => fname::from_str("snowcapped_peaks"),
        181 => fname::from_str("lofty_peaks"),
        182 => fname::from_str("stony_peaks"),
    );
}

fn get_biome_name(id: i32, prevailing_version: u32) -> Option<&'static FName> {
    BIOME_IDS_17.get_by_left(&id)
        .map(|name| registries::rename_biome(name, data_versions::V1_17_1, prevailing_version))
}

fn get_biome_id(name: &FName, prevailing_version: u32) -> Option<i32> {
    BIOME_IDS_17.get_by_right(registries::rename_biome(name, prevailing_version, data_versions::V1_17_1)).cloned()
}

convert::variants! {
    pub(super) struct SerializedChunkLevel {
        #[serde(rename = "Sections")]
        #[variants]
        pub(super) sections: Vec<SerializedChunkSection17>,

        #[serde(rename = "Biomes")]
        pub(super) biomes: Vec<i32>,
    }
}

convert::variants! {
    pub(super) struct SerializedChunkSection17 {
        #[serde(rename = "Palette")]
        #[variants]
        pub(super) palette: Option<Vec<SerializedBlockState>>,

        #[serde(rename = "BlockStates")]
        #[serde(default)]
        pub(super) block_states: Vec<i64>,
    }
}

pub(super) fn biomes_17_up(biomes: &[i32], prevailing_version: u32) -> convert::Result<Vec<Variant_SerializedBiomes_1_18>> {
    let cur_dim = CURRENT_DIMENSION.with(|c| c.borrow().clone());
    let shift = cur_dim == CommonFNames.OVERWORLD && prevailing_version > data_versions::V1_17_1;

    if biomes.len() == 64 * 24 {
        (0..24).map(|y| biomes_17_up_subchunk(|index| biomes[index + y * 64] & 255, prevailing_version)).collect()
    } else if biomes.len() == 1024 {
        let mut result = Vec::with_capacity(if shift { 24 } else { 16 });
        if shift {
            for y in 0..4 {
                result.push(biomes_17_up_subchunk(|index| biomes[(index & 15) + y * 64] & 255, prevailing_version)?);
            }
        }
        for y in 0..16 {
            result.push(biomes_17_up_subchunk(|index| biomes[index + y * 64] & 255, prevailing_version)?);
        }
        if shift {
            for y in 0..4 {
                result.push(biomes_17_up_subchunk(|index| biomes[(index & 15) + 1008 + y * 64] & 255, prevailing_version)?);
            }
        }
        Ok(result)
    } else {
        let len = if shift { 24 } else { 16 };
        Ok(vec![Variant_SerializedBiomes_1_18 {
            palette: vec![CommonFNames.PLAINS.clone()],
            data: Vec::new(),
            _extra: Default::default(),
        }; len])
    }
}

fn biomes_17_up_subchunk(getter: impl Fn(usize) -> i32, prevailing_version: u32) -> convert::Result<Variant_SerializedBiomes_1_18> {
    let mut palette = Vec::new();
    let mut inv_palette = AHashMap::new();
    for i in 0..64 {
        let biome_id = getter(i);
        if inv_palette.try_insert(biome_id, palette.len()).is_ok() {
            palette.push(get_biome_name(biome_id, prevailing_version).ok_or_else(|| convert::Error::new("Encountered unknown biome ID"))?.clone());
        }
    }

    let bits_per_biome = palette.len().next_power_of_two().trailing_zeros();

    if bits_per_biome == 0 {
        return Ok(Variant_SerializedBiomes_1_18 {
            palette,
            data: Vec::new(),
            _extra: Default::default(),
        })
    }

    let biomes_per_word = 64 / bits_per_biome;
    let mut data = Vec::with_capacity(((64 + biomes_per_word - 1) / biomes_per_word) as usize);
    data.push(0);
    let mut data_index = 0;
    let mut shift = 0;
    for i in 0..64 {
        let biome_id = getter(i);
        data[data_index] |= (*inv_palette.get(&biome_id).unwrap() as i64) << shift;
        shift += bits_per_biome;
        if shift + bits_per_biome > 64 {
            shift = 0;
            data_index += 1;
            data.push(0);
        }
    }

    Ok(Variant_SerializedBiomes_1_18 {
        palette,
        data,
        _extra: Default::default(),
    })
}

pub(super) fn biomes_17_down(sections: &[Variant_SerializedChunkSection_1_18], prevailing_version: u32) -> convert::Result<Vec<i32>> {
    let mut result = Vec::with_capacity(sections.len() * 64);
    for section in sections {
        let palette: Vec<_> = section.biomes.palette.iter()
            .map(|name| get_biome_id(name, prevailing_version).ok_or_else(|| convert::Error::new(format!("Encountered unknown biome {}", name))))
            .collect::<Result<_, _>>()?;
        if palette.is_empty() {
            return Err(convert::Error::new("Empty biome palette"));
        }
        let bits_per_biome = palette.len().next_power_of_two().trailing_zeros();
        if bits_per_biome == 0 {
            for _ in 0..64 {
                result.push(palette[0]);
            }
        } else {
            let biomes_per_word = 64 / bits_per_biome;
            let expected_data_len = ((64 + biomes_per_word - 1) / biomes_per_word) as usize;
            if section.biomes.data.len() != expected_data_len {
                return Err(convert::Error::new(format!("Expected {} data words, got {}", expected_data_len, section.biomes.data.len())));
            }
            for i in 0..64 {
                let palette_index = (section.biomes.data[(i / biomes_per_word) as usize] >> (i % biomes_per_word * bits_per_biome) & ((1 << bits_per_biome) - 1)) as usize;
                if palette_index >= palette.len() {
                    return Err(convert::Error::new(format!("Palette index is out of bounds, {} >= {}", palette_index, palette.len())));
                }
                result.push(palette[palette_index]);
            }
        }
    }

    Ok(result)
}