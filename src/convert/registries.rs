use ahash::AHashMap;
use lazy_static::lazy_static;
use crate::fname;
use crate::fname::FName;

#[derive(Debug, Default)]
struct Entry {
    down_version: u32,
    up_renames: AHashMap<FName, FName>,
    down_renames: AHashMap<FName, FName>,
}

#[derive(Debug, Default)]
struct Table {
    table: Vec<Entry>,
}

impl Table {
    fn translate<'a, 'b: 'a>(&'b self, mut name: &'a FName, from_version: u32, to_version: u32) -> &'a FName {
        if from_version == to_version {
            return name;
        }
        let mut index = match self.table.binary_search_by_key(&from_version, |entry| entry.down_version) {
            Ok(index) => index,
            Err(index) => index,
        };
        if from_version > to_version {
            while index > 0 && self.table[index - 1].down_version >= to_version {
                name = self.table[index - 1].down_renames.get(name).unwrap_or(name);
                index -= 1;
            }
        } else {
            while index < self.table.len() && self.table[index].down_version < to_version {
                name = self.table[index].up_renames.get(name).unwrap_or(name);
                index += 1;
            }
        }
        name
    }
}

macro_rules! make_table {
    ($($version:expr => {$($down_name:expr => $up_name:expr),*$(,)*}),*$(,)*) => {
        {
            let mut table = Table::default();
            $(
                let mut entry = Entry::default();
                $(
                    let down_name = fname::from_str($down_name);
                    let up_name = fname::from_str($up_name);
                    entry.down_renames.insert(up_name.clone(), down_name.clone());
                    entry.up_renames.insert(down_name, up_name);
                )*
                table.table.push(entry);
            )*
            table.table.sort_by_key(|entry| entry.down_version);
            table
        }
    }
}

lazy_static! {
    static ref BIOME_RENAMES: Table = make_table! {
        V1_17_1 => {
            "badlands_plateau" => "badlands",
            "bamboo_jungle_hills" => "bamboo_jungle",
            "birch_forest_hills" => "birch_forest",
            "dark_forest_hills" => "dark_forest",
            "desert_hills" => "desert",
            "desert_lakes" => "desert",
            "giant_spruce_taiga_hills" => "old_growth_spruce_taiga",
            "giant_spruce_taiga" => "old_growth_spruce_taiga",
            "giant_tree_taiga_hills" => "old_growth_pine_taiga",
            "giant_tree_taiga" => "old_growth_pine_taiga",
            "gravelly_mountains" => "windswept_gravelly_hills",
            "jungle_edge" => "sparse_jungle",
            "jungle_hills" => "jungle",
            "modified_badlands_plateau" => "badlands",
            "modified_gravelly_mountains" => "windswept_gravelly_hills",
            "modified_jungle_edge" => "sparse_jungle",
            "modified_jungle" => "jungle",
            "modified_wooded_badlands_plateau" => "wooded_badlands",
            "mountain_edge" => "windswept_hills",
            "mountains" => "windswept_hills",
            "mushroom_field_shore" => "mushroom_fields",
            "shattered_savanna" => "windswept_savanna",
            "shattered_savanna_plateau" => "windswept_savanna",
            "snowy_mountains" => "snowy_plains",
            "snowy_taiga_hills" => "snowy_taiga",
            "snowy_taiga_mountains" => "snowy_taiga",
            "snowy_tundra" => "snowy_plains",
            "stone_shore" => "stony_shore",
            "swamp_hills" => "swamp",
            "taiga_hills" => "taiga",
            "taiga_mountains" => "taiga",
            "tall_birch_forest" => "old_growth_birch_forest",
            "tall_birch_hills" => "old_growth_birch_forest",
            "wooded_badlands_plateau" => "wooded_badlands",
            "wooded_hills" => "forest",
            "wooded_mountains" => "windswept_forest",
            "lofty_peaks" => "jagged_peaks",
            "snowcapped_peaks" => "frozen_peaks",
        }
    };
}

pub fn rename_biome(name: &FName, from_version: u32, to_version: u32) -> &FName {
    BIOME_RENAMES.translate(name, from_version, to_version)
}
