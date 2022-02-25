use std::hash::Hash;
use ahash::AHashMap;
use lazy_static::lazy_static;
use crate::convert::data_versions::*;
use crate::{CommonFNames, fname};
use crate::fname::FName;
use crate::util;
use crate::world::{IBlockState, IBlockStateExtensions};

trait Rename<T> {
    fn rename(&self, value: &T) -> T;
}

impl<T: Eq + Hash + Clone> Rename<T> for AHashMap<T, T> {
    fn rename(&self, value: &T) -> T {
        self.get(value).unwrap_or(value).clone()
    }
}

impl<T: Clone> Rename<T> for Box<dyn (Fn(&T) -> T) + Sync + Send> {
    fn rename(&self, value: &T) -> T {
        self(value)
    }
}

#[derive(Debug)]
struct Entry<R: Rename<T> = AHashMap<FName, FName>, T: Clone = FName> {
    down_version: u32,
    up_renames: R,
    down_renames: R,
    _phantom: std::marker::PhantomData<T>,
}

impl<R: Rename<T> + Default, T: Clone> Default for Entry<R, T> {
    fn default() -> Self {
        Entry { down_version: 0, up_renames: R::default(), down_renames: R::default(), _phantom: std::marker::PhantomData }
    }
}

#[derive(Debug)]
struct Table<R: Rename<T> = AHashMap<FName, FName>, T: Clone = FName> {
    table: Vec<Entry<R, T>>,
}

impl<R: Rename<T>, T: Clone> Default for Table<R, T> {
    fn default() -> Self {
        Table { table: Vec::new() }
    }
}

impl<R: Rename<T>, T: Clone> Table<R, T> {
    fn translate(&self, name: &T, from_version: u32, to_version: u32) -> T {
        let mut name = name.clone();
        if from_version == to_version {
            return name;
        }
        let mut index = match self.table.binary_search_by_key(&from_version, |entry| entry.down_version) {
            Ok(index) => index,
            Err(index) => index,
        };
        if from_version > to_version {
            while index > 0 && self.table[index - 1].down_version >= to_version {
                name = self.table[index - 1].down_renames.rename(&name);
                index -= 1;
            }
        } else {
            while index < self.table.len() && self.table[index].down_version < to_version {
                name = self.table[index].up_renames.rename(&name);
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
                let mut entry: Entry<AHashMap<FName, FName>, FName> = Entry {
                    down_version: $version,
                    ..Default::default()
                };
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
    static ref BLOCK_RENAMES: Table = make_table! {
        V1_16_5 => {
            "grass_path" => "dirt_path",
            "cauldron" => "water_cauldron",
        }
    };

    static ref ITEM_RENAMES: Table = make_table! {
        V1_16_5 => {
            "grass_path" => "dirt_path",
        }
    };

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

    static ref BLOCK_STATE_RENAMES: Table<Box<dyn (Fn(&IBlockState) -> IBlockState) + Sync + Send>, IBlockState> = {
        macro_rules! make_state_table {
            ($($version:expr, $up_fn:ident, $down_fn:ident);*$(;)*) => {
                {
                    let mut table: Table<Box<dyn (Fn(&IBlockState) -> IBlockState) + Sync + Send>, IBlockState> = Table::default();
                    $(
                        table.table.push(Entry {
                            down_version: $version,
                            up_renames: Box::new($up_fn as fn(&IBlockState) -> IBlockState),
                            down_renames: Box::new($down_fn as fn(&IBlockState) -> IBlockState),
                            _phantom: std::marker::PhantomData,
                        });
                    )*
                    table
                }
            }
        }

        let mut table = make_state_table! {
            V1_16_5, state_upgrade_16_5, state_downgrade_16_5;
        };

        table.table.sort_by_key(|entry| entry.down_version);

        for block_rename in &BLOCK_RENAMES.table {
            let existing_index = table.table.binary_search_by_key(&block_rename.down_version, |entry| entry.down_version);
            match existing_index {
                Ok(index) => {
                    let entry: &mut Entry<_, _> = &mut table.table[index];
                    util::box_compute(&mut entry.down_renames, |existing_fn| {
                        Box::new(move |state| {
                            let state = existing_fn(state);
                            state.with_block(block_rename.down_renames.rename(&state.block))
                        })
                    });
                    util::box_compute(&mut entry.up_renames, |existing_fn| {
                        Box::new(move |state| {
                            let state = state.with_block(block_rename.up_renames.rename(&state.block));
                            existing_fn(&state)
                        })
                    });
                }
                Err(index) => {
                    table.table.insert(index, Entry {
                        down_version: block_rename.down_version,
                        up_renames: Box::new(move |state| {
                            state.with_block(block_rename.up_renames.rename(&state.block))
                        }),
                        down_renames: Box::new(move |state| {
                            state.with_block(block_rename.down_renames.rename(&state.block))
                        }),
                        _phantom: std::marker::PhantomData,
                    });
                }
            }
        }
        table
    };
}

pub fn rename_block(name: &FName, from_version: u32, to_version: u32) -> FName {
    BLOCK_RENAMES.translate(name, from_version, to_version)
}

pub fn rename_item(name: &FName, from_version: u32, to_version: u32) -> FName {
    ITEM_RENAMES.translate(name, from_version, to_version)
}

pub fn rename_biome(name: &FName, from_version: u32, to_version: u32) -> FName {
    BIOME_RENAMES.translate(name, from_version, to_version)
}

fn state_upgrade_16_5(state: &IBlockState) -> IBlockState {
    // cauldron was renamed to water_cauldron, rename back to cauldron if level is zero
    if state.block == CommonFNames.WATER_CAULDRON && state.properties.get(&CommonFNames.LEVEL).unwrap_or(&CommonFNames.ZERO) == &CommonFNames.ZERO {
        state.with_block(CommonFNames.CAULDRON.clone())
    } else {
        state.clone()
    }
}

fn state_downgrade_16_5(state: &IBlockState) -> IBlockState {
    state.clone()
}
