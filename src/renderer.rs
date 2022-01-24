use ahash::AHashMap;
use glam::{Mat4, Quat, Vec3, Vec3Swizzles, Vec4};
use glium::{Surface, uniform};
use num_traits::FloatConst;
use crate::{CommonFNames, geom, make_a_hash_map, util};
use crate::geom::{BlockPos, DVec3Extensions, IVec3Extensions, IVec3RangeExtensions};
use crate::resources::{Resources, TextureAtlas};
use crate::util::{FastDashMap, Lerp, make_fast_dash_map};
use crate::world::{Dimension, IBlockState, Subchunk, World};

const MAIN_VERT_SHADER: &str = include_str!("../res/main.vsh");
const MAIN_FRAG_SHADER: &str = include_str!("../res/main.fsh");

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

#[derive(Default)]
struct Geometry {
    vertices: Vec<util::Vertex>,
    indices: Vec<u32>,
}

#[derive(Default)]
struct ChunkGeometry {
    opaque_geometry: Geometry,
    transparent_geometry: Geometry,
    translucent_geometry: Geometry,
}

struct BakedGeometry {
    vertices: glium::VertexBuffer<util::Vertex>,
    indices: glium::IndexBuffer<u32>,
}

pub struct BakedChunkGeometry {
    opaque_geometry: BakedGeometry,
    transparent_geometry: BakedGeometry,
    translucent_geometry: BakedGeometry,
}

pub struct WorldRenderer {
    shader_program: glium::Program,
    transparent_shader_program: glium::Program,
    baked_model_cache: FastDashMap<IBlockState, BakedModel>,
    block_atlas_texture: glium::texture::SrgbTexture2d,
}

unsafe impl Send for WorldRenderer {}
unsafe impl Sync for WorldRenderer {}

impl WorldRenderer {
    pub fn new(_mc_version: &str, resources: &Resources) -> WorldRenderer {
        let atlas_image = glium::texture::RawImage2d::from_raw_rgba(
            resources.block_atlas.data.clone(),
            (resources.block_atlas.width, resources.block_atlas.height),
        );
        let (version_str, rest) = MAIN_FRAG_SHADER.split_at(MAIN_FRAG_SHADER.find('\n').unwrap());
        WorldRenderer {
            shader_program: glium::Program::from_source(get_display(), MAIN_VERT_SHADER, MAIN_FRAG_SHADER, None).unwrap(),
            transparent_shader_program: glium::Program::from_source(get_display(), MAIN_VERT_SHADER, format!("{}\n#define TRANSPARENCY\n{}", version_str, rest).as_str(), None).unwrap(),
            baked_model_cache: make_fast_dash_map(),
            block_atlas_texture: glium::texture::SrgbTexture2d::new(get_display(), atlas_image).unwrap(),
        }
    }

    pub fn has_changed(&self) -> bool {
        true
    }

