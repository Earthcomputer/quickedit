use std::collections::hash_map::DefaultHasher;
use std::{fmt, io, time};
use std::fmt::Formatter;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use ahash::AHashMap;
use flate2::read;
use glam::{IVec2, Vec3Swizzles};
use internment::ArcIntern;
use positioned_io_preview::RandomAccessFile;
use crate::fname::FName;
use crate::geom::{BlockPos, ChunkPos, IVec2RangeExtensions};
use crate::renderer;
use crate::renderer::WorldRenderer;
use crate::{CommonFNames, minecraft, resources};
use crate::convert::VersionedSerde;
use crate::util::{FastDashMap, make_fast_dash_map};
use crate::world::io::{get_level_dat_version, LevelDat};
use crate::world::palette::{BiomeData, BlockData};
use crate::world::workers;
use crate::world::workers::WorldRef;

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

pub struct Subchunk {
    pub(super) block_data: RwLock<BlockData>,
    pub(super) biome_data: RwLock<BiomeData>,
    pub needs_redraw: AtomicBool,
}

impl Subchunk {
    #[profiling::function]
    pub fn get_block_state(&self, pos: BlockPos) -> IBlockState {
        self.block_data.read().unwrap().get(pos.x as usize, pos.y as usize, pos.z as usize).clone()
    }

    #[profiling::function]
    fn set_block_state(&self, pos: BlockPos, value: &IBlockState) {
        self.block_data.write().unwrap().set(pos.x as usize, pos.y as usize, pos.z as usize, value);
        self.needs_redraw.store(true, Ordering::Release);
    }

    #[profiling::function]
    pub fn get_biome(&self, pos: BlockPos) -> FName {
        self.biome_data.read().unwrap().get(pos.x as usize >> 2, pos.y as usize >> 2, pos.z as usize >> 2).clone()
    }

    #[profiling::function]
    fn set_biome(&self, pos: BlockPos, value: &FName) {
        self.biome_data.write().unwrap().set(pos.x as usize >> 2, pos.y as usize >> 2, pos.z as usize >> 2, value);
        self.needs_redraw.store(true, Ordering::Release);
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

    #[profiling::function]
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

    #[profiling::function]
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
    pub(super) id: FName,
    pub min_y: i32,
    pub max_y: i32,
    pub(super) chunks: FastDashMap<ChunkPos, Arc<Chunk>>,

    pub(super) region_file_cache: FastDashMap<IVec2, (RandomAccessFile, time::SystemTime)>,
    pub(super) chunk_existence_cache: FastDashMap<IVec2, bool>,
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

    #[profiling::function]
    pub fn get_block_state(&self, pos: BlockPos) -> Option<IBlockState> {
        let chunk = self.get_chunk(pos.xz() >> glam::IVec2::new(4, 4))?;
        chunk.get_block_state(self, pos & glam::IVec3::new(15, !0, 15))
    }

    #[profiling::function]
    pub fn get_biome(&self, pos: BlockPos) -> Option<FName> {
        let chunk = self.get_chunk(pos.xz() >> glam::IVec2::new(4, 4))?;
        chunk.get_biome(self, pos & glam::IVec3::new(15, !0, 15))
    }

    pub(super) fn on_chunk_load(&self, pos: ChunkPos) {
        for delta in (-IVec2::ONE..=IVec2::ONE).iter() {
            if let Some(chunk) = self.get_chunk(pos + delta) {
                for subchunk in chunk.subchunks.iter().flatten() {
                    subchunk.needs_redraw.store(true, Ordering::Release);
                }
            }
        }
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

pub struct World {
    pub camera: RwLock<Camera>,
    level_dat: LevelDat,
    pub(super) path: PathBuf,
    pub resources: Arc<resources::Resources>,
    pub renderer: WorldRenderer,
    dimensions: FastDashMap<FName, Arc<Dimension>>,
}

impl World {
    #[profiling::function]
    pub fn load(path: PathBuf, interaction_handler: &mut dyn minecraft::DownloadInteractionHandler) -> io::Result<WorldRef> {
        let level_dat = path.join("level.dat");
        let level_dat_version = get_level_dat_version(&mut nbt::de::Decoder::new(read::GzDecoder::new(File::open(&level_dat)?)))?;
        let level_dat: LevelDat = VersionedSerde::deserialize(level_dat_version, &mut nbt::de::Decoder::new(read::GzDecoder::new(File::open(&level_dat)?)))?;
        let mc_version = level_dat.data.version.as_ref().map(|v| &v.name).unwrap_or(&minecraft::ABSENT_MINECRAFT_VERSION.to_string()).clone();
        let resources = match resources::loader::load(&mc_version, &Vec::new(), interaction_handler) {
            Some(r) => Arc::new(r),
            None => return Err(io::Error::new(io::ErrorKind::Other, "Failed to load resources")),
        };
        let renderer = WorldRenderer::new(&mc_version, resources.clone());
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
        world.spawn_worker(workers::chunk_loader);
        world.spawn_worker(renderer::worker::chunk_render_worker);
        Ok(world)
    }

    #[profiling::function]
    pub fn get_dimension(&self, id: &FName) -> Option<Arc<Dimension>> {
        self.dimensions.get(id).map(|d| d.clone())
    }
}