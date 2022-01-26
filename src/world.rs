use std::{fmt, fs, io, mem, time};
use std::borrow::{Borrow, BorrowMut};
use std::cell::RefCell;
use std::collections::btree_map::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::fmt::Formatter;
use std::hash::{Hash, Hasher};
use std::io::Cursor;
use std::mem::MaybeUninit;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use ahash::AHashMap;
use serde::{Deserialize, Serialize};
use byteorder::{BigEndian, ReadBytesExt};
use dashmap::mapref::entry::Entry;
use glam::{IVec2, Vec3Swizzles};
use internment::ArcIntern;
use lazy_static::lazy_static;
use num_integer::Integer;
use positioned_io_preview::{RandomAccessFile, ReadAt, ReadBytesAtExt};
use crate::fname::{CommonFNames, FName};
use crate::{minecraft, renderer};
use crate::geom::{BlockPos, ChunkPos};
use crate::resources;
use crate::util::{FastDashMap, make_fast_dash_map};
use crate::renderer::WorldRenderer;

lazy_static! {
    pub static ref WORLDS: RwLock<Vec<WorldRef>> = RwLock::new(Vec::new());
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockState {
    pub block: FName,
    pub properties: AHashMap<FName, FName>,
}

impl fmt::Display for BlockState {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.block)?;
        if !self.properties.is_empty() {
            write!(f, "[{}]", self.properties.iter().map(|(k, v)| format!("{}={}", k.to_nice_string(), v.to_nice_string())).collect::<Vec<_>>().join(","))?;
        }
        Ok(())
    }
}

#[allow(clippy::derive_hash_xor_eq)]
impl Hash for BlockState {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.block.hash(state);
        let mut result = 0_u64;
        for (k, v) in self.properties.iter() {
            let mut hasher = DefaultHasher::new();
            k.hash(&mut hasher);
            v.hash(&mut hasher);
            result ^= hasher.finish();
        }
        result.hash(state);
    }
}

pub type IBlockState = ArcIntern<BlockState>;

impl BlockState {
    pub fn new(block: &FName) -> Self {
        BlockState {
            block: block.clone(),
            properties: AHashMap::new(),
        }
    }
}