    pub fn render_world(&self, world: &World, dimension: &Dimension, target: &mut glium::Frame) {
        let fov = 70.0f32;
        let render_distance_chunks = 16;
        let aspect_ratio = target.get_dimensions().0 as f32 / target.get_dimensions().1 as f32;
        let znear = 0.05f32;
        let zfar = render_distance_chunks as f32 * 64.0;
        let projection = Mat4::perspective_rh(fov.to_radians(), aspect_ratio, znear, zfar);
        let camera_pos = world.camera.pos.to_float();
        let camera_yaw = world.camera.yaw.to_radians();
        let camera_pitch = world.camera.pitch.to_radians();
        let view_matrix = Mat4::from_rotation_x(-camera_pitch) * Mat4::from_rotation_y(-camera_yaw) * Mat4::from_translation(-camera_pos);
        let uniforms = uniform! {
                    projection_matrix: projection.to_cols_array_2d(),
                    view_matrix: view_matrix.to_cols_array_2d(),
                    tex: self.block_atlas_texture.sampled().magnify_filter(glium::uniforms::MagnifySamplerFilter::Nearest),
                    ambient_light: 0.1f32,
                    sky_brightness: 0.0f32,
                    sky_darkness: 0.0f32,
                    night_vision_strength: 0.0f32,
                    gamma: 1.0f32,
                };

        fn get_forward_vector(yaw: f32, pitch: f32) -> glam::DVec3 {
            let pitch = pitch.to_radians();
            let yaw = yaw.to_radians();
            let x = -(pitch.cos() * yaw.sin());
            let y = pitch.sin();
            let z = -(pitch.cos() * yaw.cos());
            glam::DVec3::new(x as f64, y as f64, z as f64)
        }
        let camera_pos = world.camera.pos;
        let left_normal = get_forward_vector(world.camera.yaw - 90.0f32 + fov, world.camera.pitch);
        let right_normal = get_forward_vector(world.camera.yaw + 90.0f32 - fov, world.camera.pitch);
        let up_normal = get_forward_vector(world.camera.yaw, world.camera.pitch - 90.0f32 + fov);
        let down_normal = get_forward_vector(world.camera.yaw, world.camera.pitch + 90.0f32 - fov);
        let min_left_dot = left_normal.dot(camera_pos);
        let min_right_dot = right_normal.dot(camera_pos);
        let min_up_dot = up_normal.dot(camera_pos);
        let min_down_dot = down_normal.dot(camera_pos);
        let current_subchunk = camera_pos.floor().as_ivec3() >> 4;

        let mut subchunks_to_render = Vec::new();

        'subchunk_loop:
        for subchunk_pos in ((current_subchunk - glam::IVec3::ONE * render_distance_chunks)..=(current_subchunk + glam::IVec3::ONE * render_distance_chunks)).iter() {
            let subchunk_index = subchunk_pos.y - (dimension.min_y >> 4);
            if subchunk_index < 0 {
                continue;
            }
            let subchunk_index = subchunk_index as usize;

            for delta in (glam::IVec3::ZERO..=glam::IVec3::ONE).iter() {
                let pos = ((subchunk_pos + delta) * 16).as_dvec3();
                if left_normal.dot(pos) >= min_left_dot && right_normal.dot(pos) >= min_right_dot && up_normal.dot(pos) >= min_up_dot && down_normal.dot(pos) >= min_down_dot {
                    if let Some(achunk) = dimension.get_chunk(subchunk_pos.xz()) {
                        let chunk = achunk.as_ref();
                        if subchunk_index >= chunk.subchunks.len() {
                            continue 'subchunk_loop;
                        }
                        if let Some(subchunk) = &chunk.subchunks[subchunk_index] {
                            let mut cached_geom = subchunk.baked_geometry.borrow_mut();
                            if cached_geom.is_none() {
                                cached_geom.replace(self.render_subchunk(world, dimension, subchunk, subchunk_pos));
                            }
                            subchunks_to_render.push((achunk.clone(), subchunk_index));
                        }
                    }
                    break;
                }
            }
        }

        let params = glium::DrawParameters {
            depth: glium::Depth {
                test: glium::DepthTest::IfLess,
                write: true,
                ..Default::default()
            },
            backface_culling: glium::draw_parameters::BackfaceCullingMode::CullClockwise,
            ..Default::default()
        };
        for (chunk, index) in &subchunks_to_render {
            let subchunk = chunk.subchunks[*index].as_ref().unwrap();
            let subchunk_ref = subchunk.baked_geometry.borrow();
            let baked_subchunk = subchunk_ref.as_ref().unwrap();
            target.draw(
                &baked_subchunk.opaque_geometry.vertices,
                &baked_subchunk.opaque_geometry.indices,
                &self.shader_program,
                &uniforms,
                &params,
            ).unwrap();
        }
        for (chunk, index) in &subchunks_to_render {
            let subchunk = chunk.subchunks[*index].as_ref().unwrap();
            let subchunk_ref = subchunk.baked_geometry.borrow();
            let baked_subchunk = subchunk_ref.as_ref().unwrap();
            target.draw(
                &baked_subchunk.transparent_geometry.vertices,
                &baked_subchunk.transparent_geometry.indices,
                &self.transparent_shader_program,
                &uniforms,
                &params,
            ).unwrap();
        }
        let params = glium::DrawParameters {
            blend: glium::Blend::alpha_blending(),
            depth: glium::Depth {
                test: glium::DepthTest::IfLess,
                write: true,
                ..Default::default()
            },
            backface_culling: glium::draw_parameters::BackfaceCullingMode::CullClockwise,
            ..Default::default()
        };
        for (chunk, index) in &subchunks_to_render {
            let subchunk = chunk.subchunks[*index].as_ref().unwrap();
            let subchunk_ref = subchunk.baked_geometry.borrow();
            let baked_subchunk = subchunk_ref.as_ref().unwrap();
            // TODO: sort
            target.draw(
                &baked_subchunk.translucent_geometry.vertices,
                &baked_subchunk.translucent_geometry.indices,
                &self.shader_program,
                &uniforms,
                &params
            ).unwrap();
        }
    }

