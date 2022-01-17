use glam::{EulerRot, Mat4, Quat, Vec3, Vec4};
use glium::{Surface, uniform};
use num_traits::FloatConst;
use crate::{CommonFNames, fname, util, world};
use crate::resources::{Resources, TextureAtlas};
use crate::util::{FastDashMap, Lerp, make_fast_dash_map};
use crate::world::{BlockPos, BlockState, IBlockState, World};

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
pub struct Geometry {
    vertices: Vec<util::Vertex>,
    indices: Vec<u32>,
}

pub struct WorldRenderer {
    shader_program: glium::Program,
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
        WorldRenderer {
            shader_program: glium::Program::from_source(get_display(), MAIN_VERT_SHADER, MAIN_FRAG_SHADER, None).unwrap(),
            baked_model_cache: make_fast_dash_map(),
            block_atlas_texture: glium::texture::SrgbTexture2d::new(get_display(), atlas_image).unwrap(),
        }
    }

    pub fn has_changed(&self) -> bool {
        true
    }

    pub fn render_world(&self, world: &World, target: &mut glium::Frame) {
        let mut geometry = Geometry::default();
        self.render_state(world, &IBlockState::new(BlockState::new(&fname::from_str("brewing_stand"))), BlockPos::new(0, 0, 0), &mut geometry);

        let fov = 70.0f32;
        let aspect_ratio = target.get_dimensions().0 as f32 / target.get_dimensions().1 as f32;
        let znear = 0.05f32;
        let render_distance_chunks = 16.0f32;
        let zfar = render_distance_chunks * 64.0;
        let projection = Mat4::perspective_rh(fov.to_radians(), aspect_ratio, znear, zfar);
        let camera_pos = world::Pos::<f32>::from(world.camera.pos).to_glam();
        let camera_yaw = world.camera.yaw.to_radians();
        let camera_pitch = world.camera.pitch.to_radians();
        let view_matrix = Mat4::from_rotation_translation(Quat::from_euler(EulerRot::XYZ, camera_pitch, camera_yaw, 0.0), camera_pos).inverse();

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
        let vertex_buffer = glium::VertexBuffer::new(get_display(), &geometry.vertices).unwrap();
        let index_buffer = glium::IndexBuffer::new(get_display(), glium::index::PrimitiveType::TrianglesList, &geometry.indices).unwrap();
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
        target.draw(&vertex_buffer, &index_buffer, &self.shader_program, &uniforms, &params).unwrap();
    }

    fn render_state(&self, world: &World, state: &IBlockState, pos: BlockPos, out_geometry: &mut Geometry) {
        let baked_model = self.get_baked_model(world, state);
        let index = out_geometry.vertices.len() as u32;
        for vertex in &baked_model.vertices {
            out_geometry.vertices.push(util::Vertex {
                position: [
                    vertex.position[0] + pos.x as f32,
                    vertex.position[1] + pos.y as f32,
                    vertex.position[2] + pos.z as f32,
                ],
                tex_coords: vertex.tex_coords,
                lightmap_coords: [1.0, 0.0],
            });
        }
        for i in &baked_model.indices {
            out_geometry.indices.push(index + *i as u32);
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
                    90 => Mat4::from_cols(Vec4::X, Vec4::Z, -Vec4::Y, Vec4::ZERO),
                    180 => Mat4::from_cols(Vec4::X, -Vec4::Y, -Vec4::Z, Vec4::ZERO),
                    270 => Mat4::from_cols(Vec4::X, -Vec4::Z, Vec4::Y, Vec4::ZERO),
                    _ => Mat4::IDENTITY,
                }
                * match model.y_rotation.rem_euclid(360) {
                    90 => Mat4::from_cols(Vec4::Z, Vec4::Y, -Vec4::X, Vec4::ZERO),
                    180 => Mat4::from_cols(-Vec4::X, Vec4::Y, -Vec4::Z, Vec4::ZERO),
                    270 => Mat4::from_cols(-Vec4::Z, Vec4::Y, Vec4::X, Vec4::ZERO),
                    _ => Mat4::IDENTITY,
                }
                * Mat4::from_translation(Vec3::new(-0.5, -0.5, -0.5))
                * (Mat4::IDENTITY.mul_scalar(0.0625));

            let uvlock = model.uvlock;
            let model = model.model;
            baked_model.ambient_occlusion = baked_model.ambient_occlusion && model.ambient_occlusion;
            for element in &model.elements {
                let mut element_transform = match element.rotation.axis {
                    world::Axis::X => Mat4::from_rotation_x(element.rotation.angle.to_radians()),
                    world::Axis::Y => Mat4::from_rotation_y(element.rotation.angle.to_radians()),
                    world::Axis::Z => Mat4::from_rotation_z(element.rotation.angle.to_radians()),
                };
                if element.rotation.rescale {
                    let scale = if element.rotation.angle.abs() == 22.5 {
                        f32::FRAC_PI_8().cos().recip() - 1.0
                    } else {
                        f32::FRAC_PI_4().cos().recip() - 1.0
                    };
                    element_transform *= Mat4::from_scale(Vec3::new(scale, scale, scale));
                }
                element_transform = Mat4::from_translation(element.rotation.origin.to_glam())
                    * element_transform
                    * Mat4::from_translation(-element.rotation.origin.to_glam());
                element_transform = model_transform * element_transform;

                for (dir, face) in &element.faces {
                    // TODO: face.rotation, .cullface
                    let (u1, v1, u2, v2) = if let Some(uv) = &face.uv {
                        (uv.u1, uv.v1, uv.u2, uv.v2)
                    } else {
                        match dir {
                            world::Direction::PosX => (16.0 - element.to.z, 16.0 - element.to.y, 16.0 - element.from.z, 16.0 - element.from.y),
                            world::Direction::NegX => (element.from.z, 16.0 - element.to.y, element.to.z, 16.0 - element.from.y),
                            world::Direction::PosY => (element.from.x, element.from.z, element.to.x, element.to.z),
                            world::Direction::NegY => (element.from.x, 16.0 - element.to.z, element.to.x, 16.0 - element.from.z),
                            world::Direction::PosZ => (element.from.x, 16.0 - element.to.y, element.to.x, 16.0 - element.from.y),
                            world::Direction::NegZ => (16.0 - element.to.x, 16.0 - element.to.y, 16.0 - element.from.x, 16.0 - element.from.y),
                        }
                    };
                    let (u1, v1, u2, v2) = if uvlock {
                        let uvlock_transform = Mat4::from_quat(Quat::from_rotation_arc(world::Pos::<f32>::from(dir.forward()).to_glam(), Vec3::Z))
                            * model_transform
                            * Mat4::from_quat(Quat::from_rotation_arc(Vec3::Z, world::Pos::<f32>::from(dir.forward()).to_glam()));
                        let trans_uv = uvlock_transform.transform_point3(Vec3::new(u1, v1, 0.0));
                        let (trans_u1, trans_v1) = (trans_uv.x, trans_uv.y);
                        let trans_uv = model_transform.transform_point3(Vec3::new(u2, v2, 0.0));
                        let (trans_u2, trans_v2) = (trans_uv.x, trans_uv.y);
                        fn zero_safe_signum(n: f32) -> f32 {
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
                    let index = baked_model.vertices.len() as u16;
                    let (vert1, vert2, vert3, vert4) = match dir {
                        world::Direction::PosX =>
                            (Vec3::new(element.to.x, element.from.y, element.to.z), Vec3::new(element.to.x, element.from.y, element.from.z),
                             Vec3::new(element.to.x, element.to.y, element.from.z), Vec3::new(element.to.x, element.to.y, element.to.z)),
                        world::Direction::NegX =>
                            (Vec3::new(element.from.x, element.from.y, element.from.z), Vec3::new(element.from.x, element.from.y, element.to.z),
                             Vec3::new(element.from.x, element.to.y, element.to.z), Vec3::new(element.from.x, element.to.y, element.from.z)),
                        world::Direction::PosY =>
                            (Vec3::new(element.from.x, element.to.y, element.to.z), Vec3::new(element.to.x, element.to.y, element.to.z),
                             Vec3::new(element.to.x, element.to.y, element.from.z), Vec3::new(element.from.x, element.to.y, element.from.z)),
                        world::Direction::NegY =>
                            (Vec3::new(element.from.x, element.from.y, element.from.z), Vec3::new(element.to.x, element.from.y, element.from.z),
                             Vec3::new(element.to.x, element.from.y, element.to.z), Vec3::new(element.from.x, element.from.y, element.to.z)),
                        world::Direction::PosZ =>
                            (Vec3::new(element.from.x, element.from.y, element.to.z), Vec3::new(element.to.x, element.from.y, element.to.z),
                             Vec3::new(element.to.x, element.to.y, element.to.z), Vec3::new(element.from.x, element.to.y, element.to.z)),
                        world::Direction::NegZ =>
                            (Vec3::new(element.to.x, element.from.y, element.from.z), Vec3::new(element.from.x, element.from.y, element.from.z),
                             Vec3::new(element.from.x, element.to.y, element.from.z), Vec3::new(element.to.x, element.to.y, element.from.z)),
                    };
                    baked_model.vertices.push(BakedModelVertex {
                        position: element_transform.transform_point3(vert1).to_array(),
                        tex_coords: [u1, v2]
                    });
                    baked_model.vertices.push(BakedModelVertex {
                        position: element_transform.transform_point3(vert2).to_array(),
                        tex_coords: [u2, v2]
                    });
                    baked_model.vertices.push(BakedModelVertex {
                        position: element_transform.transform_point3(vert3).to_array(),
                        tex_coords: [u2, v1]
                    });
                    baked_model.vertices.push(BakedModelVertex {
                        position: element_transform.transform_point3(vert4).to_array(),
                        tex_coords: [u1, v1]
                    });
                    baked_model.indices.push(index);
                    baked_model.indices.push(index + 1);
                    baked_model.indices.push(index + 2);
                    baked_model.indices.push(index + 2);
                    baked_model.indices.push(index + 3);
                    baked_model.indices.push(index);
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
        let vertices = vec![
            // -Z
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
            // +Z
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
            // -X
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
            // +X
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
            // -Y
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
            // +Y
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
        ];
        let indices = vec![
            0, 1, 2,
            2, 3, 0,
            4, 5, 6,
            6, 7, 4,
            8, 9, 10,
            10, 11, 8,
            12, 13, 14,
            14, 15, 12,
            16, 17, 18,
            18, 19, 16,
            20, 21, 22,
            22, 23, 20,
        ];
        return BakedModel { vertices, indices, ambient_occlusion: false }
    }
}

#[derive(Default)]
struct BakedModel {
    vertices: Vec<BakedModelVertex>,
    indices: Vec<u16>,
    ambient_occlusion: bool,
}

struct BakedModelVertex {
    position: [f32; 3],
    tex_coords: [f32; 2],
}