macro_rules! define_paletted_data {
    ($name:ident, $type:ty, $h_bits:expr, $v_bits:expr, $default_palette_size:expr) => {
        struct $name {
            entries_per_long: u8,
            bits_per_block: u8,
            palette: Vec<$type>,
            inv_palette: AHashMap<$type, usize>,
            data: Vec<u64>,
        }

        impl $name {
            fn new() -> Self {
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

            fn direct_init(palette: Vec<$type>, data: Vec<u64>) -> Self {
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

            fn get(&self, x: usize, y: usize, z: usize) -> &$type {
                if self.data.is_empty() {
                    return &self.palette[0];
                }
                let index = y << ($h_bits + $h_bits) | z << $h_bits | x;
                let (bit, inbit) = index.div_mod_floor(&(self.entries_per_long as usize));
                return &self.palette[((self.data[bit] >> (inbit * self.bits_per_block as usize)) & ((1 << self.bits_per_block) - 1)) as usize];
            }

            fn set(&mut self, x: usize, y: usize, z: usize, value: &$type) {
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
define_paletted_data!(BiomeData, FName, 2_usize, 2_usize, 4_usize);

pub struct Subchunk {
    block_data: BlockData,
    biome_data: BiomeData,

    pub baked_geometry: RefCell<Option<renderer::BakedChunkGeometry>>,
}

impl Subchunk {
    pub fn get_block_state(&self, pos: BlockPos) -> &IBlockState {
        self.block_data.get(pos.x as usize, pos.y as usize, pos.z as usize)
    }

    pub fn set_block_state(&mut self, pos: BlockPos, value: &IBlockState) {
        self.block_data.set(pos.x as usize, pos.y as usize, pos.z as usize, value);
    }

    pub fn get_biome(&self, pos: BlockPos) -> &FName {
        self.biome_data.get(pos.x as usize >> 2, pos.y as usize >> 2, pos.z as usize >> 2)
    }

    pub fn set_biome(&mut self, pos: BlockPos, value: &FName) {
        self.biome_data.set(pos.x as usize >> 2, pos.y as usize >> 2, pos.z as usize >> 2, value);
    }
}

pub struct Chunk {
    pub subchunks: Vec<Option<Subchunk>>,
}

impl Chunk {
    pub fn empty() -> Self {
        Chunk {
            subchunks: Vec::new(),
        }
    }

    pub fn get_block_state(&self, dimension: &Dimension, pos: BlockPos) -> Option<IBlockState> {
        let subchunk_index = (pos.y - dimension.min_y) >> 4;
        if subchunk_index < 0 {
            return None;
        }
        let subchunk_index = subchunk_index as usize;
        if subchunk_index >= self.subchunks.len() {
            return None;
        }
        let subchunk = self.subchunks[subchunk_index].as_ref()?;
        Some(subchunk.get_block_state(pos & glam::IVec3::new(!0, 15, !0)).clone())
    }

    pub fn get_biome(&self, dimension: &Dimension, pos: BlockPos) -> Option<FName> {
        let subchunk_index = (pos.y - dimension.min_y) >> 4;
        if subchunk_index < 0 {
            return None;
        }
        let subchunk_index = subchunk_index as usize;
        if subchunk_index >= self.subchunks.len() {
            return None;
        }
        let subchunk = self.subchunks[subchunk_index].as_ref()?;
        Some(subchunk.get_biome(pos & glam::IVec3::new(!0, 15, !0)).clone())
    }
}

pub struct Dimension {
    id: FName,
    pub min_y: i32,
    pub max_y: i32,
    chunks: FastDashMap<ChunkPos, Arc<Chunk>>,

    region_file_cache: FastDashMap<IVec2, (RandomAccessFile, time::SystemTime)>,
    chunk_existence_cache: FastDashMap<IVec2, bool>,
}

impl Dimension {
    pub fn new(id: FName) -> Self {
        Dimension {
            id,
            min_y: 0,
            max_y: 256,
            chunks: make_fast_dash_map(),
            region_file_cache: make_fast_dash_map(),
            chunk_existence_cache: make_fast_dash_map(),
        }
    }

    fn get_save_dir(&self, world: &World) -> PathBuf {
        if self.id == CommonFNames.OVERWORLD {
            world.path.clone()
        } else if self.id == CommonFNames.THE_NETHER {
            world.path.join("DIM-1")
        } else if self.id == CommonFNames.THE_END {
            world.path.join("DIM1")
        } else {
            world.path.join("dimensions").join(self.id.namespace.clone()).join(self.id.name.clone())
        }
    }

    pub fn get_block_state(&self, pos: BlockPos) -> Option<IBlockState> {
        let chunk = self.get_chunk(pos.xz() >> glam::IVec2::new(4, 4))?;
        chunk.get_block_state(self, pos & glam::IVec3::new(15, !0, 15))
    }

    pub fn get_biome(&self, pos: BlockPos) -> Option<FName> {
        let chunk = self.get_chunk(pos.xz() >> glam::IVec2::new(4, 4))?;
        chunk.get_biome(self, pos & glam::IVec3::new(15, !0, 15))
    }

    pub fn get_chunk(&self, pos: ChunkPos) -> Option<Arc<Chunk>> {
        self.chunks.view(&pos, |_, chunk| {
            chunk.clone()
        })
    }

    pub fn load_chunk(&self, world: &World, pos: ChunkPos) -> Option<Arc<Chunk>> {
        if self.chunk_existence_cache.get(&pos).map(|b| *b) == Some(false) {
            return None;
        }
        if let Some(chunk) = self.chunks.get(&pos) {
            return Some(chunk.clone());
        }
        self.chunks.entry(pos).or_try_insert_with(|| {
            match self.read_chunk(world, pos)? {
                Some(chunk) => Ok(Arc::new(chunk)),
                None => Err(io::Error::new(io::ErrorKind::NotFound, "Chunk not found"))
            }
        }).map(|r| r.clone()).map_err(|err| {
            if err.kind() != io::ErrorKind::NotFound {
                eprintln!("Failed to load chunk: {}", err);
            }
        }).ok()
    }

    pub fn does_chunk_exist(&self, world: &World, pos: ChunkPos) -> bool {
        if let Some(exists) = self.chunk_existence_cache.get(&pos) {
            return *exists;
        }

        *self.chunk_existence_cache.entry(pos).or_insert_with(|| {
            if self.chunks.contains_key(&pos) {
                return true;
            }
            let region_file = match self.get_region_file(world, pos >> 5i8) {
                Ok(file) => file,
                Err(e) => {
                    eprintln!("Failed to get region file: {}", e);
                    return false;
                }
            };
            match region_file.0.read_u32_at::<byteorder::NativeEndian>((((pos.x & 31) | ((pos.y & 31) << 5)) << 2) as u64) {
                Ok(0) => false,
                Ok(_) => true,
                Err(e) => {
                    eprintln!("Error checking existence of chunk: {}", e);
                    false
                }
            }
        })
    }

    fn get_region_file(&self, world: &World, region_pos: IVec2) -> io::Result<dashmap::mapref::one::RefMut<IVec2, (RandomAccessFile, time::SystemTime), ahash::RandomState>> {
        if let Entry::Occupied(entry) = self.region_file_cache.entry(region_pos) {
            return Ok(entry.into_ref());
        }

        let first_accessed_pos = self.region_file_cache.iter().min_by_key(|r| r.value().1).map(|r| *r.key());
        if self.region_file_cache.len() >= 8 {
            if let Some(first_accessed_pos) = first_accessed_pos {
                self.region_file_cache.remove(&first_accessed_pos);
            }
        }
        let region_file_cache_entry = self.region_file_cache.entry(region_pos).or_try_insert_with::<io::Error>(|| {
            let save_dir = self.get_save_dir(world);
            let region_path = save_dir.join("region").join(format!("r.{}.{}.mca", region_pos.x, region_pos.y));
            let raf = RandomAccessFile::open(region_path)?;
            Ok((raf, time::SystemTime::now()))
        })?;
        Ok(region_file_cache_entry)
    }

    fn read_chunk(&self, world: &World, pos: ChunkPos) -> io::Result<Option<Chunk>> {
        let serialized_chunk: SerializedChunk = {
            let region_file_cache_entry = self.get_region_file(world, pos >> 5i8)?;
            let raf = &region_file_cache_entry.0;

            #[allow(clippy::uninit_assumed_init)]
                let mut sector_data: [u8; 4] = unsafe { MaybeUninit::uninit().assume_init() };
            raf.read_exact_at((((pos.x & 31) | ((pos.y & 31) << 5)) << 2) as u64, &mut sector_data)?;
            if sector_data == [0, 0, 0, 0] {
                return Ok(None);
            }
            let offset = Cursor::new(sector_data).read_u24::<BigEndian>()? as u64 * 4096;
            let size = sector_data[3] as usize * 4096;
            if size < 5 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "Chunk header is truncated"));
            }
            let mut buffer = Vec::with_capacity(size);
            #[allow(clippy::uninit_vec)]
                unsafe { buffer.set_len(size); }
            raf.read_exact_at(offset, &mut buffer)?;
            let mut cursor = Cursor::new(&buffer);
            let m = cursor.read_i32::<BigEndian>()?;
            let b = cursor.read_u8()?;
            if m == 0 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "Chunk is allocated, but stream is missing"));
            }
            if b & 128 != 0 {
                if m != 1 {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "Chunk has both internal and external streams"));
                }
                // TODO: read external chunks
                return Err(io::Error::new(io::ErrorKind::InvalidData, "External chunk"));
            }
            if m < 0 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, format!("Declared size {} of chunk is negative", m)));
            }
            let n = (m - 1) as usize;
            if n > size - 5 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, format!("Declared size {} of chunk is larger than actual size {}", n, size)));
            }
            match b {
                1 => nbt::from_gzip_reader(cursor)?,
                2 => nbt::from_zlib_reader(cursor)?,
                3 => nbt::from_reader(cursor)?,
                _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "Unknown compression type")),
            }
        };

        let mut chunk = Chunk::empty();
        for serialized_section in serialized_chunk.sections {
            let block_palette: Vec<_> = serialized_section.block_states.palette.iter().map(|serialized_state| {
                let mut state = BlockState::new(&serialized_state.name);
                for (k, v) in &serialized_state.properties {
                    state.properties.insert(k.clone(), FName::new(v.to_string().parse().unwrap()));
                }
                IBlockState::new(state)
            }).collect();
            let block_data = BlockData::direct_init(block_palette, serialized_section.block_states.data.iter().map(|i| *i as u64).collect());
            let biome_data = BiomeData::direct_init(serialized_section.biomes.palette, serialized_section.biomes.data.iter().map(|i| *i as u64).collect());
            chunk.subchunks.push(Some(Subchunk {
                block_data,
                biome_data,
                baked_geometry: RefCell::new(None),
            }));
        }
        let num_subchunks = ((self.max_y - self.min_y + 1) >> 4) as usize;
        if chunk.subchunks.len() < num_subchunks {
            chunk.subchunks.reserve(num_subchunks);
            while chunk.subchunks.len() < num_subchunks {
                chunk.subchunks.push(None);
            }
        } else {
            chunk.subchunks.truncate(num_subchunks);
        }
        chunk.subchunks.shrink_to_fit();

        Ok(Some(chunk))
    }
}

