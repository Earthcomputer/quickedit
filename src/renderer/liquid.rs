use glam::{IVec4, Vec2, Vec3, Vec3Swizzles};
use num_traits::FloatConst;
use crate::geom::{BlockPos, Direction, IVec3RangeExtensions};
use crate::renderer::storage::SubchunkGeometry;
use crate::{blocks, CommonFNames, fname, util, World};
use crate::blocks::Fluid;
use crate::renderer::bakery;
use crate::util::Lerp;
use crate::world::{Dimension, IBlockState};

fn get_velocity(world: &World, dimension: &Dimension, pos: BlockPos, state: &IBlockState, fluid: Fluid) -> Vec3 {
    let mut velocity = Vec3::ZERO;

    for dir in Direction::HORIZONTAL {
        let other_state = match dimension.get_block_state(pos + dir.forward()) {
            Some(state) => state,
            None => continue,
        };
        let other_fluid = blocks::get_fluid(&other_state);
        if other_fluid != Fluid::Empty && other_fluid != fluid {
            continue;
        }
        let height = match other_fluid {
            Fluid::Empty => 0.0,
            _ => get_level(&other_state) as f32 / 9.0,
        };
        let delta_height = if height == 0.0 {
            let blocks_movement_heuristic = other_fluid != Fluid::Empty && bakery::get_baked_model(world, &other_state)
                .faces.get(&Some(dir.opposite()))
                .map(|face| face.cull_mask != [IVec4::ZERO, IVec4::ZERO])
                .unwrap_or(false);
            if blocks_movement_heuristic {
                continue;
            }
            let below_state = match dimension.get_block_state(pos + dir.forward() - BlockPos::Y) {
                Some(state) => state,
                None => continue,
            };
            let below_fluid = blocks::get_fluid(&below_state);
            if below_fluid != fluid {
                continue;
            }
            let height = get_level(&below_state) as f32 / 9.0;
            if height == 0.0 {
                continue;
            }
            get_level(state) as f32 / 9.0 - (height - 8.0 / 9.0)
        } else {
            get_level(state) as f32 / 9.0 - height
        };

        if delta_height != 0.0 {
            velocity += dir.forward().as_vec3() * delta_height;
        }
    }

    let falling = dimension.get_block_state(pos - BlockPos::Y)
        .map(|s| blocks::get_fluid(&s) == fluid)
        .unwrap_or(false);
    if falling {
        for dir in Direction::HORIZONTAL {
            let offset_pos = pos + dir.forward();
            if is_solid_face(world, dimension, fluid, offset_pos, dir) || is_solid_face(world, dimension, fluid, offset_pos + BlockPos::Y, dir) {
                velocity = velocity.normalize_or_zero() + Vec3::new(0.0, -6.0, 0.0);
                break;
            }
        }
    }

    velocity.normalize_or_zero()
}

fn is_solid_face(world: &World, dimension: &Dimension, fluid: Fluid, pos: BlockPos, dir: Direction) -> bool {
    let state = match dimension.get_block_state(pos) {
        Some(state) => state,
        None => return false,
    };
    let fl = blocks::get_fluid(&state);
    if fl == fluid {
        return false;
    }
    if state.block == CommonFNames.ICE || state.block == CommonFNames.FROSTED_ICE {
        return false;
    }
    let model = bakery::get_baked_model(world, &state);
    match model.faces.get(&Some(dir.opposite())) {
        Some(face) => face.cull_mask == [!IVec4::ZERO, !IVec4::ZERO],
        None => false,
    }
}

fn should_render_side(world: &World, dimension: &Dimension, fluid: Fluid, pos: BlockPos, side: Direction) -> bool {
    let neighbor_state = match dimension.get_block_state(pos + side.forward()) {
        Some(state) => state,
        None => return true
    };
    if blocks::get_fluid(&neighbor_state) == fluid {
        return false;
    }
    let neighbor_model = bakery::get_baked_model(world, &neighbor_state);
    let neighbor_face = match neighbor_model.faces.get(&Some(side.opposite())) {
        Some(face) => face,
        None => return true
    };
    return neighbor_face.cull_mask != [!IVec4::ZERO, !IVec4::ZERO]
}

