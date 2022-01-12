use glium::{Surface, uniform};
use crate::{CommonFNames, util};
use crate::resources::{Resources, TextureAtlas};
use crate::util::{FastDashMap, make_fast_dash_map};
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
        let atlas_image = glium::texture::RawImage2d::from_raw_rgba_reversed(
            &resources.block_atlas.data,
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
        self.render_state(world, &IBlockState::new(BlockState::new(&CommonFNames.STONE)), BlockPos::new(0, 0, 0), &mut geometry);
        let uniforms = uniform! {
                    matrix: [
                        [1.0, 0.0, 0.0, 0.0],
                        [0.0, 1.0, 0.0, 0.0],
                        [0.0, 0.0, 1.0, 0.0],
                        [0.0, 0.0, 0.0, 1.0f32],
                    ],
                    tex: &self.block_atlas_texture,
                    ambient_light: 0.1f32,
                    sky_brightness: 0.0f32,
                    sky_darkness: 0.0f32,
                    night_vision_strength: 0.0f32,
                    gamma: 1.0f32,
                };
        let vertex_buffer = glium::VertexBuffer::new(get_display(), &geometry.vertices).unwrap();
        let index_buffer = glium::IndexBuffer::new(get_display(), glium::index::PrimitiveType::TrianglesList, &geometry.indices).unwrap();
        target.draw(&vertex_buffer, &index_buffer, &self.shader_program, &uniforms, &Default::default()).unwrap();
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
        let _models = match world.resources.get_block_model(state) {
            Some(models) => models,
            None => return WorldRenderer::bake_missingno(atlas)
        };
        WorldRenderer::bake_missingno(atlas)
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
            BakedModelVertex {
                position: [0.0, 0.0, 0.0],
                tex_coords: [u1, v1],
            },
            BakedModelVertex {
                position: [16.0, 0.0, 0.0],
                tex_coords: [u2, v1],
            },
            BakedModelVertex {
                position: [16.0, 16.0, 0.0],
                tex_coords: [u2, v2],
            },
            BakedModelVertex {
                position: [0.0, 16.0, 0.0],
                tex_coords: [u1, v2],
            },
            BakedModelVertex {
                position: [0.0, 0.0, 16.0],
                tex_coords: [u1, v1],
            },
            BakedModelVertex {
                position: [16.0, 0.0, 16.0],
                tex_coords: [u2, v1],
            },
            BakedModelVertex {
                position: [16.0, 16.0, 16.0],
                tex_coords: [u2, v2],
            },
            BakedModelVertex {
                position: [0.0, 16.0, 16.0],
                tex_coords: [u1, v2],
            },
            BakedModelVertex {
                position: [16.0, 0.0, 0.0],
                tex_coords: [u1, v1],
            },
            BakedModelVertex {
                position: [16.0, 0.0, 16.0],
                tex_coords: [u2, v1],
            },
            BakedModelVertex {
                position: [16.0, 16.0, 16.0],
                tex_coords: [u2, v2],
            },
            BakedModelVertex {
                position: [16.0, 16.0, 0.0],
                tex_coords: [u1, v2],
            },
            BakedModelVertex {
                position: [0.0, 0.0, 16.0],
                tex_coords: [u1, v1],
            },
            BakedModelVertex {
                position: [0.0, 0.0, 0.0],
                tex_coords: [u2, v1],
            },
            BakedModelVertex {
                position: [0.0, 16.0, 0.0],
                tex_coords: [u2, v2],
            },
            BakedModelVertex {
                position: [0.0, 16.0, 16.0],
                tex_coords: [u1, v2],
            },
            BakedModelVertex {
                position: [0.0, 0.0, 0.0],
                tex_coords: [u1, v1],
            },
            BakedModelVertex {
                position: [16.0, 0.0, 0.0],
                tex_coords: [u2, v1],
            },
            BakedModelVertex {
                position: [16.0, 0.0, 16.0],
                tex_coords: [u2, v2],
            },
            BakedModelVertex {
                position: [0.0, 0.0, 16.0],
                tex_coords: [u1, v2],
            },
            BakedModelVertex {
                position: [0.0, 16.0, 16.0],
                tex_coords: [u1, v1],
            },
            BakedModelVertex {
                position: [0.0, 16.0, 0.0],
                tex_coords: [u2, v1],
            },
            BakedModelVertex {
                position: [16.0, 16.0, 0.0],
                tex_coords: [u2, v2],
            },
            BakedModelVertex {
                position: [16.0, 16.0, 16.0],
                tex_coords: [u1, v2],
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
        return BakedModel { vertices, indices }
    }
}

struct BakedModel {
    vertices: Vec<BakedModelVertex>,
    indices: Vec<u16>,
}

struct BakedModelVertex {
    position: [f32; 3],
    tex_coords: [f32; 2],
}