#[derive(Default)]
pub struct Camera {
    pub pos: glam::DVec3,
    pub yaw: f32,
    pub pitch: f32,
}

impl Camera {
    pub fn move_camera(&mut self, x: f64, y: f64, z: f64, yaw: f32, pitch: f32) {
        self.pos.x += x;
        self.pos.y += y;
        self.pos.z += z;
        self.yaw = (self.yaw + yaw).rem_euclid(360.0);
        self.pitch = (self.pitch + pitch).clamp(-90.0, 90.0);
    }
}

pub struct WorldRef(pub World);
unsafe impl Send for WorldRef {}
unsafe impl Sync for WorldRef {}
impl WorldRef {
    pub fn unwrap(&self) -> &World {
        &self.0
    }
    pub fn unwrap_mut(&mut self) -> &mut World {
        &mut self.0
    }
}
impl Borrow<World> for WorldRef {
    fn borrow(&self) -> &World {
        &self.0
    }
}
impl BorrowMut<World> for WorldRef {
    fn borrow_mut(&mut self) -> &mut World {
        &mut self.0
    }
}

pub struct World {
    pub camera: Camera,
    level_dat: LevelDat,
    path: PathBuf,
    pub resources: resources::Resources,
    pub renderer: WorldRenderer,
    dimensions: FastDashMap<FName, Arc<RwLock<Dimension>>>,
}

