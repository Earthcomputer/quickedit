use std::{fmt, fs, io, mem, time};
use std::collections::btree_map::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::collections::vec_deque::VecDeque;
use std::fmt::Formatter;
use std::hash::{Hash, Hasher};
use std::io::{Cursor, ErrorKind};
use std::mem::MaybeUninit;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::sync::atomic::{AtomicBool, Ordering};
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
use crate::geom::{BlockPos, ChunkPos, IVec2RangeExtensions};
use crate::resources;
use crate::util::{FastDashMap, MainThreadStore, make_fast_dash_map};
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
define_paletted_data!(BiomeData, FName, 2_usize, 2_usize, 2_usize);

pub struct Subchunk {
    block_data: RwLock<BlockData>,
    biome_data: RwLock<BiomeData>,

    pub needs_redraw: AtomicBool,
    pub baked_geometry: Mutex<Option<MainThreadStore<renderer::BakedChunkGeometry>>>,
}

impl Subchunk {
    pub fn get_block_state(&self, pos: BlockPos) -> IBlockState {
        self.block_data.read().unwrap().get(pos.x as usize, pos.y as usize, pos.z as usize).clone()
    }

    pub fn set_block_state(&self, pos: BlockPos, value: &IBlockState) {
        self.block_data.write().unwrap().set(pos.x as usize, pos.y as usize, pos.z as usize, value);
        self.needs_redraw.store(true, Ordering::Release);
    }

    pub fn get_biome(&self, pos: BlockPos) -> FName {
        self.biome_data.read().unwrap().get(pos.x as usize >> 2, pos.y as usize >> 2, pos.z as usize >> 2).clone()
    }

    pub fn set_biome(&self, pos: BlockPos, value: &FName) {
        self.biome_data.write().unwrap().set(pos.x as usize >> 2, pos.y as usize >> 2, pos.z as usize >> 2, value);
        self.needs_redraw.store(true, Ordering::Release);
    }
}

