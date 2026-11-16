//! Screen-mode rendering.
//!
//! Every frame is drawn into a fixed 512x212 canvas. 256-pixel modes are
//! doubled horizontally, 512-pixel modes map 1:1, TEXT1 (240) and TEXT2
//! (480) sit inside the 256/512 grid with the chip's left border. R18
//! display adjust shifts the picture inside the canvas; uncovered areas
//! show the border colour.

use super::{rgb9, V9938};

pub const CANVAS_WIDTH: usize = 512;
pub const CANVAS_HEIGHT: usize = 212;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenMode {
    Text1,
    Text2,
    Multicolor,
    Graphic1,
    Graphic2,
    Graphic3,
    Graphic4,
    Graphic5,
    Graphic6,
    Graphic7,
    /// Mode-bit combination the datasheet leaves undefined: border only.
    Undefined,
}

impl ScreenMode {
    /// Horizontal resolution of the mode's native pixel grid.
    pub fn native_width(self) -> usize {
        match self {
            ScreenMode::Text1 => 240,
            ScreenMode::Text2 => 480,
            ScreenMode::Graphic5 | ScreenMode::Graphic6 => 512,
            _ => 256,
        }
    }

    pub fn is_bitmap(self) -> bool {
        matches!(
            self,
            ScreenMode::Graphic4
                | ScreenMode::Graphic5
                | ScreenMode::Graphic6
                | ScreenMode::Graphic7
        )
    }

    pub fn sprite_mode(self) -> Option<u8> {
        match self {
            ScreenMode::Multicolor | ScreenMode::Graphic1 | ScreenMode::Graphic2 => Some(1),
            ScreenMode::Graphic3
            | ScreenMode::Graphic4
            | ScreenMode::Graphic5
            | ScreenMode::Graphic6
            | ScreenMode::Graphic7 => Some(2),
            _ => None,
        }
    }