fn get_level(state: &IBlockState) -> u32 {
    let block = &state.block;
    if block == &CommonFNames.WATER || block == &CommonFNames.FLOWING_WATER || block == &CommonFNames.LAVA || block == &CommonFNames.FLOWING_LAVA {
        state.properties.get(&CommonFNames.LEVEL)
            .and_then(fname::to_int)
            .unwrap_or(8)
            .clamp(0, 8)
    } else {
        8
    }
}

fn get_north_west_fluid_height(dimension: &Dimension, pos: BlockPos, fluid: Fluid) -> f32 {
    let mut total_weight = 0;
    let mut height = 0.0;

    for other_pos in ((pos - BlockPos::new(1, 0, 1))..=pos).iter() {
        let fluid_above = dimension.get_block_state(other_pos + BlockPos::Y)
            .map(|s| blocks::get_fluid(&s) == fluid)
            .unwrap_or(false);
        if fluid_above {
            return 1.0;
        }

        let state_across = match dimension.get_block_state(other_pos) {
            Some(state) => state,
            None => continue,
        };
        if blocks::get_fluid(&state_across) == fluid {
            let h = get_level(&state_across) as f32 / 9.0;
            if h >= 0.8 {
                height += h * 10.0;
                total_weight += 10;
            } else {
                height += h;
                total_weight += 1;
            }
        }
    }

    return if total_weight == 0 {
        0.0
    } else {
        height / total_weight as f32
    };
}

