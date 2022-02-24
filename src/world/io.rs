use std::{io, time};
use std::io::Cursor;
use std::mem::MaybeUninit;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::sync::atomic::AtomicBool;
use ahash::AHashMap;
use byteorder::{BigEndian, ReadBytesExt};
use dashmap::mapref::entry::Entry;
use dashmap::try_result::TryResult;
use flate2::read;
use glam::IVec2;
use positioned_io_preview::{RandomAccessFile, ReadAt, ReadBytesAtExt};
use serde::{Deserialize, Deserializer};
use crate::{CommonFNames, convert, World};
use crate::convert::{ConvertInto, VersionedSerde};
use crate::fname::FName;
use crate::geom::ChunkPos;
use crate::util::FastDashRefMut;
use crate::world::{BlockState, Chunk, Dimension, IBlockState, Subchunk, versioned_io};
use crate::world::palette::{BiomeData, BlockData};
use crate::world::versioned_io::*;

impl Dimension {
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

    #[profiling::function]
    pub fn get_chunk(&self, pos: ChunkPos) -> Option<Arc<Chunk>> {
        self.chunks.get(&pos).map(|chunk| chunk.clone())
    }

    #[profiling::function]
    pub fn load_chunk(&self, world: &World, pos: ChunkPos) -> Option<Arc<Chunk>> {
        if self.chunk_existence_cache.get(&pos).map(|b| *b) == Some(false) {
            return None;
        }
        if let Some(chunk) = self.chunks.get(&pos) {
            return Some(chunk.clone());
        }
        let mut chunk_loaded = false;
        self.chunks.entry(pos).or_try_insert_with(|| {
            match self.read_chunk(world, pos)? {
                Some(chunk) => {
                    chunk_loaded = true;
                    Ok(Arc::new(chunk))
                },
                None => Err(io::Error::new(io::ErrorKind::NotFound, "Chunk not found"))
            }
        }).map(|r| {
            let result = r.clone();
            drop(r);
            if chunk_loaded {
                self.on_chunk_load(pos);
            }
            result
        }).map_err(|err| {
            if err.kind() != io::ErrorKind::NotFound {
                eprintln!("Failed to load chunk: {}", err);
            }
        }).ok()
    }

    #[profiling::function]
    pub fn unload_chunk(&self, _world: &World, pos: ChunkPos) -> bool {
        self.chunks.remove(&pos).is_some()
    }

    #[profiling::function]
    pub fn try_does_chunk_exist(&self, world: &World, pos: ChunkPos) -> Option<bool> {
        self.does_chunk_exist_internal(world, pos, false)
    }

    #[profiling::function]
    pub fn does_chunk_exist(&self, world: &World, pos: ChunkPos) -> bool {
        self.does_chunk_exist_internal(world, pos, true).unwrap()
    }

    fn does_chunk_exist_internal(&self, world: &World, pos: ChunkPos, now: bool) -> Option<bool> {
        if now {
            if let Some(exists) = self.chunk_existence_cache.get(&pos) {
                return Some(*exists);
            }
        } else {
            match self.chunk_existence_cache.try_get(&pos) {
                TryResult::Present(exists) => return Some(*exists),
                TryResult::Absent => {},
                TryResult::Locked => return None,
            };
        }

        self.chunk_existence_cache.entry(pos).or_try_insert_with::<()>(|| {
            if self.chunks.contains_key(&pos) {
                return Ok(true);
            }
            let region_file = match self.get_region_file(world, pos >> 5i8, now) {
                Ok(file) => file.ok_or(())?,
                Err(e) => {
                    if e.kind() != io::ErrorKind::NotFound {
                        eprintln!("Failed to get region file: {}", e);
                    }
                    return Ok(false);
                }
            };
            match region_file.0.read_u32_at::<byteorder::NativeEndian>((((pos.x & 31) | ((pos.y & 31) << 5)) << 2) as u64) {
                Ok(0) => Ok(false),
                Ok(_) => Ok(true),
                Err(e) => {
                    eprintln!("Error checking existence of chunk: {}", e);
                    Ok(false)
                }
            }
        }).ok().map(|b| *b)
    }