    /// Canvas X of the left edge of native pixel `x`.
    pub fn canvas_x(self, x: usize) -> usize {
        match self {
            ScreenMode::Text1 => (8 + x) * 2,
            ScreenMode::Text2 => 16 + x,
            ScreenMode::Graphic5 | ScreenMode::Graphic6 => x,
            _ => x * 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFrame {
    pub width: usize,
    pub height: usize,
    pub pixels_rgba: Vec<u32>,
    pub mode: ScreenMode,
    /// Active display lines (192 or 212).
    pub lines: usize,
}

impl VideoFrame {
    pub fn pixel(&self, x: usize, y: usize) -> u32 {
        self.pixels_rgba[(y * self.width) + x]
    }
}

/// Fixed sprite colours used in GRAPHIC7, as 9-bit G<<6 | R<<3 | B.
const G7_SPRITE_COLORS: [u16; 16] = [
    0, 2, 192, 194, 48, 50, 240, 242, 482, 7, 448, 455, 56, 63, 504, 511,
];

pub(super) fn grb332(value: u8) -> u32 {
    let g = value >> 5;
    let r = (value >> 2) & 0x07;
    let b2 = value & 0x03;
    rgb9(r, g, (b2 << 1) | (b2 >> 1))
}

fn grb9(value: u16) -> u32 {
    rgb9(
        ((value >> 3) & 7) as u8,
        ((value >> 6) & 7) as u8,
        (value & 7) as u8,
    )
}

impl V9938 {
    pub fn screen_mode(&self) -> ScreenMode {
        let r0 = self.regs[0];
        let r1 = self.regs[1];
        let m1 = (r1 >> 4) & 1;
        let m2 = (r1 >> 3) & 1;
        let m3 = (r0 >> 1) & 1;
        let m4 = (r0 >> 2) & 1;
        let m5 = (r0 >> 3) & 1;
        match (m5, m4, m3, m2, m1) {
            (0, 0, 0, 0, 0) => ScreenMode::Graphic1,
            (0, 0, 1, 0, 0) => ScreenMode::Graphic2,
            (0, 1, 0, 0, 0) => ScreenMode::Graphic3,
            (0, 1, 1, 0, 0) => ScreenMode::Graphic4,
            (1, 0, 0, 0, 0) => ScreenMode::Graphic5,
            (1, 0, 1, 0, 0) => ScreenMode::Graphic6,
            (1, 1, 1, 0, 0) => ScreenMode::Graphic7,
            (0, 0, 0, 1, 0) => ScreenMode::Multicolor,
            (0, 0, 0, 0, 1) => ScreenMode::Text1,
            (0, 1, 0, 0, 1) => ScreenMode::Text2,
            _ => ScreenMode::Undefined,
        }
    }

    pub fn display_enabled(&self) -> bool {
        self.regs[1] & 0x40 != 0
    }

    fn transparent_zero(&self) -> bool {
        self.regs[8] & 0x20 == 0
    }

    /// Border colour at canvas X (G5 alternates two colours per pixel pair).
    fn border_rgb(&self, mode: ScreenMode, canvas_x: usize) -> u32 {
        let r7 = self.regs[7];
        match mode {
            ScreenMode::Graphic7 => grb332(r7),
            ScreenMode::Graphic5 => {
                if canvas_x & 1 == 0 {
                    self.palette_rgb((r7 >> 2) & 0x03)
                } else {
                    self.palette_rgb(r7 & 0x03)
                }
            }
            _ => self.palette_rgb(r7 & 0x0f),
        }
    }

    /// Colour of a pattern/bitmap pixel; code 0 shows the border unless TP.
    fn color_rgb(&self, mode: ScreenMode, code: u8, canvas_x: usize) -> u32 {
        if code == 0 && self.transparent_zero() {
            self.border_rgb(mode, canvas_x)
        } else {
            self.palette_rgb(code)
        }
    }

    fn adjust(&self) -> (i32, i32) {
        let sign4 = |n: u8| i32::from(((n << 4) as i8) >> 4);
        let r18 = self.regs[18];
        (-sign4(r18 & 0x0f), -sign4(r18 >> 4))
    }

    /// Render the whole picture as currently programmed. Has no effect on
    /// status registers (those follow emulated time through `tick`).
    pub fn frame(&self) -> VideoFrame {
        let mode = self.screen_mode();
        let lines = self.display_lines();
        let (adjust_x, adjust_y) = self.adjust();
        let mut pixels = vec![0u32; CANVAS_WIDTH * CANVAS_HEIGHT];
        let mut line = [0u32; CANVAS_WIDTH];

        for canvas_y in 0..CANVAS_HEIGHT {
            let row = &mut pixels[canvas_y * CANVAS_WIDTH..(canvas_y + 1) * CANVAS_WIDTH];
            let y = canvas_y as i32 - adjust_y;
            if y < 0 || y >= lines as i32 {
                for (x, pixel) in row.iter_mut().enumerate() {
                    *pixel = self.border_rgb(mode, x);
                }
                continue;
            }
            self.render_line(y as usize, &mut line);
            for (x, pixel) in row.iter_mut().enumerate() {
                let source = x as i32 - (adjust_x * 2);
                *pixel = if (0..CANVAS_WIDTH as i32).contains(&source) {
                    line[source as usize]
                } else {
                    self.border_rgb(mode, x)
                };
            }
        }

        VideoFrame {
            width: CANVAS_WIDTH,
            height: CANVAS_HEIGHT,
            pixels_rgba: pixels,
            mode,
            lines,
        }
    }

    /// Colour of native pixel (x, y) of the current mode, sprites included,
    /// ignoring R18 display adjust.
    pub fn native_pixel(&self, x: usize, y: usize) -> u32 {
        let mut line = [0u32; CANVAS_WIDTH];
        self.render_line(y, &mut line);
        line[self.screen_mode().canvas_x(x)]
    }

    /// Render display line `y` (0-based, before vertical scroll) into a
    /// 512-wide canvas row.
    pub(super) fn render_line(&self, y: usize, out: &mut [u32; CANVAS_WIDTH]) {
        let mode = self.screen_mode();
        for (x, pixel) in out.iter_mut().enumerate() {
            *pixel = self.border_rgb(mode, x);
        }
        if !self.display_enabled() {
            return;
        }

        let vline = (y + usize::from(self.regs[23])) & 0xff;
        match mode {
            ScreenMode::Text1 => self.render_text1(vline, out),
            ScreenMode::Text2 => self.render_text2(vline, out),
            ScreenMode::Multicolor => self.render_multicolor(vline, out),
            ScreenMode::Graphic1 => self.render_graphic1(vline, out),
            ScreenMode::Graphic2 | ScreenMode::Graphic3 => self.render_graphic23(vline, out),
            ScreenMode::Graphic4 => self.render_graphic4(vline, out),
            ScreenMode::Graphic5 => self.render_graphic5(vline, out),
            ScreenMode::Graphic6 => self.render_graphic6(vline, out),
            ScreenMode::Graphic7 => self.render_graphic7(vline, out),
            ScreenMode::Undefined => return,
        }

        if let Some(sprites) = self.sprite_line(y) {
            for (x, color) in sprites.colors.iter().enumerate() {
                let Some(color) = *color else { continue };
                let (left, right) = match mode {
                    ScreenMode::Graphic7 => {
                        let rgb = grb9(G7_SPRITE_COLORS[usize::from(color & 0x0f)]);
                        (rgb, rgb)
                    }
                    ScreenMode::Graphic5 => (
                        self.palette_rgb((color >> 2) & 0x03),
                        self.palette_rgb(color & 0x03),
                    ),
                    _ => {
                        let rgb = self.palette_rgb(color);
                        (rgb, rgb)
                    }
                };
                out[x * 2] = left;
                out[x * 2 + 1] = right;
            }
        }
    }

    fn vram_at(&self, address: u32) -> u8 {
        self.vram[(address & super::VRAM_MASK) as usize]
    }

    /// Address in a table whose base register bits also mask the index
    /// (the V9938 ANDs low base-register bits into the generated address).
    fn masked(base: u32, index: u32, mask: u32) -> u32 {
        base | (index & mask)
    }

    fn put_pattern_pixels(
        &self,
        mode: ScreenMode,
        out: &mut [u32; CANVAS_WIDTH],
        native_x: usize,
        pattern: u8,
        width: usize,
        (fg, bg): (u8, u8),
    ) {
        for bit in 0..width {
            let code = if pattern & (0x80 >> bit) != 0 { fg } else { bg };
            let cx = mode.canvas_x(native_x + bit);
            let scale = if mode == ScreenMode::Text2 { 1 } else { 2 };
            for dx in 0..scale {
                out[cx + dx] = self.color_rgb(mode, code, cx + dx);
            }
        }
    }

    fn render_text1(&self, vline: usize, out: &mut [u32; CANVAS_WIDTH]) {
        let r2 = u32::from(self.regs[2] & 0x7f);
        let pattern_base = u32::from(self.regs[4] & 0x3f) << 11;
        let fg = self.regs[7] >> 4;
        let bg = self.regs[7] & 0x0f;
        let row = (vline / 8) as u32;
        let line_in_char = (vline & 7) as u32;
        for column in 0..40u32 {
            let name = Self::masked(0, row * 40 + column, (r2 << 10) | 0x3ff);
            let character = u32::from(self.vram_at(name));
            let pattern = self.vram_at(pattern_base + character * 8 + line_in_char);
            self.put_pattern_pixels(
                ScreenMode::Text1,
                out,
                column as usize * 6,
                pattern,
                6,
                (fg, bg),
            );
        }
    }

    fn render_text2(&self, vline: usize, out: &mut [u32; CANVAS_WIDTH]) {
        let r2 = u32::from(self.regs[2]);
        let r3 = u32::from(self.regs[3]);
        let name_base = (r2 & 0x7c) << 10;
        let name_mask = ((r2 & 0x03) << 10) | 0x3ff;
        let blink_base = (u32::from(self.regs[10] & 0x07) << 14) | ((r3 & 0xf8) << 6);
        let blink_mask = ((r3 & 0x07) << 6) | 0x3f;
        let pattern_base = u32::from(self.regs[4] & 0x3f) << 11;
        let row = (vline / 8) as u32;
        let line_in_char = (vline & 7) as u32;

        for column in 0..80u32 {
            let character =
                u32::from(self.vram_at(Self::masked(name_base, row * 80 + column, name_mask)));
            let pattern = self.vram_at(pattern_base + character * 8 + line_in_char);
            let blink_byte =
                self.vram_at(Self::masked(blink_base, row * 10 + column / 8, blink_mask));
            let blinking = self.blink_phase && blink_byte & (0x80 >> (column & 7)) != 0;
            let colors = if blinking {
                self.regs[12]
            } else {
                self.regs[7]
            };
            self.put_pattern_pixels(
                ScreenMode::Text2,
                out,
                column as usize * 6,
                pattern,
                6,
                (colors >> 4, colors & 0x0f),
            );
        }
    }

    fn render_multicolor(&self, vline: usize, out: &mut [u32; CANVAS_WIDTH]) {
        let name_base = u32::from(self.regs[2] & 0x7f) << 10;
        let pattern_base = u32::from(self.regs[4] & 0x3f) << 11;
        let row = (vline / 8) as u32;
        let line_in_char = (vline & 7) as u32;
        for column in 0..32u32 {
            let character = u32::from(self.vram_at(name_base + row * 32 + column));
            let colors =
                self.vram_at(pattern_base + character * 8 + (row & 3) * 2 + (line_in_char >> 2));
            for x in 0..8usize {
                let code = if x < 4 { colors >> 4 } else { colors & 0x0f };
                let cx = (column as usize * 8 + x) * 2;
                out[cx] = self.color_rgb(ScreenMode::Multicolor, code, cx);
                out[cx + 1] = out[cx];
            }
        }
    }

    fn render_graphic1(&self, vline: usize, out: &mut [u32; CANVAS_WIDTH]) {
        let name_base = u32::from(self.regs[2] & 0x7f) << 10;
        let pattern_base = u32::from(self.regs[4] & 0x3f) << 11;
        let color_base = (u32::from(self.regs[10] & 0x07) << 14) | (u32::from(self.regs[3]) << 6);
        let row = (vline / 8) as u32;
        let line_in_char = (vline & 7) as u32;
        for column in 0..32u32 {
            let character = u32::from(self.vram_at(name_base + row * 32 + column));
            let pattern = self.vram_at(pattern_base + character * 8 + line_in_char);
            let colors = self.vram_at(color_base + character / 8);
            self.put_pattern_pixels(
                ScreenMode::Graphic1,
                out,
                column as usize * 8,
                pattern,
                8,
                (colors >> 4, colors & 0x0f),
            );
        }
    }

    fn render_graphic23(&self, vline: usize, out: &mut [u32; CANVAS_WIDTH]) {
        let mode = self.screen_mode();
        let r3 = u32::from(self.regs[3]);
        let r4 = u32::from(self.regs[4]);
        let name_base = u32::from(self.regs[2] & 0x7f) << 10;
        let pattern_base = (r4 & 0x3c) << 11;
        let pattern_mask = ((r4 & 0x03) << 11) | 0x7ff;
        let color_base = (u32::from(self.regs[10] & 0x07) << 14) | ((r3 & 0x80) << 6);
        let color_mask = ((r3 & 0x7f) << 6) | 0x3f;
        let row = (vline / 8) as u32;
        let third = (row >> 3) & 0x03;
        let line_in_char = (vline & 7) as u32;
        for column in 0..32u32 {
            let character = u32::from(self.vram_at(name_base + row * 32 + column));
            let offset = ((third * 256 + character) * 8) + line_in_char;
            let pattern = self.vram_at(Self::masked(pattern_base, offset, pattern_mask));
            let colors = self.vram_at(Self::masked(color_base, offset, color_mask));
            self.put_pattern_pixels(
                mode,
                out,
                column as usize * 8,
                pattern,
                8,
                (colors >> 4, colors & 0x0f),
            );
        }
    }

    /// Page base for G4/G5 (32 KiB pages, R2 bits 6..5) with R13 alternate
    /// page and EO/IL field alternation applied.
    fn bitmap_line_address(&self, page_shift: u32, line_bytes: u32, vline: usize) -> u32 {
        let r2 = u32::from(self.regs[2]);
        let (page_mask, page_bit) = if page_shift == 15 {
            (0x60, 0x20)
        } else {
            (0x20, 0x20)
        };
        let mut page = r2 & page_mask;
        let alternate_r13 = self.regs[13] != 0 && self.blink_phase;
        let interlace_even = self.regs[9] & 0x04 != 0 && self.status[2] & super::S2_EO == 0;
        if alternate_r13 || interlace_even {
            page &= !page_bit;
        }
        let base = page << (page_shift - 5);
        let offset_mask = ((r2 & 0x1f) << (page_shift - 5)) | ((1 << (page_shift - 5)) - 1);
        Self::masked(base, vline as u32 * line_bytes, offset_mask)
    }

    fn render_graphic4(&self, vline: usize, out: &mut [u32; CANVAS_WIDTH]) {
        let line = self.bitmap_line_address(15, 128, vline);
        for byte in 0..128u32 {
            let value = self.vram_at(line + byte);
            for (half, code) in [(0, value >> 4), (1, value & 0x0f)] {
                let cx = (byte as usize * 2 + half) * 2;
                out[cx] = self.color_rgb(ScreenMode::Graphic4, code, cx);
                out[cx + 1] = out[cx];
            }
        }
    }

    fn render_graphic5(&self, vline: usize, out: &mut [u32; CANVAS_WIDTH]) {
        let line = self.bitmap_line_address(15, 128, vline);
        for byte in 0..128u32 {
            let value = self.vram_at(line + byte);
            for pixel in 0..4usize {
                let code = (value >> (6 - pixel * 2)) & 0x03;
                let cx = byte as usize * 4 + pixel;
                out[cx] = self.color_rgb(ScreenMode::Graphic5, code, cx);
            }
        }
    }

    fn render_graphic6(&self, vline: usize, out: &mut [u32; CANVAS_WIDTH]) {
        let line = self.bitmap_line_address(16, 256, vline);
        for byte in 0..256u32 {
            let value = self.vram_at(line + byte);
            for (half, code) in [(0, value >> 4), (1, value & 0x0f)] {
                let cx = byte as usize * 2 + half;
                out[cx] = self.color_rgb(ScreenMode::Graphic6, code, cx);
            }
        }
    }

    fn render_graphic7(&self, vline: usize, out: &mut [u32; CANVAS_WIDTH]) {
        let line = self.bitmap_line_address(16, 256, vline);
        let r7 = self.regs[7];
        for x in 0..256u32 {
            let value = self.vram_at(line + x);
            let rgb = if value == 0 && self.transparent_zero() {
                grb332(r7)
            } else {
                grb332(value)
            };
            out[x as usize * 2] = rgb;
            out[x as usize * 2 + 1] = rgb;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{default_palette_color, V9938};
    use super::*;

    fn vdp_with(regs: &[(usize, u8)]) -> V9938 {
        let mut vdp = V9938::new();
        for &(reg, value) in regs {
            vdp.write_register(reg as u8, value);
        }
        vdp
    }

    #[test]
    fn mode_bits_decode_to_every_screen_mode() {
        let cases = [
            (0x00, 0x40, ScreenMode::Graphic1),
            (0x02, 0x40, ScreenMode::Graphic2),
            (0x04, 0x40, ScreenMode::Graphic3),
            (0x06, 0x40, ScreenMode::Graphic4),
            (0x08, 0x40, ScreenMode::Graphic5),
            (0x0a, 0x40, ScreenMode::Graphic6),
            (0x0e, 0x40, ScreenMode::Graphic7),
            (0x00, 0x48, ScreenMode::Multicolor),
            (0x00, 0x50, ScreenMode::Text1),
            (0x04, 0x50, ScreenMode::Text2),
            (0x02, 0x50, ScreenMode::Undefined),
        ];
        for (r0, r1, mode) in cases {
            assert_eq!(
                vdp_with(&[(0, r0), (1, r1)]).screen_mode(),
                mode,
                "{r0:02x} {r1:02x}"
            );
        }
    }

    #[test]
    fn text2_renders_six_pixel_glyphs_from_vram() {
        let mut vdp = vdp_with(&[(0, 0x04), (1, 0x50), (2, 0x03), (4, 0x02), (7, 0xf4)]);
        vdp.vram[1] = b'A'; // row 0, column 1
        vdp.vram[0x1000 + usize::from(b'A') * 8] = 0b1000_0100;
        let fg = default_palette_color(15);
        let bg = default_palette_color(4);
        assert_eq!(vdp.native_pixel(6, 0), fg);
        assert_eq!(vdp.native_pixel(7, 0), bg);
        assert_eq!(vdp.native_pixel(11, 0), fg);
        assert_eq!(vdp.native_pixel(12, 0), bg); // bit 1 is not displayed
        let frame = vdp.frame();
        assert_eq!(frame.mode, ScreenMode::Text2);
        assert_eq!(frame.pixel(16 + 6, 0), fg);
        assert_eq!(frame.pixel(0, 0), bg); // border
    }

    #[test]
    fn text2_r2_low_bits_mask_the_name_table() {
        // With R2 = 0x00, A10 of the name address is forced low: row 13
        // (offset 0x410) aliases to offset 0x010.
        let mut vdp = vdp_with(&[(0, 0x04), (1, 0x50), (2, 0x00), (4, 0x02), (7, 0xf4)]);
        vdp.vram[0x010] = 1;
        vdp.vram[0x1000 + 8] = 0xfc;
        let fg = default_palette_color(15);
        assert_eq!(vdp.native_pixel(0, 13 * 8), fg);
        vdp.write_register(2, 0x03);
        assert_ne!(vdp.native_pixel(0, 13 * 8), fg);
    }

    #[test]
    fn text2_blink_uses_r12_colours_during_on_phase() {
        let mut vdp = vdp_with(&[
            (0, 0x04),
            (1, 0x50),
            (2, 0x03),
            (3, 0x27),
            (4, 0x02),
            (7, 0xf4),
            (12, 0x1a),
        ]);
        vdp.vram[0x1000] = 0x00; // char 0 is blank
        vdp.vram[0x0800] = 0x80; // blink column 0
        assert_eq!(vdp.native_pixel(0, 0), default_palette_color(4));
        vdp.blink_phase = true;
        assert_eq!(vdp.native_pixel(0, 0), default_palette_color(0x0a));
        assert_eq!(vdp.native_pixel(6, 0), default_palette_color(4));
    }

    #[test]
    fn text1_renders_40_columns_with_left_border() {
        let mut vdp = vdp_with(&[(0, 0x00), (1, 0x50), (2, 0x00), (4, 0x01), (7, 0x21)]);
        vdp.vram[0] = 5;
        vdp.vram[0x800 + 5 * 8] = 0x80;
        assert_eq!(vdp.native_pixel(0, 0), default_palette_color(2));
        assert_eq!(vdp.native_pixel(1, 0), default_palette_color(1));
        let frame = vdp.frame();
        assert_eq!(frame.pixel(16, 0), default_palette_color(2));
        assert_eq!(frame.pixel(17, 0), default_palette_color(2));
        assert_eq!(frame.pixel(15, 0), default_palette_color(1));
    }

    #[test]
    fn graphic1_uses_one_colour_byte_per_eight_patterns() {
        let mut vdp = vdp_with(&[(0, 0x00), (1, 0x40), (2, 0x06), (3, 0x80), (4, 0x00)]);
        vdp.vram[0x1800] = 9;
        vdp.vram[9 * 8] = 0x80;
        vdp.vram[0x2000 + 1] = 0x6c;
        assert_eq!(vdp.native_pixel(0, 0), default_palette_color(6));
        assert_eq!(vdp.native_pixel(1, 0), default_palette_color(0x0c));
    }

    #[test]
    fn graphic2_selects_pattern_and_colour_by_screen_third() {
        let mut vdp = vdp_with(&[(0, 0x02), (1, 0x40), (2, 0x06), (3, 0xff), (4, 0x03)]);
        // row 8 (second third), character 1
        vdp.vram[0x1800 + 8 * 32] = 1;
        let offset = (256 + 1) * 8;
        vdp.vram[offset] = 0x80;
        vdp.vram[0x2000 + offset] = 0xa2;
        assert_eq!(vdp.native_pixel(0, 64), default_palette_color(0x0a));
        assert_eq!(vdp.native_pixel(1, 64), default_palette_color(0x02));
    }

    #[test]
    fn multicolor_draws_four_by_four_blocks() {
        let mut vdp = vdp_with(&[(0, 0x00), (1, 0x48), (2, 0x02), (4, 0x00)]);
        vdp.vram[0x0800] = 3; // row 0 col 0 -> pattern 3
        vdp.vram[3 * 8] = 0x5a; // top blocks
        vdp.vram[3 * 8 + 1] = 0x7c; // bottom blocks
        assert_eq!(vdp.native_pixel(0, 0), default_palette_color(5));
        assert_eq!(vdp.native_pixel(4, 3), default_palette_color(0x0a));
        assert_eq!(vdp.native_pixel(0, 4), default_palette_color(7));
        assert_eq!(vdp.native_pixel(7, 7), default_palette_color(0x0c));
    }

    #[test]
    fn graphic4_high_nibble_is_the_left_pixel() {
        let mut vdp = vdp_with(&[(0, 0x06), (1, 0x40), (2, 0x1f), (8, 0x08)]);
        vdp.vram[10 * 128 + 3] = 0x9e;
        assert_eq!(vdp.native_pixel(6, 10), default_palette_color(9));
        assert_eq!(vdp.native_pixel(7, 10), default_palette_color(0x0e));
        let frame = vdp.frame();
        assert_eq!(frame.pixel(12, 10), default_palette_color(9));
        assert_eq!(frame.pixel(13, 10), default_palette_color(9));
        assert_eq!(frame.pixel(14, 10), default_palette_color(0x0e));
    }

    #[test]
    fn graphic4_colour_zero_shows_border_unless_tp() {
        let mut vdp = vdp_with(&[(0, 0x06), (1, 0x40), (2, 0x1f), (7, 0x05)]);
        assert_eq!(vdp.native_pixel(0, 0), default_palette_color(5));
        vdp.write_register(8, 0x20);
        assert_eq!(vdp.native_pixel(0, 0), default_palette_color(0));
    }

    #[test]
    fn graphic4_page_select_and_vertical_scroll() {
        let mut vdp = vdp_with(&[(0, 0x06), (1, 0x40), (2, 0x3f)]);
        vdp.vram[0x8000 + 5 * 128] = 0x30;
        assert_eq!(vdp.native_pixel(0, 5), default_palette_color(3));
        vdp.write_register(23, 5);
        assert_eq!(vdp.native_pixel(0, 0), default_palette_color(3));
    }

    #[test]
    fn graphic4_r13_alternates_to_the_even_page() {
        let mut vdp = vdp_with(&[(0, 0x06), (1, 0x40), (2, 0x3f), (13, 0x11)]);
        vdp.vram[0x8000] = 0x30;
        vdp.vram[0x0000] = 0x50;
        assert_eq!(vdp.native_pixel(0, 0), default_palette_color(3));
        vdp.blink_phase = true;
        assert_eq!(vdp.native_pixel(0, 0), default_palette_color(5));
    }

    #[test]
    fn graphic5_has_four_two_bit_pixels_per_byte() {
        let mut vdp = vdp_with(&[(0, 0x08), (1, 0x40), (2, 0x1f), (8, 0x20)]);
        vdp.vram[0] = 0b11_10_01_00;
        assert_eq!(vdp.native_pixel(0, 0), default_palette_color(3));
        assert_eq!(vdp.native_pixel(1, 0), default_palette_color(2));
        assert_eq!(vdp.native_pixel(2, 0), default_palette_color(1));
        assert_eq!(vdp.native_pixel(3, 0), default_palette_color(0));
    }

    #[test]
    fn graphic6_is_512_wide_four_bits_per_pixel() {
        let mut vdp = vdp_with(&[(0, 0x0a), (1, 0x40), (2, 0x1f)]);
        vdp.vram[256 + 255] = 0x7f;
        assert_eq!(vdp.native_pixel(510, 1), default_palette_color(7));
        assert_eq!(vdp.native_pixel(511, 1), default_palette_color(15));
        vdp.write_register(2, 0x3f);
        vdp.vram[0x10000] = 0x20;
        assert_eq!(vdp.native_pixel(0, 0), default_palette_color(2));
    }

    #[test]
    fn graphic7_pixels_are_grb332() {
        let mut vdp = vdp_with(&[(0, 0x0e), (1, 0x40), (2, 0x1f)]);
        vdp.vram[3] = 0b1110_0000; // G=7
        vdp.vram[4] = 0b0001_1111; // R=7 B=3
        assert_eq!(vdp.native_pixel(3, 0), 0x00ff00);
        assert_eq!(vdp.native_pixel(4, 0), 0xff00ff);
    }

    #[test]
    fn blanked_display_shows_only_border() {
        let mut vdp = vdp_with(&[(0, 0x06), (1, 0x00), (7, 0x06)]);
        vdp.vram[0] = 0xff;
        assert_eq!(vdp.native_pixel(0, 0), default_palette_color(6));
    }

    #[test]
    fn r18_adjust_shifts_the_picture() {
        let mut vdp = vdp_with(&[(0, 0x06), (1, 0x40), (2, 0x1f), (7, 0x01)]);
        vdp.vram[0] = 0xf0;
        assert_eq!(vdp.frame().pixel(0, 0), default_palette_color(15));
        vdp.write_register(18, 0x0f); // X = +1: picture moves right
        let frame = vdp.frame();
        assert_eq!(frame.pixel(0, 0), default_palette_color(1));
        assert_eq!(frame.pixel(2, 0), default_palette_color(15));
        vdp.write_register(18, 0xf0); // Y = +1: picture moves down
        let frame = vdp.frame();
        assert_eq!(frame.pixel(0, 0), default_palette_color(1));
        assert_eq!(frame.pixel(0, 1), default_palette_color(15));
    }

    #[test]
    fn lines_beyond_192_show_border_when_ln_is_clear() {
        let mut vdp = vdp_with(&[(0, 0x06), (1, 0x40), (2, 0x1f), (7, 0x01)]);
        vdp.vram[200 * 128] = 0xff;
        assert_eq!(vdp.frame().pixel(0, 200), default_palette_color(1));
        vdp.write_register(9, 0x80);
        assert_eq!(vdp.frame().pixel(0, 200), default_palette_color(15));
    }
}
