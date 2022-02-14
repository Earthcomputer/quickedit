use std::ops::Deref;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use glam::IVec4;
use crate::fname::FName;
use crate::geom::{BlockPos, ChunkPos, IVec2Extensions, IVec2RangeExtensions, IVec3RangeExtensions};
use crate::renderer::storage::{ChunkStore, Geometry, SubchunkGeometry};
use crate::renderer::{bakery, Transparency};
use crate::renderer::bakery::BakedModelVertex;
use crate::{blocks, util, World};
use crate::world::{Dimension, IBlockState, Subchunk};

#[profiling::function]
pub fn chunk_render_worker(world: Arc<World>, stop: &dyn Fn() -> bool) {
    while !stop() {
        let dimension_id = world.camera.read().unwrap().dimension.clone();
        let dimension = match world.get_dimension(&dimension_id) {
            Some(d) => d,
            None => {
                World::worker_yield();
                continue;
            }
        };
        let render_distance_chunks = crate::get_config().render_distance() as i32;

        for delta in ChunkPos::ZERO.square_range(render_distance_chunks).iter() {
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
                            if build_subchunk_geometry(
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
                        upload_chunk_geometry(&*world, dimension_id, chunk_pos, render_distance_chunks);
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
                render_subchunk(&**world, &*dimension, chunk_pos, subchunk, subchunk_y, subchunk_geometry);
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

fn render_subchunk(world: &World, dimension: &Dimension, chunk_pos: ChunkPos, subchunk: &Subchunk, subchunk_y: i32, out_geometry: &mut SubchunkGeometry) {
    for pos in (BlockPos::new(0, 0, 0)..BlockPos::new(16, 16, 16)).iter() {
        let block_state = subchunk.get_block_state(pos);
        let relative_pos = BlockPos::Y * (subchunk_y * 16) + pos;
        let world_pos = BlockPos::new(chunk_pos.x << 4, 0, chunk_pos.y << 4) + relative_pos;
        render_state(world, dimension, &block_state, relative_pos, world_pos, out_geometry);
    }
}

fn render_state(world: &World, dimension: &Dimension, state: &IBlockState, pos: BlockPos, world_pos: BlockPos, out_geometry: &mut SubchunkGeometry) {
    let color = blocks::get_block_color(world, dimension, world_pos, state);
    let baked_model = bakery::get_baked_model(world, state);
    for (dir, face) in &baked_model.faces {
        if let Some(dir) = dir {
            if let Some(neighbor) = dimension.get_block_state(world_pos + dir.forward()) {
                let mut culling = false;
                let neighbor_model = bakery::get_baked_model(world, &neighbor);
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