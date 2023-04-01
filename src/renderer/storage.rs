use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use glium::Surface;
use lazy_static::lazy_static;
use crate::{profile_mutex, renderer, util};
use crate::debug::{ProfileMutex, ProfileMutexGuard};
use crate::geom::{ChunkPos, IVec2Extensions, IVec2RangeExtensions};
use crate::renderer::Transparency;
use crate::util::MainThreadStore;

pub(super) type Quad<T> = [T; 4];

#[derive(Default)]
pub(super) struct Geometry {
    pub(super) quads: Vec<Quad<util::Vertex>>,
}

impl Geometry {
    pub(super) fn join_vertices<'a>(geoms: impl Iterator<Item=&'a Geometry>) -> Vec<util::Vertex> {
        geoms.flat_map(|g| &g.quads).cloned().flatten().collect()
    }
}

pub(super) struct SubchunkGeometry {
    pub(super) opaque_geometry: Geometry,
    pub(super) transparent_geometry: Geometry,
    pub(super) translucent_geometry: Geometry,
    pub(super) dirty: bool,
    pub(super) mark_for_upload: bool,
}

impl SubchunkGeometry {
    fn new() -> Self {
        Self {
            opaque_geometry: Geometry::default(),
            transparent_geometry: Geometry::default(),
            translucent_geometry: Geometry::default(),
            dirty: true,
            mark_for_upload: false,
        }
    }

    pub(super) fn clear(&mut self) {
        self.opaque_geometry.quads.clear();
        self.transparent_geometry.quads.clear();
        self.translucent_geometry.quads.clear();
    }

    pub(super) fn get_geometry(&mut self, transparency: Transparency) -> &mut Geometry {
        match transparency {
            Transparency::Opaque => &mut self.opaque_geometry,
            Transparency::Transparent => &mut self.transparent_geometry,
            Transparency::Translucent => &mut self.translucent_geometry,
        }
    }
}

lazy_static! {
    static ref SHARED_INDEX_BUFFER: MainThreadStore<RefCell<glium::index::IndexBuffer<u32>>> = MainThreadStore::create(||
        RefCell::new(glium::IndexBuffer::empty(renderer::get_display(), glium::index::PrimitiveType::TrianglesList, 0).unwrap()));
}

#[derive(Default)]
pub(super) struct BakedGeometry {
    vertices: Option<glium::VertexBuffer<util::Vertex>>,
    len: usize,
}

impl BakedGeometry {
        fn expand_index_buffer(new_size: usize) {
        let index_buffer: Vec<_> = (0..new_size)
            .flat_map(|i| {
                let i = i * 4;
                [i, i + 1, i + 2, i + 2, i + 3, i]
            })
            .map(|i| i as u32)
            .collect();
        SHARED_INDEX_BUFFER.swap(&RefCell::new(glium::IndexBuffer::new(renderer::get_display(), glium::index::PrimitiveType::TrianglesList, &index_buffer).unwrap()));
    }

        pub(super) fn set_buffer_data(&mut self, vertices: &[util::Vertex]) {
        if self.vertices.as_ref().map(|verts| verts.len()).unwrap_or(0) < vertices.len() {
            self.vertices = Some(glium::VertexBuffer::new(renderer::get_display(), vertices).unwrap());
            if (**SHARED_INDEX_BUFFER).borrow().len() * 2 < vertices.len() * 3 {
                BakedGeometry::expand_index_buffer(vertices.len() * 3 / 2);
            }
        } else if !vertices.is_empty() {
            self.vertices.as_ref().unwrap().slice(0..vertices.len()).unwrap().write(vertices);
        }
        self.len = vertices.len();
    }

        pub(super) fn draw<U>(&self, target: &mut glium::Frame, program: &glium::Program, uniforms: &U, params: &glium::DrawParameters)
        where U: glium::uniforms::Uniforms
    {
        if let Some(vertices) = &self.vertices {
            let vertices = vertices.slice(0..self.len).unwrap();
            let index_buffer = (**SHARED_INDEX_BUFFER).borrow();
            let indices = index_buffer.slice(0..self.len * 3 / 2).unwrap();
            target.draw(vertices, indices, program, uniforms, params).unwrap();
        }
    }
}

#[derive(Default)]
pub(super) struct BakedChunkGeometry {
    pub(super) opaque_geometry: BakedGeometry,
    pub(super) transparent_geometry: BakedGeometry,
    pub(super) translucent_geometry: BakedGeometry,
}

pub(super) struct BuiltChunk {
    pub(super) subchunk_geometry: Mutex<Vec<SubchunkGeometry>>,
    pub(super) baked_geometry: MainThreadStore<RefCell<BakedChunkGeometry>>,
    pub(super) ready: AtomicBool,
}

impl BuiltChunk {
        fn new(subchunk_count: u32) -> Self {
        let mut subchunk_geometry = Vec::with_capacity(subchunk_count as usize);
        for _ in 0..subchunk_count {
            subchunk_geometry.push(SubchunkGeometry::new());
        }
        Self {
            subchunk_geometry: Mutex::new(subchunk_geometry),
            baked_geometry: MainThreadStore::default(),
            ready: AtomicBool::new(false),
        }
    }
}

pub(super) struct ChunkStore {
    render_distance: u32,
    pub(super) camera_pos: ProfileMutex<Option<ChunkPos>>,
    chunks: Vec<Mutex<BuiltChunk>>,
}

impl ChunkStore {
        pub(super) fn new(render_distance: u32, subchunk_count: u32) -> Self {
        let width = render_distance * 2 + 1;
        let mut chunks = Vec::with_capacity((width * width) as usize);
        for _ in 0..(width * width) {
            chunks.push(Mutex::new(BuiltChunk::new(subchunk_count)));
        }
        Self {
            render_distance,
            camera_pos: profile_mutex!("camera_pos", None),
            chunks,
        }
    }

    fn get_index(&self, chunk_pos: ChunkPos) -> usize {
        let width = (self.render_distance * 2 + 1) as i32;
        let x = chunk_pos.x.rem_euclid(width) as usize;
        let z = chunk_pos.y.rem_euclid(width) as usize;
        z * width as usize + x
    }

        pub(super) fn get(&self, chunk_pos: ChunkPos) -> impl Deref<Target=BuiltChunk> + DerefMut<Target=BuiltChunk> + '_ {
        self.chunks[self.get_index(chunk_pos)].lock().unwrap()
    }

        pub(super) fn set_camera_pos(&self, camera_pos: ChunkPos) -> ProfileMutexGuard<Option<ChunkPos>> {
        let mut current_camera_pos = self.camera_pos.lock().unwrap();
        if *current_camera_pos == Some(camera_pos) {
            return current_camera_pos;
        }

        if let Some(cur_camera_pos) = *current_camera_pos {
            for chunk_pos in cur_camera_pos.square_range(self.render_distance as i32).iter() {
                let distance = chunk_pos.rectangular_distance(camera_pos) as u32;
                if distance > self.render_distance {
                    let built_chunk = self.get(chunk_pos);
                    built_chunk.ready.store(false, Ordering::Relaxed);
                    for subchunk in &mut *built_chunk.subchunk_geometry.lock().unwrap() {
                        subchunk.dirty = true;
                    }
                }
            }
        }

        *current_camera_pos = Some(camera_pos);
        current_camera_pos
    }
}
