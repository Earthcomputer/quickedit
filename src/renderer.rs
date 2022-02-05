use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use ahash::AHashMap;
use approx::AbsDiffEq;
use glam::{Affine2, Affine3A, IVec2, IVec3, IVec4, Mat4, Quat, Vec2, Vec3, Vec3Swizzles, Vec4, DVec3};
use glium::{Frame, Surface, uniform};
use lazy_static::lazy_static;
use num_traits::FloatConst;
use crate::{blocks, CommonFNames, geom, make_a_hash_map, profile_mutex, util};
use crate::debug::{ProfileMutex, ProfileMutexGuard};
use crate::geom::{BlockPos, ChunkPos, IVec3RangeExtensions, IVec2RangeExtensions, IVec2Extensions};
use crate::resources::{Resources, TextureAtlas};
use crate::util::{BlitVertex, FastDashMap, Lerp, MainThreadStore, make_fast_dash_map};
use crate::world::{Dimension, IBlockState, Subchunk, World, WorldRef};
use crate::fname::FName;

const MAIN_VERT_SHADER: &str = include_str!("../res/main.vsh");
const MAIN_FRAG_SHADER: &str = include_str!("../res/main.fsh");
const BLIT_VERT_SHADER: &str = include_str!("../res/blit.vsh");
const BLIT_FRAG_SHADER: &str = include_str!("../res/blit.fsh");

struct DisplayHolder {
    display: *const glium::Display,
    #[cfg(debug_assertions)]
    thread: std::thread::ThreadId,
}

static mut DISPLAY: Option<DisplayHolder> = None;

pub unsafe fn set_display(display: &glium::Display) {
    DISPLAY = Some(DisplayHolder {
        display,
        #[cfg(debug_assertions)]
        thread: std::thread::current().id(),
    });
}

pub unsafe fn clear_display() {
    DISPLAY = None;
}

pub fn get_display() -> &'static glium::Display {
    unsafe {
        let holder = DISPLAY.as_ref().unwrap();
        #[cfg(debug_assertions)]
        assert_eq!(holder.thread, std::thread::current().id());
        &*holder.display
    }
}

type Quad<T> = [T; 4];

#[derive(Default)]
struct Geometry {
    quads: Vec<Quad<util::Vertex>>,
}

impl Geometry {
    fn join_vertices<'a>(geoms: impl Iterator<Item=&'a Geometry>) -> Vec<util::Vertex> {
        geoms.flat_map(|g| &g.quads).cloned().flatten().collect()
    }
}

struct SubchunkGeometry {
    opaque_geometry: Geometry,
    transparent_geometry: Geometry,
    translucent_geometry: Geometry,
    dirty: bool,
    mark_for_upload: bool,
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

    fn clear(&mut self) {
        self.opaque_geometry.quads.clear();
        self.transparent_geometry.quads.clear();
        self.translucent_geometry.quads.clear();
    }
}

lazy_static! {
    static ref SHARED_INDEX_BUFFER: MainThreadStore<RefCell<glium::index::IndexBuffer<u32>>> = MainThreadStore::create(||
        RefCell::new(glium::IndexBuffer::empty(get_display(), glium::index::PrimitiveType::TrianglesList, 0).unwrap()));
}

#[derive(Default)]
struct BakedGeometry {
    vertices: Option<glium::VertexBuffer<util::Vertex>>,
    len: usize,
}

impl BakedGeometry {
    #[profiling::function]
    fn expand_index_buffer(new_size: usize) {
        let index_buffer: Vec<_> = (0..new_size)
            .flat_map(|i| {
                let i = i * 4;
                [i, i + 1, i + 2, i + 2, i + 3, i]
            })
            .map(|i| i as u32)
            .collect();
        *SHARED_INDEX_BUFFER.borrow_mut() = glium::IndexBuffer::new(get_display(), glium::index::PrimitiveType::TrianglesList, &index_buffer).unwrap();
    }

    #[profiling::function]
    fn set_buffer_data(&mut self, vertices: &[util::Vertex]) {
        if self.vertices.as_ref().map(|verts| verts.len()).unwrap_or(0) < vertices.len() {
            self.vertices = Some(glium::VertexBuffer::new(get_display(), vertices).unwrap());
            if SHARED_INDEX_BUFFER.borrow().len() * 2 < vertices.len() * 3 {
                BakedGeometry::expand_index_buffer(vertices.len() * 3 / 2);
            }
        } else if !vertices.is_empty() {
            self.vertices.as_ref().unwrap().slice(0..vertices.len()).unwrap().write(vertices);
        }
        self.len = vertices.len();
    }

    #[profiling::function]
    fn draw<U>(&self, target: &mut glium::Frame, program: &glium::Program, uniforms: &U, params: &glium::DrawParameters)
        where U: glium::uniforms::Uniforms
    {
        if let Some(vertices) = &self.vertices {
            let vertices = vertices.slice(0..self.len).unwrap();
            let index_buffer = SHARED_INDEX_BUFFER.borrow();
            let indices = index_buffer.slice(0..self.len * 3 / 2).unwrap();
            target.draw(vertices, indices, program, uniforms, params).unwrap();
        }
    }
}

#[derive(Default)]
struct BakedChunkGeometry {
    opaque_geometry: BakedGeometry,
    transparent_geometry: BakedGeometry,
    translucent_geometry: BakedGeometry,
}

struct BuiltChunk {
    subchunk_geometry: Mutex<Vec<SubchunkGeometry>>,
    baked_geometry: MainThreadStore<RefCell<BakedChunkGeometry>>,
    ready: AtomicBool,
}