impl World {
    pub fn new(path: PathBuf, interaction_handler: &mut dyn minecraft::DownloadInteractionHandler) -> io::Result<World> {
        let level_dat = path.join("level.dat");
        let level_dat: LevelDat = nbt::from_gzip_reader(fs::File::open(level_dat)?)?;
        let mc_version = level_dat.data.version.as_ref().map(|v| &v.name).unwrap_or(&minecraft::ABSENT_MINECRAFT_VERSION.to_string()).clone();
        let resources = match resources::Resources::load(&mc_version, &Vec::new(), interaction_handler) {
            Some(r) => r,
            None => return Err(io::Error::new(io::ErrorKind::Other, "Failed to load resources")),
        };
        let renderer = WorldRenderer::new(&mc_version, &resources);
        let world = World {
            camera: Camera::default(),
            level_dat,
            path,
            resources,
            renderer,
            dimensions: make_fast_dash_map()
        };
        let mut overworld = Dimension::new(CommonFNames.OVERWORLD.clone());
        overworld.min_y = -64;
        overworld.max_y = 384;
        world.dimensions.insert(CommonFNames.OVERWORLD.clone(), Arc::new(RwLock::new(overworld)));
        world.dimensions.insert(CommonFNames.THE_NETHER.clone(), Arc::new(RwLock::new(Dimension::new(CommonFNames.THE_NETHER.clone()))));
        world.dimensions.insert(CommonFNames.THE_END.clone(), Arc::new(RwLock::new(Dimension::new(CommonFNames.THE_END.clone()))));
        Ok(world)
    }

    pub fn get_dimension(&self, id: &FName) -> Option<Arc<RwLock<Dimension>>> {
        self.dimensions.view(id, |_, dim| dim.clone())
    }
}

#[derive(Deserialize, Serialize)]
struct LevelDat {
    #[serde(rename = "Data")]
    data: LevelDatData,

    #[serde(flatten)]
    unknown_fields: BTreeMap<String, nbt::Value>,
}

#[derive(Deserialize, Serialize)]
struct LevelDatData {
    #[serde(rename = "Version")]
    version: Option<LevelDatVersionInfo>,

    #[serde(flatten)]
    unknown_fields: BTreeMap<String, nbt::Value>,
}

#[derive(Deserialize, Serialize)]
struct LevelDatVersionInfo {
    #[serde(rename = "Id")]
    id: u32,
    #[serde(rename = "Name")]
    name: String,

    #[serde(flatten)]
    unknown_fields: BTreeMap<String, nbt::Value>,
}

#[derive(Deserialize, Serialize)]
struct SerializedChunk {
    sections: Vec<SerializedChunkSection>,

    #[serde(flatten)]
    unknown_fields: BTreeMap<String, nbt::Value>,
}

#[derive(Deserialize, Serialize)]
struct SerializedChunkSection {
    block_states: SerializedBlockStates,
    biomes: SerializedBiomes,

    #[serde(flatten)]
    unknown_fields: BTreeMap<String, nbt::Value>,
}

#[derive(Deserialize, Serialize)]
struct SerializedBlockStates {
    palette: Vec<SerializedBlockState>,
    #[serde(default)]
    data: Vec<i64>,

    #[serde(flatten)]
    unknown_fields: BTreeMap<String, nbt::Value>,
}

#[derive(Deserialize, Serialize)]
struct SerializedBlockState {
    #[serde(rename = "Name")]
    name: FName,
    #[serde(default)]
    #[serde(rename = "Properties")]
    properties: AHashMap<FName, nbt::Value>,
}

#[derive(Deserialize, Serialize)]
struct SerializedBiomes {
    palette: Vec<FName>,
    #[serde(default)]
    data: Vec<i64>,

    #[serde(flatten)]
    unknown_fields: BTreeMap<String, nbt::Value>,
}