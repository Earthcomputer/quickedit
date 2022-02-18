use std::sync::Arc;
use ahash::AHashMap;
use approx::AbsDiffEq;
use glam::{Affine3A, IVec3, IVec4, Mat4, Vec2, Vec3, Quat, Vec4, Affine2, Vec3Swizzles};
use num_traits::FloatConst;
use crate::{CommonFNames, geom};
use crate::make_a_hash_map;
use crate::renderer::storage::Quad;
use crate::resources::atlas::TextureAtlas;
use crate::util::Lerp;
use crate::world::{IBlockState, World};

pub(super) fn get_baked_model(world: &World, state: &IBlockState) -> Arc<BakedModel> {
    match world.resources.baked_model_cache.get(state) {
        Some(model) => model.value().clone(),
        None => {
            world.resources.baked_model_cache.insert(state.clone(), Arc::new(bake_model(world, state)));
            world.resources.baked_model_cache.get(state).unwrap().value().clone()
        }
    }
}

fn bake_model(world: &World, state: &IBlockState) -> BakedModel {
    let atlas = &world.resources.block_atlas;
    let models = match world.resources.get_block_model(state) {
        Some(models) => models,
        None => return bake_missingno(atlas)
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
                    None => return bake_missingno(atlas)
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
                let (cull_mask, collision_mask) = if let Some(dest_face) = dest_face {
                    let transform = Affine3A::from_translation(Vec3::new(0.5, 0.5, 0.5))
                        * Affine3A::from_quat(Quat::from_rotation_arc(dest_face.forward().as_vec3(), Vec3::Z))
                        * Affine3A::from_translation(Vec3::new(-0.5, -0.5, -0.5))
                        * Affine3A::from_scale(Vec3::new(1.0 / 16.0, 1.0 / 16.0, 1.0 / 16.0));
                    let (vert1, vert2, vert4) = (transform.transform_point3(vert1).xy(), transform.transform_point3(vert2).xy(), transform.transform_point3(vert4).xy());
                    let face_transform = Affine2::from_cols(vert2 - vert1, vert4 - vert1, vert1);
                    let mut cull_mask = [IVec4::ZERO, IVec4::ZERO];
                    let mut collision_mask = [IVec4::ZERO, IVec4::ZERO];
                    for x in 0..16 {
                        for y in 0..16 {
                            let transformed = face_transform.transform_point2(Vec2::new(x as f32 / 16.0, y as f32 / 16.0));
                            let (x, y) = (transformed.x, transformed.y);
                            let u = (sprite.u1 as f32).lerp(sprite.u2 as f32, x).round() as i32;
                            let v = (sprite.v1 as f32).lerp(sprite.v2 as f32, y).round() as i32;
                            let alpha = atlas.get_alpha(u.clamp(0, atlas.width as i32 - 1) as u32, v.clamp(0, atlas.height as i32 - 1) as u32);
                            let mut x = ((x * 16.0).round() as i32).clamp(0, 15);
                            let mut y = ((y * 16.0).round() as i32).clamp(0, 15);
                            if dest_face.forward().dot(IVec3::ONE) == -1 {
                                x = 15 - x;
                                y = 15 - y;
                            }
                            let i1 = (y >> 3) as usize;
                            let i2 = ((y >> 1) & 3) as usize;
                            let mask = 1 << (((y & 1) << 4) | x);
                            collision_mask[i1][i2] |= mask;
                            if alpha == 255 {
                                cull_mask[i1][i2] |= mask;
                            }
                        }
                    }
                    (cull_mask, collision_mask)
                } else {
                    ([IVec4::ZERO, IVec4::ZERO], [IVec4::ZERO, IVec4::ZERO])
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
                dest_face.collision_mask[0] = dest_face.collision_mask[0] | collision_mask[0];
                dest_face.collision_mask[1] = dest_face.collision_mask[1] | collision_mask[1];
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
                collision_mask: [!IVec4::ZERO, !IVec4::ZERO],
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
                collision_mask: [!IVec4::ZERO, !IVec4::ZERO],
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
                collision_mask: [!IVec4::ZERO, !IVec4::ZERO],
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
                collision_mask: [!IVec4::ZERO, !IVec4::ZERO],
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
                collision_mask: [!IVec4::ZERO, !IVec4::ZERO],
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
                collision_mask: [!IVec4::ZERO, !IVec4::ZERO],
            },
        );
    return BakedModel { faces, ambient_occlusion: false }
}

#[derive(Default)]
pub struct BakedModel {
    pub(super) faces: AHashMap<Option<geom::Direction>, BakedModelFace>,
    ambient_occlusion: bool,
}

#[derive(Default)]
pub(super) struct BakedModelFace {
    pub(super) quads: Vec<Quad<BakedModelVertex>>,
    pub(super) transparency: Transparency,
    // 256 bits of data, 1 if there is a pixel on the face touching this side
    pub(super) cull_mask: [glam::IVec4; 2],
    // 256 bits of data, 1 if there is a face covering the given pixel on this side
    pub(super) collision_mask: [glam::IVec4; 2],
}

pub(super) struct BakedModelVertex {
    pub(super) position: [f32; 3],
    pub(super) tex_coords: [f32; 2],
    pub(super) tint: bool,
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