impl BuiltChunk {
    #[profiling::function]
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

struct ChunkStore {
    render_distance: u32,
    camera_pos: ProfileMutex<Option<ChunkPos>>,
    chunks: Vec<Mutex<BuiltChunk>>,
}

impl ChunkStore {
    #[profiling::function]
    fn new(render_distance: u32, subchunk_count: u32) -> Self {
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

    #[profiling::function]
    fn get(&self, chunk_pos: ChunkPos) -> impl Deref<Target=BuiltChunk> + DerefMut<Target=BuiltChunk> + '_ {
        self.chunks[self.get_index(chunk_pos)].lock().unwrap()
    }

    #[profiling::function]
    fn set_camera_pos(&self, camera_pos: ChunkPos) -> ProfileMutexGuard<Option<ChunkPos>> {
        let mut current_camera_pos = self.camera_pos.lock().unwrap();
        if *current_camera_pos == Some(camera_pos) {
            return current_camera_pos;
        }

        for chunk_pos in camera_pos.square_range(self.render_distance as i32).iter() {
            let distance = chunk_pos.rectangular_distance(camera_pos) as u32;
            if distance > self.render_distance {
                let built_chunk = self.get(chunk_pos);
                built_chunk.ready.store(false, Ordering::Relaxed);
                for subchunk in &mut *built_chunk.subchunk_geometry.lock().unwrap() {
                    subchunk.dirty = true;
                }
            }
        }

        *current_camera_pos = Some(camera_pos);
        current_camera_pos
    }
}

pub struct WorldRenderer {
    shader_program: MainThreadStore<glium::Program>,
    transparent_shader_program: MainThreadStore<glium::Program>,
    blit_shader_program: MainThreadStore<glium::Program>,
    block_atlas_texture: MainThreadStore<glium::texture::SrgbTexture2d>,
    chunk_store: FastDashMap<FName, ChunkStore>,
}

const UNLOADED_RENDER_DISTANCE: i32 = 32;
const EXISTING_CHUNK_COLOR_A: [f32; 3] = [1.0, 0.5, 0.0];
const EXISTING_CHUNK_COLOR_B: [f32; 3] = [1.0, 1.0, 0.0];

thread_local! {
    static DEFAULT_DRAW_PARAMS: glium::DrawParameters<'static> = glium::DrawParameters {
        depth: glium::Depth {
            test: glium::DepthTest::IfLess,
            write: true,
            ..Default::default()
        },
        backface_culling: glium::draw_parameters::BackfaceCullingMode::CullClockwise,
        ..Default::default()
    };
}

impl WorldRenderer {
    pub fn new(_mc_version: &str, resources: Arc<Resources>) -> WorldRenderer {
        WorldRenderer {
            shader_program: MainThreadStore::create(|| glium::Program::from_source(get_display(), MAIN_VERT_SHADER, MAIN_FRAG_SHADER, None).unwrap()),
            transparent_shader_program: MainThreadStore::create(|| {
                let (version_str, rest) = MAIN_FRAG_SHADER.split_at(MAIN_FRAG_SHADER.find('\n').unwrap());
                let fragment_shader = format!("{}\n#define TRANSPARENCY\n{}", version_str, rest);
                glium::Program::from_source(get_display(), MAIN_VERT_SHADER, fragment_shader.as_str(), None).unwrap()
            }),
            blit_shader_program: MainThreadStore::create(|| glium::Program::from_source(get_display(), BLIT_VERT_SHADER, BLIT_FRAG_SHADER, None).unwrap()),
            block_atlas_texture: MainThreadStore::create(move || {
                let atlas_image = glium::texture::RawImage2d::from_raw_rgba(
                    resources.block_atlas.data.clone(),
                    (resources.block_atlas.width, resources.block_atlas.height),
                );
                glium::texture::SrgbTexture2d::with_mipmaps(get_display(), atlas_image, glium::texture::MipmapsOption::AutoGeneratedMipmapsMax(resources.mipmap_levels)).unwrap()
            }),
            chunk_store: make_fast_dash_map(),
        }
    }

    #[profiling::function]
    fn chunk_render_worker(world: Arc<World>, stop: &dyn Fn() -> bool) {
        while !stop() {
            let dimension_id = world.camera.read().unwrap().dimension.clone();
            let dimension = match world.get_dimension(&dimension_id) {
                Some(d) => d,
                None => {
                    World::worker_yield();
                    continue;
                }
            };
            let render_distance_chunks = 16;

            for delta in ChunkPos::ZERO.square_range(render_distance_chunks as i32).iter() {
                let mut chunk_changed = false;
                let mut last_camera_pos = None;
                'subchunk_loop:
                for subchunk_y in dimension.min_y >> 4..=dimension.max_y >> 4 {
                    if stop() {
                        return;
                    }
                    loop {
                        profiling::scope!("build_worker lock loop");
                        if let Some(chunk_store) = world.renderer.chunk_store.get(&dimension_id) {
                            let camera_pos_guard = chunk_store.camera_pos.lock().unwrap();
                            if let Some(camera_pos) = *camera_pos_guard {
                                if WorldRenderer::build_subchunk_geometry(
                                    &world,
                                    &dimension,
                                    subchunk_y,
                                    delta,
                                    camera_pos,
                                    &mut last_camera_pos,
                                    &chunk_store,
                                    &mut chunk_changed,
                                    stop,
                                ) {
                                    continue 'subchunk_loop;
                                } else {
                                    break 'subchunk_loop;
                                }
                            }
                        }
                        if stop() {
                            return;
                        }
                        World::worker_yield();
                    }
                }

                if chunk_changed {
                    if let Some(camera_pos) = last_camera_pos {
                        let chunk_pos = camera_pos + delta;
                        let dimension_id = dimension_id.clone();
                        let world = world.clone();
                        crate::add_non_urgent_queued_task(move || {
                            WorldRenderer::upload_chunk_geometry(&*world, dimension_id, chunk_pos, render_distance_chunks);
                        });
                    }
                }
            }
            if stop() {
                return;
            }
            World::worker_yield();
        }
    }

