//! Sprite modes 1 and 2.
//!
//! `sprite_line` evaluates one display line exactly as the VDP does:
//! attribute scan with the terminator Y, the per-line sprite limit, size
//! and magnification, early clock, mode 2 per-line colours with CC (OR
//! with the preceding sprite) and IC (no collision), priority, and
//! collision detection. Rendering uses only the colours; `tick` feeds the
//! overflow/collision results into S#0 and S#3..S#6.

use super::V9938;

#[derive(Debug, Clone)]
pub(super) struct SpriteLine {
    /// Sprite colour code per 256-resolution X, if a sprite pixel is shown.
    pub colors: [Option<u8>; 256],
    /// Number of the 5th (mode 1) / 9th (mode 2) sprite on the line.
    pub overflow: Option<u8>,
    /// Last sprite number examined during the attribute scan.
    pub last_checked: u8,
    /// First collision on the line as (x, y) in sprite coordinates.
    pub collision: Option<(u32, u32)>,
}

struct VisibleSprite {
    number: u8,
    row: u32,
}

impl V9938 {
    fn sprite_attribute_base(&self, mode: u8) -> u32 {
        let high = u32::from(self.regs[11] & 0x03) << 15;
        let r5 = u32::from(self.regs[5]);
        if mode == 1 {
            high | (r5 << 7)
        } else {
            high | ((r5 & 0xfc) << 7)
        }
    }

    pub(super) fn sprite_line(&self, y: usize) -> Option<SpriteLine> {
        let mode = self.screen_mode().sprite_mode()?;
        if self.regs[8] & 0x02 != 0 || !self.display_enabled() {
            return None;
        }

        let size: u32 = if self.regs[1] & 0x02 != 0 { 16 } else { 8 };
        let magnify: u32 = if self.regs[1] & 0x01 != 0 { 2 } else { 1 };
        let height = size * magnify;
        let vline = ((y as u32) + u32::from(self.regs[23])) & 0xff;
        let attribute_base = self.sprite_attribute_base(mode);
        let color_base = attribute_base.wrapping_sub(0x200) & super::VRAM_MASK;
        let pattern_base = u32::from(self.regs[6] & 0x3f) << 11;
        let (terminator, limit) = if mode == 1 {
            (208u8, 4usize)
        } else {
            (216u8, 8usize)
        };

        let mut visible: Vec<VisibleSprite> = Vec::with_capacity(limit);
        let mut overflow = None;
        let mut last_checked = 0u8;
        for number in 0..32u8 {
            last_checked = number;
            let entry = attribute_base + u32::from(number) * 4;
            let sprite_y = self.vram_byte(entry);
            if sprite_y == terminator {
                break;
            }
            let row = vline.wrapping_sub(u32::from(sprite_y)).wrapping_sub(1) & 0xff;
            if row >= height {
                continue;
            }
            if visible.len() == limit {
                overflow = Some(number);
                break;
            }
            visible.push(VisibleSprite {
                number,
                row: row / magnify,
            });
        }

        let transparent_zero = self.regs[8] & 0x20 == 0;
        let mut colors = [None; 256];
        // Index into `visible` of the sprite (group) that owns each pixel.
        let mut owner: [Option<usize>; 256] = [None; 256];
        // Collision occupancy: which sprite's pattern bit covers the pixel.
        let mut occupied: [bool; 256] = [false; 256];
        let mut collision = None;
        let mut group: Option<usize> = None;

        for (index, sprite) in visible.iter().enumerate() {
            let entry = attribute_base + u32::from(sprite.number) * 4;
            let x = i32::from(self.vram_byte(entry + 1));
            let mut pattern = u32::from(self.vram_byte(entry + 2));
            if size == 16 {
                pattern &= 0xfc;
            }
            let (early_clock, cc, ic, color) = if mode == 1 {
                let attr = self.vram_byte(entry + 3);
                (attr & 0x80 != 0, false, false, attr & 0x0f)
            } else {
                let attr = self.vram_byte(color_base + u32::from(sprite.number) * 16 + sprite.row);
                (
                    attr & 0x80 != 0,
                    attr & 0x40 != 0,
                    attr & 0x20 != 0,
                    attr & 0x0f,
                )
            };

            if cc {
                if group.is_none() {
                    continue; // CC sprite with no preceding sprite is not shown
                }
            } else {
                group = Some(index);
            }
            let this_group = group.unwrap();

            let left = self.vram_byte(pattern_base + pattern * 8 + sprite.row);
            let bits = if size == 16 {
                let right = self.vram_byte(pattern_base + pattern * 8 + 16 + sprite.row);
                (u32::from(left) << 8) | u32::from(right)
            } else {
                u32::from(left) << 8
            };
            let origin = x - if early_clock { 32 } else { 0 };

            for bit in 0..size {
                if bits & (0x8000 >> bit) == 0 {
                    continue;
                }
                for repeat in 0..magnify {
                    let px = origin + (bit * magnify + repeat) as i32;
                    if !(0..256).contains(&px) {
                        continue;
                    }
                    let px = px as usize;

                    if !cc && !ic {
                        if occupied[px] && collision.is_none() {
                            collision = Some((px as u32, vline));
                        }
                        occupied[px] = true;
                    }

                    let shown = color != 0 || !transparent_zero;
                    match owner[px] {
                        None => {
                            if shown {
                                colors[px] = Some(color);
                                owner[px] = Some(this_group);
                            }
                        }
                        Some(existing) if cc && existing == this_group => {
                            colors[px] = Some(colors[px].unwrap_or(0) | color);
                        }
                        Some(_) => {}
                    }
                }
            }
        }

        Some(SpriteLine {
            colors,
            overflow,
            last_checked,
            collision,
        })
    }

