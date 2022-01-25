#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]

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
    SAND = "sand";
    GRAVEL = "gravel";

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
    POWER = "power";
    SNOWY = "snowy";
    TRUE = "true";

    // textures
    MISSINGNO = "missingno";
}