    pub fn start_build_worker(world_ref: &WorldRef) {
        world_ref.spawn_worker(Self::chunk_render_worker);
    }

    #[profiling::function]
    #[allow(clippy::too_many_arguments)] // TODO: simplify this
    fn build_subchunk_geometry(
        world: &Arc<World>,
        dimension: &Dimension,
        subchunk_y: i32,
        delta: ChunkPos,
        camera_pos: ChunkPos,
        last_camera_pos: &mut Option<ChunkPos>,
        chunk_store: &impl Deref<Target=ChunkStore>,
        chunk_changed: &mut bool,
        stop: &dyn Fn() -> bool
    ) -> bool {
        let subchunk_index = (subchunk_y - (dimension.min_y >> 4)) as usize;

        if let Some(last_camera_pos) = last_camera_pos {
            if camera_pos != *last_camera_pos {
                return false;
            }
        } else {
            *last_camera_pos = Some(camera_pos);
        }

        let chunk_pos = camera_pos + delta;
        if let Some(chunk) = dimension.get_chunk(chunk_pos) {
            let built_chunk = chunk_store.get(chunk_pos);
            let mut subchunk_geometry_guard = built_chunk.subchunk_geometry.lock().unwrap();
            let mut subchunk_geometry = &mut (*subchunk_geometry_guard)[subchunk_index];
            if chunk.subchunks[subchunk_index].as_ref().map(|subchunk| subchunk.needs_redraw.swap(false, Ordering::Acquire)).unwrap_or(false)
                || subchunk_geometry.dirty
            {
                if stop() {
                    return false;
                }
                *chunk_changed = true;
                subchunk_geometry.clear();
                if let Some(subchunk) = &chunk.subchunks[subchunk_index] {
                    WorldRenderer::render_subchunk(&**world, &*dimension, chunk_pos, subchunk, subchunk_y, subchunk_geometry);
                }
                subchunk_geometry.dirty = false;
                subchunk_geometry.mark_for_upload = true;
            }
        } else {
            return false;
        }

        return true;
    }

    #[profiling::function]
    fn upload_chunk_geometry(world: &World, dimension: FName, chunk_pos: ChunkPos, render_distance: i32) -> Option<()> {
        let chunk_store = world.renderer.chunk_store.get(&dimension)?;
        let camera_pos_guard = {
            profiling::scope!("wait_camera_pos_guard");
            chunk_store.camera_pos.lock().unwrap()
        };
        let camera_pos = (*camera_pos_guard)?;
        if camera_pos.rectangular_distance(chunk_pos) > render_distance {
            // camera has moved away since this chunk was built
            return None;
        }
        let built_chunk = chunk_store.get(chunk_pos);
        let subchunk_geometry = built_chunk.subchunk_geometry.lock().unwrap();
        let mut baked_geometry = built_chunk.baked_geometry.borrow_mut();
        baked_geometry.opaque_geometry.set_buffer_data(&Geometry::join_vertices(subchunk_geometry.iter().map(|geom| &geom.opaque_geometry)));
        baked_geometry.transparent_geometry.set_buffer_data(&Geometry::join_vertices(subchunk_geometry.iter().map(|geom| &geom.transparent_geometry)));
        // TODO: sort translucent geometry
        baked_geometry.translucent_geometry.set_buffer_data(&Geometry::join_vertices(subchunk_geometry.iter().map(|geom| &geom.translucent_geometry)));

        built_chunk.ready.store(true, Ordering::Release);

        return None;
    }

    #[profiling::function]
    pub fn has_changed(&self) -> bool {
        true
    }