pub(super) fn render_fluid(world: &World, dimension: &Dimension, state: &IBlockState, pos: BlockPos, world_pos: BlockPos, out_geometry: &mut SubchunkGeometry) {
    let fluid = blocks::get_fluid(state);

    let should_render_up = dimension.get_block_state(world_pos + BlockPos::Y)
        .map(|s| blocks::get_fluid(&s) != fluid)
        .unwrap_or(true);
    let should_render_down = should_render_side(world, dimension, fluid, world_pos, Direction::Down);
    let should_render_north = should_render_side(world, dimension, fluid, world_pos, Direction::North);
    let should_render_south = should_render_side(world, dimension, fluid, world_pos, Direction::South);
    let should_render_east = should_render_side(world, dimension, fluid, world_pos, Direction::East);
    let should_render_west = should_render_side(world, dimension, fluid, world_pos, Direction::West);
    if !should_render_up && !should_render_down && !should_render_north && !should_render_south && !should_render_east && !should_render_west {
        return;
    }

    let color = (blocks::get_block_color(world, dimension, world_pos, state).as_vec3() / 255.0).to_array();
    let atlas = &world.resources.block_atlas;
    let (still_sprite, flowing_sprite) = match fluid {
        Fluid::Water => (
            atlas.get_sprite(&CommonFNames.WATER_STILL).unwrap(),
            atlas.get_sprite(&CommonFNames.WATER_FLOW).unwrap(),
        ),
        _ => (
            atlas.get_sprite(&CommonFNames.LAVA_STILL).unwrap(),
            atlas.get_sprite(&CommonFNames.LAVA_FLOW).unwrap(),
        )
    };

    let mut nw_height = get_north_west_fluid_height(dimension, world_pos, fluid);
    let mut ne_height = get_north_west_fluid_height(dimension, world_pos + BlockPos::new(1, 0, 0), fluid);
    let mut sw_height = get_north_west_fluid_height(dimension, world_pos + BlockPos::new(0, 0, 1), fluid);
    let mut se_height = get_north_west_fluid_height(dimension, world_pos + BlockPos::new(1, 0, 1), fluid);

    if should_render_up {
        nw_height -= 0.001;
        ne_height -= 0.001;
        sw_height -= 0.001;
        se_height -= 0.001;

        let velocity = get_velocity(world, dimension, world_pos, state, fluid);

        let (u1, v1, u2, v2, u3, v3, u4, v4, transparency) = if velocity.xz() == Vec2::ZERO {
            let u1 = still_sprite.u1 as f32 / atlas.width as f32;
            let v1 = still_sprite.v1 as f32 / atlas.height as f32;
            let u2 = still_sprite.u2 as f32 / atlas.width as f32;
            let v2 = still_sprite.v2 as f32 / atlas.height as f32;
            (u1, v1, u1, v2, u2, v2, u2, v1, still_sprite.transparency)
        } else {
            let u1 = flowing_sprite.u1 as f32 / atlas.width as f32;
            let v1 = flowing_sprite.v1 as f32 / atlas.height as f32;
            let u2 = flowing_sprite.u2 as f32 / atlas.width as f32;
            let v2 = flowing_sprite.v2 as f32 / atlas.height as f32;
            let angle = velocity.z.atan2(velocity.x) - f32::FRAC_PI_2();
            let sin = angle.sin() * 0.25;
            let cos = angle.cos() * 0.25;
            (
                u1.lerp(u2, 0.5 + (-cos - sin)),
                v1.lerp(v2, 0.5 + (-cos + sin)),
                u1.lerp(u2, 0.5 + (-cos + sin)),
                v1.lerp(v2, 0.5 + (cos + sin)),
                u1.lerp(u2, 0.5 + (cos + sin)),
                v1.lerp(v2, 0.5 + (cos - sin)),
                u1.lerp(u2, 0.5 + (cos - sin)),
                v1.lerp(v2, 0.5 + (-cos - sin)),
                flowing_sprite.transparency,
            )
        };

        let geometry = out_geometry.get_geometry(transparency);
        let mut quad = [
            util::Vertex {
                position: [pos.x as f32, pos.y as f32 + nw_height, pos.z as f32],
                tex_coords: [u1, v1],
                lightmap_coords: [1.0, 0.0],
                color
            },
            util::Vertex {
                position: [pos.x as f32, pos.y as f32 + sw_height, pos.z as f32 + 1.0],
                tex_coords: [u2, v2],
                lightmap_coords: [1.0, 0.0],
                color
            },
            util::Vertex {
                position: [pos.x as f32 + 1.0, pos.y as f32 + se_height, pos.z as f32 + 1.0],
                tex_coords: [u3, v3],
                lightmap_coords: [1.0, 0.0],
                color
            },
            util::Vertex {
                position: [pos.x as f32 + 1.0, pos.y as f32 + ne_height, pos.z as f32],
                tex_coords: [u4, v4],
                lightmap_coords: [1.0, 0.0],
                color
            },
        ];
        geometry.quads.push(quad);
        quad.reverse();
        geometry.quads.push(quad);
    }

    if should_render_down {
        let u1 = still_sprite.u1 as f32 / atlas.width as f32;
        let v1 = still_sprite.v1 as f32 / atlas.height as f32;
        let u2 = still_sprite.u2 as f32 / atlas.width as f32;
        let v2 = still_sprite.v2 as f32 / atlas.height as f32;
        let quad = [
            util::Vertex {
                position: [pos.x as f32, pos.y as f32 + 0.001, pos.z as f32],
                tex_coords: [u1, v1],
                lightmap_coords: [1.0, 0.0],
                color
            },
            util::Vertex {
                position: [pos.x as f32 + 1.0, pos.y as f32 + 0.001, pos.z as f32],
                tex_coords: [u2, v1],
                lightmap_coords: [1.0, 0.0],
                color
            },
            util::Vertex {
                position: [pos.x as f32 + 1.0, pos.y as f32 + 0.001, pos.z as f32 + 1.0],
                tex_coords: [u2, v2],
                lightmap_coords: [1.0, 0.0],
                color
            },
            util::Vertex {
                position: [pos.x as f32, pos.y as f32 + 0.001, pos.z as f32 + 1.0],
                tex_coords: [u1, v2],
                lightmap_coords: [1.0, 0.0],
                color
            },
        ];
        out_geometry.get_geometry(still_sprite.transparency).quads.push(quad);
    }

    for dir in Direction::HORIZONTAL {
        let (height1, height2, x1, x2, z1, z2, should_render) = match dir {
            Direction::North => (
                nw_height,
                ne_height,
                pos.x as f32,
                pos.x as f32 + 1.0,
                pos.z as f32 + 0.001,
                pos.z as f32 + 0.001,
                should_render_north,
            ),
            Direction::South => (
                se_height,
                sw_height,
                pos.x as f32 + 1.0,
                pos.x as f32,
                pos.z as f32 + 1.0 - 0.001,
                pos.z as f32 + 1.0 - 0.001,
                should_render_south,
            ),
            Direction::West => (
                sw_height,
                nw_height,
                pos.x as f32 + 0.001,
                pos.x as f32 + 0.001,
                pos.z as f32 + 1.0,
                pos.z as f32,
                should_render_west,
            ),
            Direction::East => (
                ne_height,
                se_height,
                pos.x as f32 + 1.0 - 0.001,
                pos.x as f32 + 1.0 - 0.001,
                pos.z as f32,
                pos.z as f32 + 1.0,
                should_render_east,
            ),
            _ => unreachable!(),
        };

        if should_render {
            let mut sprite = flowing_sprite;
            let mut overlay = false;
            if fluid == Fluid::Water {
                if let Some(neighbor_state) = dimension.get_block_state(pos + dir.forward()) {
                    let model = bakery::get_baked_model(world, &neighbor_state);
                    if let Some(face) = model.faces.get(&Some(dir.opposite())) {
                        if face.cull_mask != [!IVec4::ZERO, !IVec4::ZERO] && face.collision_mask == [!IVec4::ZERO, !IVec4::ZERO] {
                            sprite = atlas.get_sprite(&CommonFNames.WATER_OVERLAY).unwrap();
                            overlay = true;
                        }
                    }
                }
            }

            let u1 = sprite.u1 as f32 / atlas.width as f32;
            let v1 = sprite.v1 as f32 / atlas.height as f32;
            let u2 = sprite.u2 as f32 / atlas.width as f32;
            let v2 = sprite.v2 as f32 / atlas.height as f32;
            let u2 = u1.lerp(u2, 0.5);
            let v2 = v1.lerp(v2, 0.5);
            let v3 = v1.lerp(v2, 1.0 - height1);
            let v4 = v1.lerp(v2, 1.0 - height2);

            let mut quad = [
                util::Vertex {
                    position: [x1, pos.y as f32 + height1, z1],
                    tex_coords: [u1, v3],
                    lightmap_coords: [1.0, 0.0],
                    color
                },
                util::Vertex {
                    position: [x2, pos.y as f32 + height2, z2],
                    tex_coords: [u2, v4],
                    lightmap_coords: [1.0, 0.0],
                    color
                },
                util::Vertex {
                    position: [x2, pos.y as f32 + 0.001, z2],
                    tex_coords: [u2, v2],
                    lightmap_coords: [1.0, 0.0],
                    color
                },
                util::Vertex {
                    position: [x1, pos.y as f32 + 0.001, z1],
                    tex_coords: [u1, v2],
                    lightmap_coords: [1.0, 0.0],
                    color
                },
            ];

            let geometry = out_geometry.get_geometry(sprite.transparency);
            geometry.quads.push(quad);
            if !overlay {
                quad.reverse();
                geometry.quads.push(quad);
            }
        }
    }
}