    fn vram_byte(&self, address: u32) -> u8 {
        self.vram[(address & super::VRAM_MASK) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::super::{default_palette_color, PORT_COMMAND, S0_5S, S0_C, V9938};

    /// GRAPHIC4 with mode 2 sprites: attributes 0x7600, colours 0x7400,
    /// patterns 0x7800.
    fn g4(r1_size_bits: u8) -> V9938 {
        let mut vdp = V9938::new();
        for (reg, value) in [
            (0u8, 0x06u8),
            (1, 0x40 | r1_size_bits),
            (2, 0x1f),
            (5, 0xef),
            (6, 0x0f),
            (7, 0x01),
            (8, 0x08),
            (11, 0x00),
        ] {
            vdp.write_register(reg, value);
        }
        for n in 0..32 {
            vdp.vram[0x7600 + n * 4] = 217;
        }
        vdp
    }

    /// GRAPHIC1 with mode 1 sprites: attributes 0x1b00, patterns 0x3800.
    fn g1() -> V9938 {
        let mut vdp = V9938::new();
        for (reg, value) in [
            (0u8, 0x00u8),
            (1, 0x40),
            (2, 0x06),
            (5, 0x36),
            (6, 0x07),
            (7, 0x01),
        ] {
            vdp.write_register(reg, value);
        }
        vdp
    }

    fn place2(vdp: &mut V9938, n: usize, x: u8, y: u8, pattern: u8, color: u8) {
        vdp.vram[0x7600 + n * 4] = y;
        vdp.vram[0x7600 + n * 4 + 1] = x;
        vdp.vram[0x7600 + n * 4 + 2] = pattern;
        for row in 0..16 {
            vdp.vram[0x7400 + n * 16 + row] = color;
        }
    }

    fn solid_pattern(vdp: &mut V9938, pattern: usize) {
        for byte in 0..32 {
            vdp.vram[0x7800 + pattern * 8 + byte] = 0xff;
        }
    }

    #[test]
    fn mode2_sprite_draws_one_line_below_its_y() {
        let mut vdp = g4(0);
        solid_pattern(&mut vdp, 0);
        place2(&mut vdp, 0, 20, 9, 0, 8);
        assert_eq!(vdp.native_pixel(20, 9), default_palette_color(1));
        assert_eq!(vdp.native_pixel(20, 10), default_palette_color(8));
        assert_eq!(vdp.native_pixel(27, 17), default_palette_color(8));
        assert_eq!(vdp.native_pixel(28, 10), default_palette_color(1));
        assert_eq!(vdp.native_pixel(20, 18), default_palette_color(1));
    }

    #[test]
    fn sixteen_by_sixteen_uses_four_quadrants_and_magnify_doubles() {
        let mut vdp = g4(0x02);
        vdp.vram[0x7800 + 4 * 8 + 16] = 0x01; // pattern 4, top-right quadrant, row 0
        place2(&mut vdp, 0, 0, 255, 5, 3); // pattern 5 masks to 4
        assert_eq!(vdp.native_pixel(15, 0), default_palette_color(3));
        assert_eq!(vdp.native_pixel(14, 0), default_palette_color(1));

        vdp.write_register(1, 0x43);
        assert_eq!(vdp.native_pixel(30, 0), default_palette_color(3));
        assert_eq!(vdp.native_pixel(31, 1), default_palette_color(3));
        assert_eq!(vdp.native_pixel(29, 0), default_palette_color(1));
    }

    #[test]
    fn y_216_ends_the_mode2_list() {
        let mut vdp = g4(0);
        solid_pattern(&mut vdp, 0);
        place2(&mut vdp, 0, 0, 216, 0, 8);
        place2(&mut vdp, 1, 50, 0, 0, 8);
        assert_eq!(vdp.native_pixel(50, 1), default_palette_color(1));
        vdp.vram[0x7600] = 217;
        assert_eq!(vdp.native_pixel(50, 1), default_palette_color(8));
    }

    #[test]
    fn lower_numbers_have_priority_and_colour_zero_is_transparent() {
        let mut vdp = g4(0);
        solid_pattern(&mut vdp, 0);
        place2(&mut vdp, 0, 10, 0, 0, 0);
        place2(&mut vdp, 1, 12, 0, 0, 9);
        place2(&mut vdp, 2, 14, 0, 0, 4);
        assert_eq!(vdp.native_pixel(11, 1), default_palette_color(1)); // transparent
        assert_eq!(vdp.native_pixel(13, 1), default_palette_color(9));
        assert_eq!(vdp.native_pixel(16, 1), default_palette_color(9)); // 1 over 2
        assert_eq!(vdp.native_pixel(21, 1), default_palette_color(4));

        vdp.write_register(8, 0x28); // TP: colour 0 is drawn
        assert_eq!(vdp.native_pixel(11, 1), default_palette_color(0));
    }

    #[test]
    fn early_clock_shifts_32_pixels_left() {
        let mut vdp = g4(0);
        solid_pattern(&mut vdp, 0);
        place2(&mut vdp, 0, 40, 0, 0, 0x87);
        assert_eq!(vdp.native_pixel(8, 1), default_palette_color(7));
        assert_eq!(vdp.native_pixel(40, 1), default_palette_color(1));
    }

    #[test]
    fn cc_sprites_or_with_the_preceding_sprite() {
        let mut vdp = g4(0);
        solid_pattern(&mut vdp, 0);
        place2(&mut vdp, 0, 0, 0, 0, 0x01);
        place2(&mut vdp, 1, 4, 0, 0, 0x40 | 0x04);
        assert_eq!(vdp.native_pixel(2, 1), default_palette_color(1));
        assert_eq!(vdp.native_pixel(5, 1), default_palette_color(5));
        assert_eq!(vdp.native_pixel(10, 1), default_palette_color(4));
    }

    #[test]
    fn cc_sprite_without_predecessor_is_hidden() {
        let mut vdp = g4(0);
        solid_pattern(&mut vdp, 0);
        place2(&mut vdp, 0, 0, 0, 0, 0x40 | 0x04);
        assert_eq!(vdp.native_pixel(2, 1), default_palette_color(1));
    }

    #[test]
    fn mode2_limits_eight_per_line_and_flags_the_ninth() {
        let mut vdp = g4(0);
        solid_pattern(&mut vdp, 0);
        for n in 0..10 {
            place2(&mut vdp, n, (n * 20) as u8, 0, 0, 2);
        }
        assert_eq!(vdp.native_pixel(7 * 20, 1), default_palette_color(2));
        assert_eq!(vdp.native_pixel(8 * 20, 1), default_palette_color(1));
        let line = vdp.sprite_line(1).unwrap();
        assert_eq!(line.overflow, Some(8));

        vdp.update_sprite_status(1);
        let s0 = vdp.peek_status(0);
        assert_ne!(s0 & S0_5S, 0);
        assert_eq!(s0 & 0x1f, 8);
    }

    #[test]
    fn collision_sets_c_and_coordinates_but_not_for_ic() {
        let mut vdp = g4(0);
        solid_pattern(&mut vdp, 0);
        place2(&mut vdp, 0, 10, 0, 0, 2);
        place2(&mut vdp, 1, 14, 0, 0, 0x20 | 3); // IC
        vdp.update_sprite_status(1);
        assert_eq!(vdp.peek_status(0) & S0_C, 0);

        place2(&mut vdp, 1, 14, 0, 0, 3);
        vdp.update_sprite_status(1);
        assert_ne!(vdp.peek_status(0) & S0_C, 0);
        assert_eq!(vdp.peek_status(3), 14 + 12);
        assert_eq!(vdp.peek_status(5), 1 + 8);

        vdp.write_register(15, 0);
        let _ = vdp.read_port(PORT_COMMAND);
        assert_eq!(vdp.peek_status(0) & S0_C, 0);
    }

    #[test]
    fn mode1_uses_attribute_colour_terminator_208_and_four_per_line() {
        let mut vdp = g1();
        for byte in 0..8 {
            vdp.vram[0x3800 + byte] = 0xff;
        }
        for n in 0..6usize {
            let entry = 0x1b00 + n * 4;
            vdp.vram[entry] = 0;
            vdp.vram[entry + 1] = (n * 10) as u8;
            vdp.vram[entry + 2] = 0;
            vdp.vram[entry + 3] = 6;
        }
        assert_eq!(vdp.native_pixel(30, 1), default_palette_color(6));
        assert_eq!(vdp.native_pixel(45, 1), default_palette_color(1));
        assert_eq!(vdp.sprite_line(1).unwrap().overflow, Some(4));

        vdp.vram[0x1b00 + 2 * 4] = 208;
        assert_eq!(vdp.native_pixel(20, 1), default_palette_color(1));
        assert_eq!(vdp.sprite_line(1).unwrap().last_checked, 2);
    }

    #[test]
    fn mode1_collides_regardless_of_colour() {
        let mut vdp = g1();
        vdp.vram[0x3800] = 0xff;
        vdp.vram[0x1b00 + 1] = 0;
        vdp.vram[0x1b00 + 3] = 0;
        vdp.vram[0x1b04 + 1] = 4;
        vdp.vram[0x1b04 + 3] = 0;
        vdp.vram[0x1b08] = 208;
        assert!(vdp.sprite_line(1).unwrap().collision.is_some());
    }

    #[test]
    fn sprites_are_disabled_by_spd_and_absent_in_text_modes() {
        let mut vdp = g4(0);
        solid_pattern(&mut vdp, 0);
        place2(&mut vdp, 0, 0, 0, 0, 2);
        vdp.write_register(8, 0x0a);
        assert!(vdp.sprite_line(1).is_none());
        vdp.write_register(8, 0x08);
        vdp.write_register(0, 0x04);
        vdp.write_register(1, 0x50);
        assert!(vdp.sprite_line(1).is_none());
    }

    #[test]
    fn sprites_follow_vertical_scroll() {
        let mut vdp = g4(0);
        solid_pattern(&mut vdp, 0);
        place2(&mut vdp, 0, 0, 49, 0, 2);
        assert_eq!(vdp.native_pixel(0, 50), default_palette_color(2));
        vdp.write_register(23, 10);
        assert_eq!(vdp.native_pixel(0, 40), default_palette_color(2));
    }

    #[test]
    fn graphic5_sprites_split_colour_bits_across_pixel_pair() {
        let mut vdp = g4(0);
        vdp.write_register(0, 0x08);
        vdp.write_register(8, 0x28);
        solid_pattern(&mut vdp, 0);
        place2(&mut vdp, 0, 0, 0, 0, 0b1101);
        let frame = vdp.frame();
        assert_eq!(frame.pixel(0, 1), default_palette_color(3));
        assert_eq!(frame.pixel(1, 1), default_palette_color(1));
    }

    #[test]
    fn graphic7_sprites_use_fixed_colours() {
        let mut vdp = g4(0);
        vdp.write_register(0, 0x0e);
        solid_pattern(&mut vdp, 0);
        place2(&mut vdp, 0, 0, 0, 0, 9); // fixed colour 9 = blue 7
        assert_eq!(vdp.native_pixel(0, 1), 0x0000ff);
    }
}