    #[profiling::function]
    pub fn render_world(&self, world: &World, target: &mut glium::Frame) {
        let (dimension, camera_pos, yaw, pitch) = {
            let camera = world.camera.read().unwrap();
            (camera.dimension.clone(), camera.pos, camera.yaw, camera.pitch)
        };
        let dimension_arc = match world.get_dimension(&dimension) {
            Some(d) => d,
            None => return,
        };

        let current_chunk: IVec2 = camera_pos.xz().floor().as_ivec2() >> 4i8;
        let render_distance_chunks = 16;
        let chunk_store = self.chunk_store.entry(dimension).or_insert_with(|| {
            ChunkStore::new(render_distance_chunks, ((dimension_arc.max_y - dimension_arc.min_y + 1) >> 4) as u32)
        }).downgrade();
        let _camera_chunk_guard = chunk_store.set_camera_pos(current_chunk);

        let dimension = &*dimension_arc;

        let fov = 70.0f32;
        let aspect_ratio = target.get_dimensions().0 as f32 / target.get_dimensions().1 as f32;
        let znear = 0.05f32;
        let zfar = render_distance_chunks as f32 * 64.0;
        let projection = Mat4::perspective_rh(fov.to_radians(), aspect_ratio, znear, zfar);
        let camera_yaw = yaw.to_radians();
        let camera_pitch = pitch.to_radians();
        let view_matrix = Mat4::from_rotation_x(-camera_pitch) * Mat4::from_rotation_y(-camera_yaw);
        let uniforms = |chunk_pos: ChunkPos| {
            uniform! {
                projection_matrix: projection.to_cols_array_2d(),
                view_matrix: (view_matrix * Mat4::from_translation((DVec3::new((chunk_pos.x << 4) as f64, 0.0, (chunk_pos.y << 4) as f64) - camera_pos).as_vec3())).to_cols_array_2d(),
                tex: self.block_atlas_texture
                    .sampled()
                    .magnify_filter(glium::uniforms::MagnifySamplerFilter::Nearest)
                    .minify_filter(glium::uniforms::MinifySamplerFilter::NearestMipmapLinear),
                ambient_light: 0.1f32,
                sky_brightness: 0.0f32,
                sky_darkness: 0.0f32,
                night_vision_strength: 0.0f32,
                gamma: 1.0f32,
            }
        };

        fn get_forward_vector(yaw: f32) -> glam::DVec2 {
            let yaw = yaw.to_radians();
            glam::DVec2::new(-yaw.sin() as f64, -yaw.cos() as f64)
        }
        let left_normal = get_forward_vector(yaw - 90.0f32 + fov);
        let right_normal = get_forward_vector(yaw + 90.0f32 - fov);
        let min_left_dot = left_normal.dot(camera_pos.xz());
        let min_right_dot = right_normal.dot(camera_pos.xz());

        self.render_existing_chunks(world, &*dimension, target, &uniforms(current_chunk), current_chunk);

        let mut chunks_to_render = Vec::new();

        for chunk_pos in current_chunk.square_range(render_distance_chunks as i32).iter() {
            let mut is_in_view = false;
            for delta in (IVec2::ZERO..=IVec2::ONE).iter() {
                let corner_pos = ((chunk_pos + delta) << 4i8).as_dvec2();
                if left_normal.dot(corner_pos) >= min_left_dot && right_normal.dot(corner_pos) >= min_right_dot {
                    is_in_view = true;
                    break;
                }
            }
            if is_in_view {
                let built_chunk = chunk_store.get(chunk_pos);
                if built_chunk.ready.load(Ordering::Acquire) {
                    chunks_to_render.push((chunk_pos, built_chunk));
                }
            }
        }

        for (pos, chunk) in &chunks_to_render {
            DEFAULT_DRAW_PARAMS.with(|params| {
                (*chunk.baked_geometry).borrow().opaque_geometry.draw(
                    target, &*self.shader_program, &uniforms(*pos), params
                );
            });
        }
        for (pos, chunk) in &chunks_to_render {
            DEFAULT_DRAW_PARAMS.with(|params| {
                (*chunk.baked_geometry).borrow().transparent_geometry.draw(
                    target, &*self.transparent_shader_program, &uniforms(*pos), params
                );
            });
        }

        let mut alpha_params = DEFAULT_DRAW_PARAMS.with(|params| params.clone());
        alpha_params.blend = glium::Blend::alpha_blending();

        for (pos, chunk) in &chunks_to_render {
            (*chunk.baked_geometry).borrow().translucent_geometry.draw(
                target, &*self.shader_program, &uniforms(*pos), &alpha_params
            );
        }
    }

    fn render_existing_chunks<U>(&self, world: &World, dimension: &Dimension, target: &mut Frame, uniforms: &U, current_chunk: IVec2)
    where
        U: glium::uniforms::Uniforms,
    {
        let mut existing_chunks_vertices = Vec::new();
        let mut existing_chunks_indices = Vec::new();
        for chunk_pos in current_chunk.square_range(UNLOADED_RENDER_DISTANCE).iter() {
            if dimension.does_chunk_exist(world, chunk_pos) {
                let color = if ((chunk_pos.x ^ chunk_pos.y) & 1) == 0 { EXISTING_CHUNK_COLOR_A } else { EXISTING_CHUNK_COLOR_B };
                let world_pos = |chunk_pos: ChunkPos| {
                    let chunk_pos = chunk_pos - current_chunk;
                    [(chunk_pos.x << 4) as f32, dimension.min_y as f32, (chunk_pos.y << 4) as f32]
                };
                existing_chunks_vertices.push(BlitVertex {
                    position: world_pos(chunk_pos),
                    color,
                });
                existing_chunks_vertices.push(BlitVertex {
                    position: world_pos(chunk_pos + ChunkPos::new(0, 1)),
                    color,
                });
                existing_chunks_vertices.push(BlitVertex {
                    position: world_pos(chunk_pos + ChunkPos::new(1, 1)),
                    color,
                });
                existing_chunks_vertices.push(BlitVertex {
                    position: world_pos(chunk_pos + ChunkPos::new(1, 0)),
                    color,
                });
                let index = (existing_chunks_vertices.len() - 4) as u32;
                existing_chunks_indices.push(index);
                existing_chunks_indices.push(index + 1);
                existing_chunks_indices.push(index + 2);
                existing_chunks_indices.push(index + 2);
                existing_chunks_indices.push(index + 3);
                existing_chunks_indices.push(index);
            }
        }
        let existing_chunks_vertices = glium::VertexBuffer::new(get_display(), &existing_chunks_vertices).unwrap();
        let existing_chunks_indices = glium::IndexBuffer::new(get_display(), glium::index::PrimitiveType::TrianglesList, &existing_chunks_indices).unwrap();
        DEFAULT_DRAW_PARAMS.with(|params| {
            target.draw(
                &existing_chunks_vertices,
                &existing_chunks_indices,
                &self.blit_shader_program,
                uniforms,
                params
            ).unwrap();
        });
    }

