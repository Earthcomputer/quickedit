use std::mem;
use ahash::AHashMap;
use num_integer::Integer;
use crate::fname::FName;
use crate::world::IBlockState;

macro_rules! define_paletted_data {
    ($name:ident, $type:ty, $h_bits:expr, $v_bits:expr, $default_palette_size:expr) => {
        pub(super) struct $name {
            entries_per_long: u8,
            bits_per_block: u8,
            palette: Vec<$type>,
            inv_palette: AHashMap<$type, usize>,
            data: Vec<u64>,
        }

        impl $name {
            pub(super) fn new() -> Self {
                let bits_per_block = $default_palette_size.log2() as u8;
                let entries_per_long = 64_u8 / bits_per_block;
                $name {
                    bits_per_block,
                    entries_per_long,
                    palette: Vec::with_capacity($default_palette_size),
                    inv_palette: AHashMap::new(),
                    data: Vec::with_capacity(((1_usize << $h_bits) * (1_usize << $h_bits) * (1_usize << $v_bits)).div_ceil(entries_per_long as usize)),
                }
            }

            pub(super) fn direct_init(palette: Vec<$type>, data: Vec<u64>) -> Self {
                let (bits_per_block, entries_per_long) = if data.is_empty() {
                    (0, 0)
                } else {
                    let bits_per_block = (((palette.len() - 1).log2() + 1) as u8).max($default_palette_size.log2() as u8);
                    let entries_per_long = 64_u8 / bits_per_block;
                    (bits_per_block, entries_per_long)
                };
                let mut inv_palette = AHashMap::new();
                for (i, v) in palette.iter().enumerate() {
                    inv_palette.insert(v.clone(), i);
                }
                $name {
                    bits_per_block,
                    entries_per_long,
                    palette,
                    inv_palette,
                    data,
                }
            }

            pub(super) fn get(&self, x: usize, y: usize, z: usize) -> &$type {
                if self.data.is_empty() {
                    return &self.palette[0];
                }
                let index = y << ($h_bits + $h_bits) | z << $h_bits | x;
                let (bit, inbit) = index.div_mod_floor(&(self.entries_per_long as usize));
                return &self.palette[((self.data[bit] >> (inbit * self.bits_per_block as usize)) & ((1 << self.bits_per_block) - 1)) as usize];
            }

            pub(super) fn set(&mut self, x: usize, y: usize, z: usize, value: &$type) {
                let val = match self.inv_palette.get(value) {
                    Some(val) => *val,
                    None => {
                        if self.palette.len() == 1 << self.bits_per_block {
                            self.resize();
                        }
                        self.palette.push(value.clone());
                        self.inv_palette.insert(value.clone(), self.palette.len() - 1);
                        self.palette.len() - 1
                    }
                };
                let index = y << ($h_bits + $h_bits) | z << $h_bits | x;
                let (bit, inbit) = index.div_mod_floor(&(self.entries_per_long as usize));
                self.data[bit] &= !(((1 << self.bits_per_block) - 1) << (inbit * self.bits_per_block as usize));
                self.data[bit] |= (val as u64) << (inbit * self.bits_per_block as usize);
            }

            #[profiling::function]
            fn resize(&mut self) {
                if self.data.is_empty() {
                    self.bits_per_block = $default_palette_size.log2() as u8;
                    self.entries_per_long = 64_u8 / self.bits_per_block;
                    self.data = vec![0; ((1_usize << $h_bits) * (1_usize << $h_bits) * (1_usize << $v_bits)).div_ceil(self.entries_per_long as usize)];
                    return;
                }
                let old_data_size = ((1 << $h_bits) * (1 << $h_bits) * (1 << $v_bits)).div_ceil(&(self.entries_per_long as usize));
                let old_bits_per_block = self.bits_per_block;
                let old_entries_per_long = self.entries_per_long;
                self.palette.reserve(self.palette.len());
                self.bits_per_block += 1;
                self.entries_per_long = 64_u8.div_floor(self.bits_per_block);
                let old_data = mem::replace(&mut self.data, Vec::with_capacity(((1 << $h_bits) * (1 << $h_bits) * (1 << $v_bits)).div_ceil(&(self.entries_per_long as usize))));
                let mut block = 0;
                for index in 0..old_data_size - 1 {
                    let word = old_data[index];
                    for i in 0..old_entries_per_long {
                        let entry = (word >> (i * old_bits_per_block)) & ((1 << old_bits_per_block) - 1);
                        block = (block << self.bits_per_block) | entry;
                        if (index * old_entries_per_long as usize + i as usize + 1) % self.entries_per_long as usize == 0 {
                            self.data.push(block);
                            block = 0;
                        }
                    }
                }
                if ((1_u64 << $h_bits) * (1_u64 << $h_bits) * (1_u64 << $v_bits)) % self.entries_per_long as u64 != 0 {
                    self.data.push(block);
                }
            }
        }
    };
}

define_paletted_data!(BlockData, IBlockState, 4_usize, 4_usize, 16_usize);
define_paletted_data!(BiomeData, FName, 2_usize, 2_usize, 2_usize);