    #[profiling::function]
    fn get_region_file(&self, world: &World, region_pos: IVec2, now: bool) -> io::Result<Option<FastDashRefMut<IVec2, (RandomAccessFile, time::SystemTime)>>> {
        if now {
            if let Entry::Occupied(entry) = self.region_file_cache.entry(region_pos) {
                return Ok(Some(entry.into_ref()));
            }
        } else {
            match self.region_file_cache.try_entry(region_pos) {
                Some(Entry::Occupied(entry)) => return Ok(Some(entry.into_ref())),
                Some(_) => {},
                None => return Ok(None),
            };
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
        Ok(Some(region_file_cache_entry))
    }

    #[profiling::function]
    fn read_chunk(&self, world: &World, pos: ChunkPos) -> io::Result<Option<Chunk>> {
        let serialized_chunk: SerializedChunk = {
            let region_file_cache_entry = match self.get_region_file(world, pos >> 5i8, true) {
                Ok(entry) => entry.unwrap(),
                Err(e) => {
                    return if e.kind() == io::ErrorKind::NotFound {
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
            let pos = cursor.position() as usize;
            let make_deserializer = || {
                let cursor = Cursor::new(&buffer[pos..]);
                let read: Box<dyn io::Read> = match b {
                    1 => Box::new(read::GzDecoder::new(cursor)),
                    2 => Box::new(read::ZlibDecoder::new(cursor)),
                    3 => Box::new(cursor),
                    _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "Unknown compression type")),
                };
                Ok(nbt::de::Decoder::new(read))
            };
            let version = convert::get_version(&mut make_deserializer()?)?;
            versioned_io::CURRENT_DIMENSION.with(|cur_dim| {
                cur_dim.replace(self.id.clone());
            });
            let result = VersionedSerde::deserialize(version, world.level_dat.data.version.id, &mut make_deserializer()?)?;
            result
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

pub fn get_level_dat_version<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    #[derive(Deserialize)]
    struct LevelDatVersionExtractorData {
        #[serde(rename = "DataVersion")]
        data_version: u32,
    }
    #[derive(Deserialize)]
    struct LevelDatVersionExtractor {
        #[serde(rename = "Data")]
        data: LevelDatVersionExtractorData,
    }
    let extractor = LevelDatVersionExtractor::deserialize(deserializer)?;
    Ok(extractor.data.data_version)
}

convert::variants! {
    pub(super) struct LevelDat {
        #[serde(rename = "Data")]
        #[variants]
        pub(super) data: LevelDatData,
    }
}

convert::variants! {
    pub(super) struct LevelDatData {
        #[serde(rename = "Version")]
        #[variants]
        pub(super) version: LevelDatVersionInfo,
    }
}

convert::variants! {
    pub(super) struct LevelDatVersionInfo {
        #[serde(rename = "Id")]
        id: u32,
        #[serde(rename = "Name")]
        pub(super) name: String,
    }
}

convert::variants! {
    struct SerializedChunk {
        #[variants]
        sections: Vec<SerializedChunkSection>,
    }
    #[variants(SerializedChunkSection, SerializedBlockStates, SerializedBiomes)]
    fn up(older: Self::UpInput, prevailing_version: u32) -> Self::UpResult {
        let biomes = biomes_17_up(&older.level.biomes, prevailing_version)?;
        let sections = fix_17_sections(older.level.sections, prevailing_version)?;
        let sections = sections.into_iter().zip(biomes).map(|(sec, biomes)| {
            let result: convert::Result<SerializedChunkSection> = try {
                SerializedChunkSection {
                    block_states: SerializedBlockStates {
                        palette: sec.palette.map(|p| p.convert_into(prevailing_version)).transpose()?.unwrap_or_else(|| {
                            vec![Variant_SerializedBlockState_1_18 {
                                name: CommonFNames.AIR.clone(),
                                properties: Default::default(),
                                _extra: Default::default(),
                            }]
                        }),
                        data: sec.block_states,
                        _extra: Default::default(),
                    },
                    biomes,
                    _extra: sec._extra,
                }
            };
            result
        }).collect::<Result<_, _>>()?;
        Ok(
            Self::UpOutput {
                sections,
                _extra: older._extra,
            }
        )
    }
    #[variants(SerializedChunkLevel, SerializedChunkSection17)]
    fn down(newer: Self::DownInput, prevailing_version: u32) -> Self::DownResult {
        let biomes = biomes_17_down(&newer.sections, prevailing_version)?;
        let sections = newer.sections.into_iter().map(|sec| {
            let result: convert::Result<SerializedChunkSection17> = try {
                SerializedChunkSection17 {
                    palette: Some(sec.block_states.palette.convert_into(prevailing_version)?),
                    block_states: sec.block_states.data,
                    y: 0,
                    _extra: sec._extra,
                }
            };
            result
        }).collect::<Result<_, _>>()?;
        Ok(
            Self::DownOutput {
                level: SerializedChunkLevel {
                    sections,
                    biomes,
                    _extra: Default::default(),
                },
                _extra: newer._extra,
            }
        )
    }
    1,17,1 => {
        #[serde(rename = "Level")]
        #[variants]
        level: SerializedChunkLevel,
    }
}

convert::variants! {
    pub(super) struct SerializedChunkSection {
        #[variants]
        block_states: SerializedBlockStates,
        #[variants]
        pub(super) biomes: SerializedBiomes,
    }
}

convert::variants! {
    pub(super) struct SerializedBlockStates {
        #[variants]
        palette: Vec<SerializedBlockState>,
        #[serde(default)]
        data: Vec<i64>,
    }
}

convert::variants! {
    pub(super) struct SerializedBlockState {
        #[registry(block)]
        #[serde(rename = "Name")]
        name: FName,
        #[serde(default)]
        #[serde(rename = "Properties")]
        properties: AHashMap<FName, nbt::Value>,
    }
}

convert::variants! {
    #[derive(Clone)]
    pub(super) struct SerializedBiomes {
        #[registry(biome)]
        pub(super) palette: Vec<FName>,
        #[serde(default)]
        pub(super) data: Vec<i64>,
    }
}