    fn render_subchunk(world: &World, dimension: &Dimension, chunk_pos: ChunkPos, subchunk: &Subchunk, subchunk_y: i32, out_geometry: &mut SubchunkGeometry) {
        for pos in (BlockPos::new(0, 0, 0)..BlockPos::new(16, 16, 16)).iter() {
            let block_state = subchunk.get_block_state(pos);
            let relative_pos = BlockPos::Y * (subchunk_y * 16) + pos;
            let world_pos = BlockPos::new(chunk_pos.x << 4, 0, chunk_pos.y << 4) + relative_pos;
            WorldRenderer::render_state(world, dimension, &block_state, relative_pos, world_pos, out_geometry);
        }
    }

    fn render_state(world: &World, dimension: &Dimension, state: &IBlockState, pos: BlockPos, world_pos: BlockPos, out_geometry: &mut SubchunkGeometry) {
        let color = blocks::get_block_color(world, dimension, world_pos, state);
        let baked_model = WorldRenderer::get_baked_model(world, state);
        for (dir, face) in &baked_model.faces {
            if let Some(dir) = dir {
                if let Some(neighbor) = dimension.get_block_state(world_pos + dir.forward()) {
                    let mut culling = false;
                    let neighbor_model = WorldRenderer::get_baked_model(world, &neighbor);
                    if let Some(neighbor_face) = neighbor_model.faces.get(&Some(dir.opposite())) {
                        if (face.cull_mask[0] & !neighbor_face.cull_mask[0]) == IVec4::ZERO && (face.cull_mask[1] & !neighbor_face.cull_mask[1]) == IVec4::ZERO {
                            culling = true;
                        }
                    }
                    if culling {
                        continue;
                    }
                }
            }
            let geom = match face.transparency {
                Transparency::Opaque => &mut out_geometry.opaque_geometry,
                Transparency::Transparent => &mut out_geometry.transparent_geometry,
                Transparency::Translucent => &mut out_geometry.translucent_geometry,
            };
            for quad in &face.quads {
                let convert_vertex = |vertex: &BakedModelVertex| {
                    util::Vertex {
                        position: [
                            vertex.position[0] + pos.x as f32,
                            vertex.position[1] + pos.y as f32,
                            vertex.position[2] + pos.z as f32,
                        ],
                        tex_coords: vertex.tex_coords,
                        lightmap_coords: [1.0, 0.0],
                        color: if vertex.tint { (color.as_vec3() / 255.0).to_array() } else { [1.0, 1.0, 1.0] },
                    }
                };
                geom.quads.push([convert_vertex(&quad[0]), convert_vertex(&quad[1]), convert_vertex(&quad[2]), convert_vertex(&quad[3])]);
            }
        }
    }

    fn get_baked_model(world: &World, state: &IBlockState) -> Arc<BakedModel> {
        match world.resources.baked_model_cache.get(state) {
            Some(model) => model.value().clone(),
            None => {
                world.resources.baked_model_cache.insert(state.clone(), Arc::new(WorldRenderer::bake_model(world, state)));
                world.resources.baked_model_cache.get(state).unwrap().value().clone()
            }
        }
    }