unsafe impl Send for Subchunk {}
unsafe impl Sync for Subchunk {}

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
        Some(subchunk.get_block_state(pos & glam::IVec3::new(!0, 15, !0)))
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
        Some(subchunk.get_biome(pos & glam::IVec3::new(!0, 15, !0)))
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
            max_y: 255,
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

    pub fn unload_chunk(&self, _world: &World, pos: ChunkPos) -> bool {
        self.chunks.remove(&pos).is_some()
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
                    if e.kind() != ErrorKind::NotFound {
                        eprintln!("Failed to get region file: {}", e);
                    }
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
            let region_file_cache_entry = match self.get_region_file(world, pos >> 5i8) {
                Ok(entry) => entry,
                Err(e) => {
                    return if e.kind() == ErrorKind::NotFound {
                        Ok(None)
                    } else {
                        Err(e)
                    }
                }
            };
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
                block_data: RwLock::new(block_data),
                biome_data: RwLock::new(biome_data),
                needs_redraw: AtomicBool::new(true),
                baked_geometry: Mutex::new(None),
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

pub struct Camera {
    pub dimension: FName,
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

fn chunk_loader(world: &World, stop: &dyn Fn() -> bool) {
    let render_distance = 16;
    let mut prev_dimension: Option<FName> = None;
    let mut prev_chunk_pos = None;
    let mut chunks_to_unload = VecDeque::new();

    'outer_loop:
    while !stop() {
        // find chunk to load
        let (dimension, pos) = {
            let camera = world.camera.read().unwrap();
            (camera.dimension.clone(), camera.pos)
        };
        if prev_dimension.as_ref() != Some(&dimension) {
            if let Some(prev_dimension) = prev_dimension {
                for chunk_pos in ((prev_chunk_pos.unwrap() - ChunkPos::new(render_distance, render_distance))..=(prev_chunk_pos.unwrap() + ChunkPos::new(render_distance, render_distance))).iter() {
                    chunks_to_unload.push_back((prev_dimension.clone(), chunk_pos));
                }
            }
            prev_dimension = Some(dimension.clone());
        }

        let chunk_pos = pos.xz().floor().as_ivec2() >> 4;

        if prev_chunk_pos != Some(chunk_pos) {
            if let Some(prev_chunk_pos) = prev_chunk_pos {
                for cp in ((prev_chunk_pos - ChunkPos::new(render_distance, render_distance))..=(prev_chunk_pos + ChunkPos::new(render_distance, render_distance))).iter() {
                    let delta: IVec2 = cp - chunk_pos;
                    let distance = delta.abs().max_element();
                    if distance > render_distance {
                        chunks_to_unload.push_back((dimension.clone(), cp));
                    }
                }
            }
            prev_chunk_pos = Some(chunk_pos);
        }

        while !chunks_to_unload.is_empty() {
            let (dimension, chunk_pos) = chunks_to_unload.pop_front().unwrap();
            if let Some(dimension) = world.get_dimension(&dimension) {
                if dimension.unload_chunk(world, chunk_pos) {
                    break;
                }
            }
        }

        let dimension = match world.get_dimension(&dimension) {
            Some(dimension) => dimension,
            None => {
                World::worker_yield();
                continue
            },
        };

        for chunk_pos in ((chunk_pos - ChunkPos::new(render_distance, render_distance))..=(chunk_pos + ChunkPos::new(render_distance, render_distance))).iter() {
            if dimension.get_chunk(chunk_pos).is_none()
                && dimension.does_chunk_exist(world, chunk_pos)
                && dimension.load_chunk(world, chunk_pos).is_some()
            {
                continue 'outer_loop;
            }
        }

        World::worker_yield();
    }
}

lazy_static! {
    static ref GLOBAL_TICK_VAR: Condvar = Condvar::new();
    static ref GLOBAL_TICK_MUTEX: Mutex<usize> = Mutex::new(0);
}

pub struct World {
    pub camera: RwLock<Camera>,
    level_dat: LevelDat,
    path: PathBuf,
    pub resources: Arc<resources::Resources>,
    pub renderer: MainThreadStore<WorldRenderer>,
    dimensions: FastDashMap<FName, Arc<Dimension>>,
}

impl World {
    pub fn load(path: PathBuf, interaction_handler: &mut dyn minecraft::DownloadInteractionHandler) -> io::Result<WorldRef> {
        let level_dat = path.join("level.dat");
        let level_dat: LevelDat = nbt::from_gzip_reader(fs::File::open(level_dat)?)?;
        let mc_version = level_dat.data.version.as_ref().map(|v| &v.name).unwrap_or(&minecraft::ABSENT_MINECRAFT_VERSION.to_string()).clone();
        let resources = match resources::Resources::load(&mc_version, &Vec::new(), interaction_handler) {
            Some(r) => Arc::new(r),
            None => return Err(io::Error::new(io::ErrorKind::Other, "Failed to load resources")),
        };
        let renderer = {
            let mc_version = mc_version.clone();
            let resources = resources.clone();
            MainThreadStore::create(move || WorldRenderer::new(&mc_version, &*resources))
        };
        let world = World {
            camera: RwLock::new(Camera {
                dimension: CommonFNames.OVERWORLD.clone(),
                pos: glam::DVec3::ZERO,
                yaw: 0.0,
                pitch: 0.0,
            }),
            level_dat,
            path,
            resources,
            renderer,
            dimensions: make_fast_dash_map()
        };
        let mut overworld = Dimension::new(CommonFNames.OVERWORLD.clone());
        overworld.min_y = -64;
        overworld.max_y = 383;
        world.dimensions.insert(CommonFNames.OVERWORLD.clone(), Arc::new(overworld));
        world.dimensions.insert(CommonFNames.THE_NETHER.clone(), Arc::new(Dimension::new(CommonFNames.THE_NETHER.clone())));
        world.dimensions.insert(CommonFNames.THE_END.clone(), Arc::new(Dimension::new(CommonFNames.THE_END.clone())));
        let world = WorldRef::new(world);
        unsafe {
            world.spawn_worker(chunk_loader);
        }
        WorldRenderer::start_build_worker(&world);
        Ok(world)
    }

    pub fn get_dimension(&self, id: &FName) -> Option<Arc<Dimension>> {
        self.dimensions.view(id, |_, dim| dim.clone())
    }

    pub fn tick() {
        {
            let mut global_tick_mutex = GLOBAL_TICK_MUTEX.lock().unwrap();
            *global_tick_mutex = global_tick_mutex.wrapping_add(1);
        }
        GLOBAL_TICK_VAR.notify_all();
    }

    pub fn worker_yield() {
        drop(GLOBAL_TICK_VAR.wait(GLOBAL_TICK_MUTEX.lock().unwrap()).unwrap());
    }
}

pub struct WorldRef {
    thread_pool: rayon::ThreadPool,
    world: Arc<World>,
    dropping: Arc<AtomicBool>,
}
unsafe impl Send for WorldRef {}
unsafe impl Sync for WorldRef {}
impl WorldRef {
    fn new(world: World) -> Self {
        Self {
            thread_pool: rayon::ThreadPoolBuilder::new().num_threads((num_cpus::get() - 1).max(4)).build().unwrap(),
            world: Arc::new(world),
            dropping: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Please kindly only access the world from your worker, and not abuse your 'static lifetime :)
    pub unsafe fn spawn_worker<F>(&self, job: F)
        where
            F: FnOnce(&World, &dyn Fn() -> bool) + Send + 'static,
    {
        let world = self.world.clone();
        let dropping = self.dropping.clone();
        self.thread_pool.spawn(move || {
            if !dropping.load(Ordering::Relaxed) {
                job(&*world, &(|| dropping.load(Ordering::Relaxed)));
            }
        });
    }
}
impl Deref for WorldRef {
    type Target = World;
    fn deref(&self) -> &World {
        &*self.world
    }
}
impl Drop for WorldRef {
    fn drop(&mut self) {
        self.dropping.store(true, Ordering::Relaxed);
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