    fn render_subchunk(&self, world: &World, _dimension: &Dimension, subchunk: &Subchunk, subchunk_pos: BlockPos) -> BakedChunkGeometry {
        let mut chunk_geom = ChunkGeometry::default();
        for pos in (BlockPos::new(0, 0, 0)..BlockPos::new(16, 16, 16)).iter() {
            let block_state = subchunk.get_block_state(pos);
            self.render_state(world, block_state, subchunk_pos * 16 + pos, &mut chunk_geom);
        }

        // upload to gpu
        BakedChunkGeometry {
            opaque_geometry: BakedGeometry {
                vertices: glium::VertexBuffer::new(get_display(), &chunk_geom.opaque_geometry.vertices).unwrap(),
                indices: glium::IndexBuffer::new(get_display(), glium::index::PrimitiveType::TrianglesList, &chunk_geom.opaque_geometry.indices).unwrap(),
            },
            transparent_geometry: BakedGeometry {
                vertices: glium::VertexBuffer::new(get_display(), &chunk_geom.transparent_geometry.vertices).unwrap(),
                indices: glium::IndexBuffer::new(get_display(), glium::index::PrimitiveType::TrianglesList, &chunk_geom.transparent_geometry.indices).unwrap(),
            },
            translucent_geometry: BakedGeometry {
                vertices: glium::VertexBuffer::new(get_display(), &chunk_geom.translucent_geometry.vertices).unwrap(),
                indices: glium::IndexBuffer::new(get_display(), glium::index::PrimitiveType::TrianglesList, &chunk_geom.translucent_geometry.indices).unwrap(),
            },
        }
    }

    fn render_state(&self, world: &World, state: &IBlockState, pos: BlockPos, out_geometry: &mut ChunkGeometry) {
        let baked_model = self.get_baked_model(world, state);
        for (_dir, face) in &baked_model.faces {
            let geom = match face.transparency {
                Transparency::Opaque => &mut out_geometry.opaque_geometry,
                Transparency::Transparent => &mut out_geometry.transparent_geometry,
                Transparency::Translucent => &mut out_geometry.translucent_geometry,
            };
            let index = geom.vertices.len() as u32;
            for vertex in &face.vertices {
                geom.vertices.push(util::Vertex {
                    position: [
                        vertex.position[0] + pos.x as f32,
                        vertex.position[1] + pos.y as f32,
                        vertex.position[2] + pos.z as f32,
                    ],
                    tex_coords: vertex.tex_coords,
                    lightmap_coords: [1.0, 0.0],
                });
            }
            for i in &face.indices {
                geom.indices.push(index + *i as u32);
            }
        }
    }

