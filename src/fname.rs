#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

use ahash::AHashMap;
use internment::ArcIntern;
use lazy_static::lazy_static;
use crate::ResourceLocation;

pub type FName = ArcIntern<ResourceLocation>;

pub fn from_str<T: Into<String>>(s: T) -> FName {
    FName::new(ResourceLocation::minecraft(s))
}

macro_rules! common_fnames {
    ($($name:ident = $value:expr;)*) => {
        pub struct _CommonFNames {
            $(
                pub $name: FName,
            )*
        }
        impl _CommonFNames {
            fn new() -> Self {
                Self {
                    $(
                        $name: FName::new(ResourceLocation::minecraft($value)),
                    )*
                }
            }
        }
    };
}
lazy_static! {
    pub static ref CommonFNames: _CommonFNames = _CommonFNames::new();

    static ref NUMBERS: AHashMap<FName, u32> = {
        let mut numbers = AHashMap::new();
        for i in 0..=16 {
            numbers.insert(from_str(format!("{}", i)), i);
        }
        numbers
    };
}

common_fnames! {
    // common dimensions
    OVERWORLD = "overworld";
    THE_NETHER = "the_nether";
    THE_END = "the_end";

    // common blocks
    AIR = "air";
    STONE = "stone";
    GRASS = "grass";
    DIRT = "dirt";
    BEDROCK = "bedrock";
    WATER = "water";
    LAVA = "lava";
    FLOWING_WATER = "flowing_water";
    FLOWING_LAVA = "flowing_lava";
    SAND = "sand";
    GRAVEL = "gravel";
    ICE = "ice";
    FROSTED_ICE = "frosted_ice";
    CAULDRON = "cauldron";
    WATER_CAULDRON = "water_cauldron";

    // common states
    HALF = "half";
    UPPER = "upper";
    VARIANT = "variant";
    OAK = "oak";
    SPRUCE = "spruce";
    BIRCH = "birch";
    JUNGLE = "jungle";
    AGE = "age";
    LEVEL = "level";
    ZERO = "0";
    ONE = "1";
    POWER = "power";
    SNOWY = "snowy";
    WATERLOGGED = "waterlogged";

    // common biomes
    OCEAN = "ocean";
    PLAINS = "plains";

    // textures
    MISSINGNO = "missingno";
    WATER_STILL = "block/water_still";
    WATER_FLOW = "block/water_flow";
    WATER_OVERLAY = "block/water_overlay";
    LAVA_STILL = "block/lava_still";
    LAVA_FLOW = "block/lava_flow";
}

pub fn to_int(value: &FName) -> Option<u32> {
    NUMBERS.get(value).cloned()
}
