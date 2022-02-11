use std::io;
use std::io::{Cursor, Read};
use ahash::AHashMap;
use lazy_static::lazy_static;
use crate::fname;
use crate::fname::FName;
use crate::make_a_hash_map;
use crate::ResourceLocation;
use crate::resources::resource_packs::ResourcePack;

lazy_static! {
    static ref BUILTIN_MODELS: AHashMap<FName, &'static str> = make_a_hash_map!(
        FName::new(ResourceLocation::quickedit("block/empty")) => include_str!("../../res/pack/empty.json"),
        FName::new(ResourceLocation::quickedit("block/white_shulker_box")) => include_str!("../../res/pack/white_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/orange_shulker_box")) => include_str!("../../res/pack/orange_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/magenta_shulker_box")) => include_str!("../../res/pack/magenta_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/light_blue_shulker_box")) => include_str!("../../res/pack/light_blue_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/yellow_shulker_box")) => include_str!("../../res/pack/yellow_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/lime_shulker_box")) => include_str!("../../res/pack/lime_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/pink_shulker_box")) => include_str!("../../res/pack/pink_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/gray_shulker_box")) => include_str!("../../res/pack/gray_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/light_gray_shulker_box")) => include_str!("../../res/pack/light_gray_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/cyan_shulker_box")) => include_str!("../../res/pack/cyan_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/purple_shulker_box")) => include_str!("../../res/pack/purple_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/blue_shulker_box")) => include_str!("../../res/pack/blue_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/brown_shulker_box")) => include_str!("../../res/pack/brown_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/green_shulker_box")) => include_str!("../../res/pack/green_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/red_shulker_box")) => include_str!("../../res/pack/red_shulker_box.json"),
        FName::new(ResourceLocation::quickedit("block/black_shulker_box")) => include_str!("../../res/pack/black_shulker_box.json"),
    );

    pub(super) static ref PARENT_INJECTS: AHashMap<FName, FName> = make_a_hash_map!(
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
        FName::new(ResourceLocation::quickedit("block/white_shulker_box_side")) => include_bytes!("../../res/pack/white_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/white_shulker_box_bottom")) => include_bytes!("../../res/pack/white_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/orange_shulker_box_side")) => include_bytes!("../../res/pack/orange_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/orange_shulker_box_bottom")) => include_bytes!("../../res/pack/orange_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/magenta_shulker_box_side")) => include_bytes!("../../res/pack/magenta_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/magenta_shulker_box_bottom")) => include_bytes!("../../res/pack/magenta_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/light_blue_shulker_box_side")) => include_bytes!("../../res/pack/light_blue_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/light_blue_shulker_box_bottom")) => include_bytes!("../../res/pack/light_blue_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/yellow_shulker_box_side")) => include_bytes!("../../res/pack/yellow_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/yellow_shulker_box_bottom")) => include_bytes!("../../res/pack/yellow_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/lime_shulker_box_side")) => include_bytes!("../../res/pack/lime_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/lime_shulker_box_bottom")) => include_bytes!("../../res/pack/lime_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/pink_shulker_box_side")) => include_bytes!("../../res/pack/pink_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/pink_shulker_box_bottom")) => include_bytes!("../../res/pack/pink_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/gray_shulker_box_side")) => include_bytes!("../../res/pack/gray_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/gray_shulker_box_bottom")) => include_bytes!("../../res/pack/gray_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/light_gray_shulker_box_side")) => include_bytes!("../../res/pack/light_gray_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/light_gray_shulker_box_bottom")) => include_bytes!("../../res/pack/light_gray_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/cyan_shulker_box_side")) => include_bytes!("../../res/pack/cyan_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/cyan_shulker_box_bottom")) => include_bytes!("../../res/pack/cyan_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/purple_shulker_box_side")) => include_bytes!("../../res/pack/purple_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/purple_shulker_box_bottom")) => include_bytes!("../../res/pack/purple_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/blue_shulker_box_side")) => include_bytes!("../../res/pack/blue_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/blue_shulker_box_bottom")) => include_bytes!("../../res/pack/blue_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/brown_shulker_box_side")) => include_bytes!("../../res/pack/brown_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/brown_shulker_box_bottom")) => include_bytes!("../../res/pack/brown_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/green_shulker_box_side")) => include_bytes!("../../res/pack/green_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/green_shulker_box_bottom")) => include_bytes!("../../res/pack/green_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/red_shulker_box_side")) => include_bytes!("../../res/pack/red_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/red_shulker_box_bottom")) => include_bytes!("../../res/pack/red_shulker_box_bottom.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/black_shulker_box_side")) => include_bytes!("../../res/pack/black_shulker_box_side.png").to_vec(),
        FName::new(ResourceLocation::quickedit("block/black_shulker_box_bottom")) => include_bytes!("../../res/pack/black_shulker_box_bottom.png").to_vec(),
    );
}

pub const MISSINGNO_DATA: &[u8] = include_bytes!("../../res/missingno.png");

pub(super) struct BuiltinResourcePack;

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