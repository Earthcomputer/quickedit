use ahash::AHashMap;
use image::GenericImage;
use lazy_static::lazy_static;
use crate::fname::FName;
use crate::gl;
use crate::{renderer, util};
use crate::util::make_fast_dash_map;

lazy_static! {
    pub(super) static ref MAX_SUPPORTED_TEXTURE_SIZE: u32 = {
        let mut max_supported_texture_size = 0;
        unsafe {
            gl::GetIntegerv(gl::MAX_TEXTURE_SIZE, &mut max_supported_texture_size);
        }
        let mut actual_max = 32768.max(max_supported_texture_size);
        while actual_max >= 1024 {
            unsafe {
                gl::TexImage2D(gl::PROXY_TEXTURE_2D, 0, gl::RGBA as i32, actual_max, actual_max, 0, gl::RGBA, gl::UNSIGNED_BYTE, std::ptr::null_mut())
            };
            let mut result = 0;
            unsafe {
                gl::GetTexLevelParameteriv(gl::PROXY_TEXTURE_2D, 0, gl::TEXTURE_WIDTH, &mut result)
            };
            if result != 0 {
                return result as u32;
            }
            actual_max >>= 1;
        }
        max_supported_texture_size.max(1024) as u32
    };
}

#[derive(Default)]
pub struct TextureAtlas {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    sprites: AHashMap<FName, Sprite>,
}

impl TextureAtlas {
    pub fn get_sprite(&self, name: &FName) -> Option<&Sprite> {
        self.sprites.get(name)
    }

    pub fn get_alpha(&self, x: u32, y: u32) -> u8 {
        let index = (y * self.width + x) as usize;
        self.data[index * 4 + 3]
    }
}

pub struct Sprite {
    pub u1: u32,
    pub v1: u32,
    pub u2: u32,
    pub v2: u32,
    pub transparency: renderer::Transparency,
}