    fn get_baked_model<'a>(&'a self, world: &World, state: &IBlockState) -> &'a BakedModel {
        match self.baked_model_cache.get(state) {
            Some(model) => model.value(),
            None => {
                self.baked_model_cache.insert(state.clone(), self.bake_model(world, state));
                self.baked_model_cache.get(state).unwrap().value()
            }
        }
    }

    fn bake_model(&self, world: &World, state: &IBlockState) -> BakedModel {
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
                                * Mat4::from_quat(Quat::from_rotation_arc(dir.forward().to_float(), Vec3::Y))
                                * Mat4::from_translation(Vec3::new(-0.5, -0.5, -0.5))
                                * model_transform
                                * Mat4::from_translation(Vec3::new(8.0, 8.0, 8.0))
                                * Mat4::from_quat(Quat::from_rotation_arc(Vec3::Y, dir.forward().to_float()))
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
                    let dest_face = baked_model.faces
                        .entry(face.cullface.map(|cullface| cullface.transform(&model_transform)))
                        .or_insert_with(BakedModelFace::default);
                    let index = dest_face.vertices.len() as u16;
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
                    dest_face.vertices.push(BakedModelVertex {
                        position: element_transform.transform_point3(vert1).to_array(),
                        tex_coords: [u1, v2]
                    });
                    dest_face.vertices.push(BakedModelVertex {
                        position: element_transform.transform_point3(vert2).to_array(),
                        tex_coords: [u2, v2]
                    });
                    dest_face.vertices.push(BakedModelVertex {
                        position: element_transform.transform_point3(vert3).to_array(),
                        tex_coords: [u2, v1]
                    });
                    dest_face.vertices.push(BakedModelVertex {
                        position: element_transform.transform_point3(vert4).to_array(),
                        tex_coords: [u1, v1]
                    });
                    dest_face.indices.push(index);
                    dest_face.indices.push(index + 1);
                    dest_face.indices.push(index + 2);
                    dest_face.indices.push(index + 2);
                    dest_face.indices.push(index + 3);
                    dest_face.indices.push(index);
                    dest_face.transparency = dest_face.transparency.merge(sprite.transparency);
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
                vertices: vec![
                    BakedModelVertex {
                        position: [0.0, 0.0, 0.0],
                        tex_coords: [u1, v1],
                    },
                    BakedModelVertex {
                        position: [0.0, 1.0, 0.0],
                        tex_coords: [u1, v2],
                    },
                    BakedModelVertex {
                        position: [1.0, 1.0, 0.0],
                        tex_coords: [u2, v2],
                    },
                    BakedModelVertex {
                        position: [1.0, 0.0, 0.0],
                        tex_coords: [u2, v1],
                    },
                ],
                indices: vec![0, 1, 2, 2, 3, 0],
                transparency: Transparency::Opaque,
            },
            Some(geom::Direction::PosZ) => BakedModelFace {
                vertices: vec![
                    BakedModelVertex {
                        position: [0.0, 0.0, 1.0],
                        tex_coords: [u1, v1],
                    },
                    BakedModelVertex {
                        position: [1.0, 0.0, 1.0],
                        tex_coords: [u2, v1],
                    },
                    BakedModelVertex {
                        position: [1.0, 1.0, 1.0],
                        tex_coords: [u2, v2],
                    },
                    BakedModelVertex {
                        position: [0.0, 1.0, 1.0],
                        tex_coords: [u1, v2],
                    },
                ],
                indices: vec![0, 1, 2, 2, 3, 0],
                transparency: Transparency::Opaque,
            },
            Some(geom::Direction::NegX) => BakedModelFace {
                vertices: vec![
                    BakedModelVertex {
                        position: [1.0, 0.0, 0.0],
                        tex_coords: [u1, v1],
                    },
                    BakedModelVertex {
                        position: [1.0, 1.0, 0.0],
                        tex_coords: [u1, v2],
                    },
                    BakedModelVertex {
                        position: [1.0, 1.0, 1.0],
                        tex_coords: [u2, v2],
                    },
                    BakedModelVertex {
                        position: [1.0, 0.0, 1.0],
                        tex_coords: [u2, v1],
                    },
                ],
                indices: vec![0, 1, 2, 2, 3, 0],
                transparency: Transparency::Opaque,
            },
            Some(geom::Direction::PosX) => BakedModelFace {
                vertices: vec![
                    BakedModelVertex {
                        position: [0.0, 0.0, 1.0],
                        tex_coords: [u1, v1],
                    },
                    BakedModelVertex {
                        position: [0.0, 1.0, 1.0],
                        tex_coords: [u1, v2],
                    },
                    BakedModelVertex {
                        position: [0.0, 1.0, 0.0],
                        tex_coords: [u2, v2],
                    },
                    BakedModelVertex {
                        position: [0.0, 0.0, 0.0],
                        tex_coords: [u2, v1],
                    },
                ],
                indices: vec![0, 1, 2, 2, 3, 0],
                transparency: Transparency::Opaque,
            },
            Some(geom::Direction::NegY) => BakedModelFace {
                vertices: vec![
                    BakedModelVertex {
                        position: [0.0, 0.0, 0.0],
                        tex_coords: [u1, v1],
                    },
                    BakedModelVertex {
                        position: [1.0, 0.0, 0.0],
                        tex_coords: [u2, v1],
                    },
                    BakedModelVertex {
                        position: [1.0, 0.0, 1.0],
                        tex_coords: [u2, v2],
                    },
                    BakedModelVertex {
                        position: [0.0, 0.0, 1.0],
                        tex_coords: [u1, v2],
                    },
                ],
                indices: vec![0, 1, 2, 2, 3, 0],
                transparency: Transparency::Opaque,
            },
            Some(geom::Direction::PosY) => BakedModelFace {
                vertices: vec![
                    BakedModelVertex {
                        position: [0.0, 1.0, 1.0],
                        tex_coords: [u1, v1],
                    },
                    BakedModelVertex {
                        position: [1.0, 1.0, 1.0],
                        tex_coords: [u1, v2],
                    },
                    BakedModelVertex {
                        position: [1.0, 1.0, 0.0],
                        tex_coords: [u2, v2],
                    },
                    BakedModelVertex {
                        position: [0.0, 1.0, 0.0],
                        tex_coords: [u2, v1],
                    },
                ],
                indices: vec![0, 1, 2, 2, 3, 0],
                transparency: Transparency::Opaque,
            },
        );
        return BakedModel { faces, ambient_occlusion: false }
    }
}

#[derive(Default)]
struct BakedModel {
    faces: AHashMap<Option<geom::Direction>, BakedModelFace>,
    ambient_occlusion: bool,
}

#[derive(Default)]
struct BakedModelFace {
    vertices: Vec<BakedModelVertex>,
    indices: Vec<u16>,
    transparency: Transparency,
}

struct BakedModelVertex {
    position: [f32; 3],
    tex_coords: [f32; 2],
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