    fn bake_model(world: &World, state: &IBlockState) -> BakedModel {
        let atlas = &world.resources.block_atlas;
        let models = match world.resources.get_block_model(state) {
            Some(models) => models,
            None => return WorldRenderer::bake_missingno(atlas)
        };

        let mut baked_model = BakedModel {
            ambient_occlusion: true,
            ..Default::default()
        };

        for model in models {
            let model_transform = Mat4::from_translation(Vec3::new(0.5, 0.5, 0.5))
                * match model.x_rotation.rem_euclid(360) {
                    90 => Mat4::from_cols(Vec4::X, Vec4::Z, -Vec4::Y, Vec4::W),
                    180 => Mat4::from_cols(Vec4::X, -Vec4::Y, -Vec4::Z, Vec4::W),
                    270 => Mat4::from_cols(Vec4::X, -Vec4::Z, Vec4::Y, Vec4::W),
                    _ => Mat4::IDENTITY,
                }
                * match model.y_rotation.rem_euclid(360) {
                    90 => Mat4::from_cols(Vec4::Z, Vec4::Y, -Vec4::X, Vec4::W),
                    180 => Mat4::from_cols(-Vec4::X, Vec4::Y, -Vec4::Z, Vec4::W),
                    270 => Mat4::from_cols(-Vec4::Z, Vec4::Y, Vec4::X, Vec4::W),
                    _ => Mat4::IDENTITY,
                }
                * Mat4::from_translation(Vec3::new(-0.5, -0.5, -0.5))
                * (Mat4::from_scale(Vec3::ONE * 0.0625));
            let uvlock = model.uvlock;
            let model = model.model;
            baked_model.ambient_occlusion = baked_model.ambient_occlusion && model.ambient_occlusion;
            for element in &model.elements {
                let mut element_transform = match element.rotation.axis {
                    geom::Axis::X => Mat4::from_rotation_x(element.rotation.angle.to_radians()),
                    geom::Axis::Y => Mat4::from_rotation_y(element.rotation.angle.to_radians()),
                    geom::Axis::Z => Mat4::from_rotation_z(element.rotation.angle.to_radians()),
                };
                if element.rotation.rescale {
                    let scale = if element.rotation.angle.abs() == 22.5 {
                        f32::FRAC_PI_8().cos().recip()
                    } else {
                        f32::SQRT_2()
                    };
                    element_transform *= Mat4::from_scale(Vec3::new(scale, 1.0, scale));
                }
                element_transform = Mat4::from_translation(element.rotation.origin)
                    * element_transform
                    * Mat4::from_translation(-element.rotation.origin);
                element_transform = model_transform * element_transform;

                for (dir, face) in &element.faces {
                    // TODO: face.rotation
                    let (u1, v1, u2, v2) = if let Some(uv) = &face.uv {
                        (uv.u1, uv.v1, uv.u2, uv.v2)
                    } else {
                        match dir {
                            geom::Direction::PosX => (16.0 - element.to.z, 16.0 - element.to.y, 16.0 - element.from.z, 16.0 - element.from.y),
                            geom::Direction::NegX => (element.from.z, 16.0 - element.to.y, element.to.z, 16.0 - element.from.y),
                            geom::Direction::PosY => (element.from.x, element.from.z, element.to.x, element.to.z),
                            geom::Direction::NegY => (element.from.x, 16.0 - element.to.z, element.to.x, 16.0 - element.from.z),
                            geom::Direction::PosZ => (element.from.x, 16.0 - element.to.y, element.to.x, 16.0 - element.from.y),
                            geom::Direction::NegZ => (16.0 - element.to.x, 16.0 - element.to.y, 16.0 - element.from.x, 16.0 - element.from.y),
                        }
                    };
                    let (u1, v1, u2, v2) = if uvlock && dir.transform(&model_transform) == dir {
                        let uvlock_transform =
                            Mat4::from_translation(Vec3::new(0.5, 0.5, 0.5))
                                * Mat4::from_quat(Quat::from_rotation_arc(dir.forward().as_vec3(), Vec3::Y))
                                * Mat4::from_translation(Vec3::new(-0.5, -0.5, -0.5))
                                * model_transform
                                * Mat4::from_translation(Vec3::new(8.0, 8.0, 8.0))
                                * Mat4::from_quat(Quat::from_rotation_arc(Vec3::Y, dir.forward().as_vec3()))
                                * Mat4::from_translation(Vec3::new(-8.0, -8.0, -8.0));
                        let trans_uv = uvlock_transform.transform_point3(Vec3::new(u1, 0.0, v1));
                        let (trans_u1, trans_v1) = (trans_uv.x, trans_uv.z);
                        let trans_uv = model_transform.transform_point3(Vec3::new(u2, 0.0, v2));
                        let (trans_u2, trans_v2) = (trans_uv.x, trans_uv.z);
                        fn zero_safe_signum(n: f32) -> f32 {
                            // because rust is stupid
                            if n == 0.0 {
                                n
                            } else {
                                n.signum()
                            }
                        }
                        let (u1, u2) = if zero_safe_signum(u2 - u1) == zero_safe_signum(trans_u2 - trans_u1) {
                            (trans_u1, trans_u2)
                        } else {
                            (trans_u2, trans_u1)
                        };
                        let (v1, v2) = if zero_safe_signum(v2 - v1) == zero_safe_signum(trans_v2 - trans_v1) {
                            (trans_v1, trans_v2)
                        } else {
                            (trans_v2, trans_v1)
                        };
                        (u1, v1, u2, v2)
                    } else {
                        (u1 / 16.0, v1 / 16.0, u2 / 16.0, v2 / 16.0)
                    };
                    let sprite = match face.texture.strip_prefix('#')
                        .and_then(|texture| model.textures.get(texture))
                        .and_then(|texture| atlas.get_sprite(texture))
                    {
                        Some(sprite) => sprite,
                        None => return WorldRenderer::bake_missingno(atlas)
                    };
                    let (u1, v1, u2, v2) = ((sprite.u1 as f32).lerp(sprite.u2 as f32, u1), (sprite.v1 as f32).lerp(sprite.v2 as f32, v1), (sprite.u1 as f32).lerp(sprite.u2 as f32, u2), (sprite.v1 as f32).lerp(sprite.v2 as f32, v2));
                    let (u1, v1, u2, v2) = (u1 / atlas.width as f32, v1 / atlas.height as f32, u2 / atlas.width as f32, v2 / atlas.height as f32);
                    let dest_face = dir.transform(&element_transform);
                    let mut dest_face = if dest_face.forward().as_vec3().abs_diff_eq(element_transform.transform_vector3(dir.forward().as_vec3()).normalize(), 0.001) {
                        Some(dest_face)
                    } else {
                        None
                    };
                    let (vert1, vert2, vert3, vert4) = match dir {
                        geom::Direction::PosX =>
                            (Vec3::new(element.to.x, element.from.y, element.to.z), Vec3::new(element.to.x, element.from.y, element.from.z),
                             Vec3::new(element.to.x, element.to.y, element.from.z), Vec3::new(element.to.x, element.to.y, element.to.z)),
                        geom::Direction::NegX =>
                            (Vec3::new(element.from.x, element.from.y, element.from.z), Vec3::new(element.from.x, element.from.y, element.to.z),
                             Vec3::new(element.from.x, element.to.y, element.to.z), Vec3::new(element.from.x, element.to.y, element.from.z)),
                        geom::Direction::PosY =>
                            (Vec3::new(element.from.x, element.to.y, element.to.z), Vec3::new(element.to.x, element.to.y, element.to.z),
                             Vec3::new(element.to.x, element.to.y, element.from.z), Vec3::new(element.from.x, element.to.y, element.from.z)),
                        geom::Direction::NegY =>
                            (Vec3::new(element.from.x, element.from.y, element.from.z), Vec3::new(element.to.x, element.from.y, element.from.z),
                             Vec3::new(element.to.x, element.from.y, element.to.z), Vec3::new(element.from.x, element.from.y, element.to.z)),
                        geom::Direction::PosZ =>
                            (Vec3::new(element.from.x, element.from.y, element.to.z), Vec3::new(element.to.x, element.from.y, element.to.z),
                             Vec3::new(element.to.x, element.to.y, element.to.z), Vec3::new(element.from.x, element.to.y, element.to.z)),
                        geom::Direction::NegZ =>
                            (Vec3::new(element.to.x, element.from.y, element.from.z), Vec3::new(element.from.x, element.from.y, element.from.z),
                             Vec3::new(element.from.x, element.to.y, element.from.z), Vec3::new(element.to.x, element.to.y, element.from.z)),
                    };
                    if !dest_face.map(|dest_face| (vert1 * 0.125 - 1.0).dot(dest_face.forward().as_vec3()).abs_diff_eq(&1.0f32, 0.001)).unwrap_or(false) {
                        dest_face = None;
                    }
                    let cull_mask = if let Some(dest_face) = dest_face {
                        let transform = Affine3A::from_translation(Vec3::new(0.5, 0.5, 0.5))
                            * Affine3A::from_quat(Quat::from_rotation_arc(dest_face.forward().as_vec3(), Vec3::Z))
                            * Affine3A::from_translation(Vec3::new(-0.5, -0.5, -0.5))
                            * Affine3A::from_scale(Vec3::new(1.0 / 16.0, 1.0 / 16.0, 1.0 / 16.0));
                        let (vert1, vert2, vert4) = (transform.transform_point3(vert1).xy(), transform.transform_point3(vert2).xy(), transform.transform_point3(vert4).xy());
                        let face_transform = Affine2::from_cols(vert2 - vert1, vert4 - vert1, vert1);
                        let mut cull_mask = [IVec4::ZERO, IVec4::ZERO];
                        for x in 0..16 {
                            for y in 0..16 {
                                let transformed = face_transform.transform_point2(Vec2::new(x as f32 / 16.0, y as f32 / 16.0));
                                let (x, y) = (transformed.x, transformed.y);
                                let u = (sprite.u1 as f32).lerp(sprite.u2 as f32, x).round() as i32;
                                let v = (sprite.v1 as f32).lerp(sprite.v2 as f32, y).round() as i32;
                                let alpha = atlas.get_alpha(u.clamp(0, atlas.width as i32 - 1) as u32, v.clamp(0, atlas.height as i32 - 1) as u32);
                                if alpha == 255 {
                                    let mut x = ((x * 16.0).round() as i32).clamp(0, 15);
                                    let mut y = ((y * 16.0).round() as i32).clamp(0, 15);
                                    if dest_face.forward().dot(IVec3::ONE) == -1 {
                                        x = 15 - x;
                                        y = 15 - y;
                                    }
                                    cull_mask[(y >> 3) as usize][((y >> 1) & 3) as usize] |= 1 << (((y & 1) << 4) | x);
                                }
                            }
                        }
                        cull_mask
                    } else {
                        [IVec4::ZERO, IVec4::ZERO]
                    };
                    let dest_face = baked_model.faces.entry(dest_face).or_default();
                    let vert1 = BakedModelVertex {
                        position: element_transform.transform_point3(vert1).to_array(),
                        tex_coords: [u1, v2],
                        tint: face.tint_index != -1,
                    };
                    let vert2 = BakedModelVertex {
                        position: element_transform.transform_point3(vert2).to_array(),
                        tex_coords: [u2, v2],
                        tint: face.tint_index != -1,
                    };
                    let vert3 = BakedModelVertex {
                        position: element_transform.transform_point3(vert3).to_array(),
                        tex_coords: [u2, v1],
                        tint: face.tint_index != -1,
                    };
                    let vert4 = BakedModelVertex {
                        position: element_transform.transform_point3(vert4).to_array(),
                        tex_coords: [u1, v1],
                        tint: face.tint_index != -1,
                    };
                    dest_face.quads.push([vert1, vert2, vert3, vert4]);
                    dest_face.transparency = dest_face.transparency.merge(sprite.transparency);
                    dest_face.cull_mask[0] = dest_face.cull_mask[0] | cull_mask[0];
                    dest_face.cull_mask[1] = dest_face.cull_mask[1] | cull_mask[1];
                }
            }
        }
        baked_model
    }