#[profiling::function]
pub(super) fn stitch<P: image::Pixel<Subpixel=u8> + 'static, I: image::GenericImageView<Pixel=P>>(
    textures: &AHashMap<FName, I>,
    mipmap_level: &mut u32,
    max_width: u32,
    max_height: u32
) -> Option<TextureAtlas> {
    let mut textures: Vec<_> = textures.iter().collect();
    textures.sort_by_key(|&(_, texture)| (!texture.width(), !texture.height()));
    for (_, texture) in &textures {
        *mipmap_level = (*mipmap_level).min(texture.width().trailing_zeros().min(texture.height().trailing_zeros()));
    }

    struct Slot<'a, P: image::Pixel<Subpixel=u8>, I: image::GenericImageView<Pixel=P>> {
        x: u32, y: u32, width: u32, height: u32, data: SlotData<'a, P, I>,
    }
    unsafe impl<P: image::Pixel<Subpixel=u8>, I: image::GenericImageView<Pixel=P>> Sync for Slot<'_, P, I> {}
    enum SlotData<'a, P: image::Pixel<Subpixel=u8>, I: image::GenericImageView<Pixel=P>> {
        Empty,
        Leaf(&'a FName, &'a I),
        Node(Vec<Slot<'a, P, I>>),
    }
    impl<'a, P: image::Pixel<Subpixel=u8>, I: image::GenericImageView<Pixel=P>> Slot<'a, P, I> {
        fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
            Slot { x, y, width, height, data: SlotData::Empty }
        }

        fn fit(&mut self, name: &'a FName, sprite: &'a I) -> bool {
            if let SlotData::Leaf(_, _) = self.data {
                return false;
            }

            let sprite_width = sprite.width();
            let sprite_height = sprite.height();
            if self.width < sprite_width || self.height < sprite_height {
                return false;
            }
            if self.width == sprite_width && self.height == sprite_height {
                self.data = SlotData::Leaf(name, sprite);
                return true;
            }
            if let SlotData::Empty = self.data {
                let mut sub_slots = vec![Slot::new(self.x, self.y, sprite_width, sprite_height)];
                let leftover_x = self.width - sprite_width;
                let leftover_y = self.height - sprite_height;
                if leftover_x == 0 {
                    sub_slots.push(Slot::new(self.x, self.y + sprite_height, sprite_width, leftover_y));
                } else if leftover_y == 0 {
                    sub_slots.push(Slot::new(self.x + sprite_width, self.y, leftover_x, sprite_height));
                } else if self.height < self.width {
                    sub_slots.push(Slot::new(self.x + sprite_width, self.y, leftover_x, sprite_height));
                    sub_slots.push(Slot::new(self.x, self.y + sprite_height, self.width, leftover_y));
                } else {
                    sub_slots.push(Slot::new(self.x, self.y + sprite_height, sprite_width, leftover_y));
                    sub_slots.push(Slot::new(self.x + sprite_width, self.y, leftover_x, self.height));
                }

                self.data = SlotData::Node(sub_slots);
            }
            if let SlotData::Node(ref mut sub_slots) = self.data {
                for sub_slot in sub_slots {
                    if sub_slot.fit(name, sprite) {
                        return true;
                    }
                }
            } else {
                unreachable!();
            }
            false
        }

        fn add_leafs(&'a self, leafs: &mut Vec<&Slot<'a, P, I>>) {
            if let SlotData::Leaf(_, _) = self.data {
                leafs.push(self);
            } else if let SlotData::Node(ref sub_slots) = self.data {
                for slot in sub_slots {
                    slot.add_leafs(leafs);
                }
            }
        }
    }
    let mut width = 0;
    let mut height = 0;
    let mut slots: Vec<Slot<P, I>> = Vec::with_capacity(256);

    'texture_loop:
    for (name, texture) in &textures {
        for slot in &mut slots {
            if slot.fit(name, texture) {
                continue 'texture_loop;
            }
        }

        // grow
        let current_effective_width = util::round_up_power_of_two(width);
        let current_effective_height = util::round_up_power_of_two(height);
        let expanded_width = util::round_up_power_of_two(width + texture.width());
        let expanded_height = util::round_up_power_of_two(height + texture.height());
        let can_expand_x = expanded_width <= max_width;
        let can_expand_y = expanded_height <= max_height;
        if !can_expand_x && !can_expand_y {
            return None;
        }
        let x_has_space_without_expanding = can_expand_x && current_effective_width != expanded_width;
        let y_has_space_without_expanding = can_expand_y && current_effective_height != expanded_height;
        let use_x = if x_has_space_without_expanding ^ y_has_space_without_expanding {
            x_has_space_without_expanding
        } else {
            can_expand_x && current_effective_width <= current_effective_height
        };

        let mut slot = if use_x {
            if height == 0 {
                height = texture.height();
            }
            let slot = Slot::new(width, 0, texture.width(), height);
            width += texture.width();
            slot
        } else {
            let slot = Slot::new(0, height, width, texture.height());
            height += texture.height();
            slot
        };

        slot.fit(name, *texture);
        slots.push(slot);
    }

    width = util::round_up_power_of_two(width);
    height = util::round_up_power_of_two(height);

    let mut leafs = Vec::with_capacity(textures.len());
    for slot in &slots {
        slot.add_leafs(&mut leafs);
    }
    let mut atlas: image::ImageBuffer<P, _> = image::ImageBuffer::new(width, height);
    let transparencies = make_fast_dash_map();
    unsafe {
        util::parallel_iter_to_output(&leafs, &mut atlas, |leaf, atlas| {
            if let SlotData::Leaf(name, texture) = leaf.data {
                atlas.copy_from(texture, leaf.x, leaf.y).unwrap();
                let transparency = calc_transparency(texture);
                transparencies.insert(name, transparency);
            } else {
                unreachable!();
            }
        });
    }
    let sprites = leafs.iter().map(|leaf| {
        if let SlotData::Leaf(name, _) = leaf.data {
            (name.clone(), Sprite {
                u1: leaf.x,
                v1: leaf.y,
                u2: leaf.x + leaf.width,
                v2: leaf.y + leaf.height,
                transparency: *transparencies.get(name).unwrap(),
            })
        } else {
            unreachable!()
        }
    }).collect();

    Some(TextureAtlas {
        width,
        height,
        data: atlas.into_raw(),
        sprites,
    })
}

#[profiling::function]
fn calc_transparency<P: image::Pixel<Subpixel=u8>, I: image::GenericImageView<Pixel=P>>(texture: &I) -> renderer::Transparency {
    let mut seen_transparent_pixel = false;
    for (_, _, pixel) in texture.pixels() {
        let alpha = pixel.to_rgba()[3];
        if alpha != 255 {
            if alpha != 0 {
                return renderer::Transparency::Translucent;
            }
            seen_transparent_pixel = true;
        }
    }
    if seen_transparent_pixel {
        renderer::Transparency::Transparent
    } else {
        renderer::Transparency::Opaque
    }
}