    fn bake_missingno(atlas: &TextureAtlas) -> BakedModel {
        let sprite = atlas.get_sprite(&CommonFNames.MISSINGNO).unwrap();
        let (u1, v1, u2, v2) = (
            sprite.u1 as f32 / atlas.width as f32,
            sprite.v1 as f32 / atlas.height as f32,
            sprite.u2 as f32 / atlas.width as f32,
            sprite.v2 as f32 / atlas.height as f32
        );
        let faces = make_a_hash_map!(
            Some(geom::Direction::NegZ) => BakedModelFace {
                quads: vec![[
                    BakedModelVertex {
                        position: [0.0, 0.0, 0.0],
                        tex_coords: [u1, v1],
                        tint: false,
                    },
                    BakedModelVertex {
                        position: [0.0, 1.0, 0.0],
                        tex_coords: [u1, v2],
                        tint: false,
                    },
                    BakedModelVertex {
                        position: [1.0, 1.0, 0.0],
                        tex_coords: [u2, v2],
                        tint: false,
                    },
                    BakedModelVertex {
                        position: [1.0, 0.0, 0.0],
                        tex_coords: [u2, v1],
                        tint: false,
                    },
                ]],
                transparency: Transparency::Opaque,
                cull_mask: [!IVec4::ZERO, !IVec4::ZERO],
            },
            Some(geom::Direction::PosZ) => BakedModelFace {
                quads: vec![[
                    BakedModelVertex {
                        position: [0.0, 0.0, 1.0],
                        tex_coords: [u1, v1],
                        tint: false,
                    },
                    BakedModelVertex {
                        position: [1.0, 0.0, 1.0],
                        tex_coords: [u2, v1],
                        tint: false,
                    },
                    BakedModelVertex {
                        position: [1.0, 1.0, 1.0],
                        tex_coords: [u2, v2],
                        tint: false,
                    },
                    BakedModelVertex {
                        position: [0.0, 1.0, 1.0],
                        tex_coords: [u1, v2],
                        tint: false,
                    },
                ]],
                transparency: Transparency::Opaque,
                cull_mask: [!IVec4::ZERO, !IVec4::ZERO],
            },
            Some(geom::Direction::PosX) => BakedModelFace {
                quads: vec![[
                    BakedModelVertex {
                        position: [1.0, 0.0, 0.0],
                        tex_coords: [u1, v1],
                        tint: false,
                    },
                    BakedModelVertex {
                        position: [1.0, 1.0, 0.0],
                        tex_coords: [u1, v2],
                        tint: false,
                    },
                    BakedModelVertex {
                        position: [1.0, 1.0, 1.0],
                        tex_coords: [u2, v2],
                        tint: false,
                    },
                    BakedModelVertex {
                        position: [1.0, 0.0, 1.0],
                        tex_coords: [u2, v1],
                        tint: false,
                    },
                ]],
                transparency: Transparency::Opaque,
                cull_mask: [!IVec4::ZERO, !IVec4::ZERO],
            },
            Some(geom::Direction::NegX) => BakedModelFace {
                quads: vec![[
                    BakedModelVertex {
                        position: [0.0, 0.0, 1.0],
                        tex_coords: [u1, v1],
                        tint: false,
                    },
                    BakedModelVertex {
                        position: [0.0, 1.0, 1.0],
                        tex_coords: [u1, v2],
                        tint: false,
                    },
                    BakedModelVertex {
                        position: [0.0, 1.0, 0.0],
                        tex_coords: [u2, v2],
                        tint: false,
                    },
                    BakedModelVertex {
                        position: [0.0, 0.0, 0.0],
                        tex_coords: [u2, v1],
                        tint: false,
                    },
                ]],
                transparency: Transparency::Opaque,
                cull_mask: [!IVec4::ZERO, !IVec4::ZERO],
            },
            Some(geom::Direction::NegY) => BakedModelFace {
                quads: vec![[
                    BakedModelVertex {
                        position: [0.0, 0.0, 0.0],
                        tex_coords: [u1, v1],
                        tint: false,
                    },
                    BakedModelVertex {
                        position: [1.0, 0.0, 0.0],
                        tex_coords: [u2, v1],
                        tint: false,
                    },
                    BakedModelVertex {
                        position: [1.0, 0.0, 1.0],
                        tex_coords: [u2, v2],
                        tint: false,
                    },
                    BakedModelVertex {
                        position: [0.0, 0.0, 1.0],
                        tex_coords: [u1, v2],
                        tint: false,
                    },
                ]],
                transparency: Transparency::Opaque,
                cull_mask: [!IVec4::ZERO, !IVec4::ZERO],
            },
            Some(geom::Direction::PosY) => BakedModelFace {
                quads: vec![[
                    BakedModelVertex {
                        position: [0.0, 1.0, 1.0],
                        tex_coords: [u1, v1],
                        tint: false,
                    },
                    BakedModelVertex {
                        position: [1.0, 1.0, 1.0],
                        tex_coords: [u1, v2],
                        tint: false,
                    },
                    BakedModelVertex {
                        position: [1.0, 1.0, 0.0],
                        tex_coords: [u2, v2],
                        tint: false,
                    },
                    BakedModelVertex {
                        position: [0.0, 1.0, 0.0],
                        tex_coords: [u2, v1],
                        tint: false,
                    },
                ]],
                transparency: Transparency::Opaque,
                cull_mask: [!IVec4::ZERO, !IVec4::ZERO],
            },
        );
        return BakedModel { faces, ambient_occlusion: false }
    }
}

#[derive(Default)]
pub struct BakedModel {
    faces: AHashMap<Option<geom::Direction>, BakedModelFace>,
    ambient_occlusion: bool,
}

#[derive(Default)]
struct BakedModelFace {
    quads: Vec<Quad<BakedModelVertex>>,
    transparency: Transparency,
    // 256 bits of data, 1 if there is a pixel on the face touching this side
    cull_mask: [glam::IVec4; 2],
}

struct BakedModelVertex {
    position: [f32; 3],
    tex_coords: [f32; 2],
    tint: bool,
}

#[derive(Clone, Copy, Default)]
pub enum Transparency {
    #[default]
    Opaque,
    Translucent,
    Transparent,
}

impl Transparency {
    pub fn merge(self, other: Transparency) -> Transparency {
        match (self, other) {
            (Transparency::Translucent, _) => Transparency::Translucent,
            (_, Transparency::Translucent) => Transparency::Translucent,
            (Transparency::Transparent, _) => Transparency::Transparent,
            (_, Transparency::Transparent) => Transparency::Transparent,
            (Transparency::Opaque, Transparency::Opaque) => Transparency::Opaque,
        }
    }
}
