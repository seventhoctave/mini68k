//! Texas Instruments TMS9918A video display processor.
//!
//! Models everything software can observe: the two CPU ports (MODE = CPU
//! A1) including the address-latch quirk where the first control byte
//! already loads the low address bits, the read-ahead latch, 16 KiB VRAM
//! with 14-bit wrap, R0-R7, the status register (F, 5S, C and the fifth
//! sprite number), Graphics I/II, Multicolor and Text modes (including the
//! Graphics II R3/R4 table masks), the fixed palette, sprites (8x8/16x16,
//! magnification, early clock, 4 per line, Y=208 terminator, collision),
//! NTSC frame timing and the vblank interrupt.
//!
//! Not modelled: analog output, external video (R0 EXTVID), the 4K/16K
//! DRAM addressing difference (mini68k only uses 16K parts), and the CPU
//! access-window timing (real hardware drops VRAM accesses that come too
//! fast; the BIOS paces itself so this never matters for it).

use crate::v9938::{ScreenMode, VideoFrame};

pub const VRAM_SIZE: usize = 16 * 1024;
const VRAM_MASK: u16 = 0x3fff;

pub const PORT_DATA: u8 = 0x00;
pub const PORT_CONTROL: u8 = 0x02;

pub const MASTER_CLOCK_HZ: u64 = 10_738_635;
/// 342 pixel clocks at master / 2.
pub const MASTER_CYCLES_PER_LINE: u64 = 684;
pub const LINES_PER_FRAME: u32 = 262;
pub const ACTIVE_LINES: usize = 192;
pub const DEFAULT_CPU_HZ: u64 = 8_000_000;

pub const CANVAS_WIDTH: usize = 512;
pub const CANVAS_HEIGHT: usize = ACTIVE_LINES;

pub const STATUS_F: u8 = 0x80;
pub const STATUS_5S: u8 = 0x40;
pub const STATUS_C: u8 = 0x20;

/// The fixed TMS9918A colours. Code 0 is transparent (shows the backdrop).
pub const PALETTE: [u32; 16] = [
    0x000000, 0x000000, 0x21c842, 0x5edc78, 0x5455ed, 0x7d76fc, 0xd4524d, 0x42ebf5, 0xfc5554,
    0xff7978, 0xd4c154, 0xe6ce80, 0x21b03b, 0xc95bba, 0xcccccc, 0xffffff,
];

pub fn palette_color(index: u8) -> u32 {
    PALETTE[usize::from(index & 0x0f)]
}

#[derive(Debug, Clone)]
struct SpriteLine {
    colors: [Option<u8>; 256],
    overflow: Option<u8>,
    last_checked: u8,
    collision: bool,
}

#[derive(Debug, Clone)]
pub struct Tms9918a {
    regs: [u8; 8],
    status: u8,
    vram: Vec<u8>,
    latch: Option<u8>,
    address: u16,
    read_ahead: u8,
    cpu_hz: u64,
    /// Elapsed time in the current line, in master cycles * cpu_hz.
    line_clock: u64,
    line: u32,
    frame_count: u64,
}

impl Default for Tms9918a {
    fn default() -> Self {
        Self::new()
    }
}

impl Tms9918a {
    pub fn new() -> Self {
        Self::with_cpu_hz(DEFAULT_CPU_HZ)
    }

    pub fn with_cpu_hz(cpu_hz: u64) -> Self {
        let mut vdp = Self {
            regs: [0; 8],
            status: 0,
            vram: vec![0; VRAM_SIZE],
            latch: None,
            address: 0,
            read_ahead: 0,
            cpu_hz: cpu_hz.max(1),
            line_clock: 0,
            line: 0,
            frame_count: 0,
        };
        vdp.reset();
        vdp
    }

    pub fn reset(&mut self) {
        self.regs.fill(0);
        self.status = 0;
        self.vram.fill(0);
        self.latch = None;
        self.address = 0;
        self.read_ahead = 0;
        self.line_clock = 0;
        self.line = 0;
        self.frame_count = 0;
    }

    // ------------------------------------------------------------------
    // CPU ports (MODE is wired to CPU A1)

    pub fn read_port(&mut self, port: u8) -> u8 {
        self.latch = None;
        if port & 0x02 == 0 {
            let value = self.read_ahead;
            self.read_ahead = self.vram[usize::from(self.address)];
            self.address = (self.address + 1) & VRAM_MASK;
            value
        } else {
            let value = self.status;
            self.status &= !(STATUS_F | STATUS_5S | STATUS_C);
            value
        }
    }

    pub fn write_port(&mut self, port: u8, value: u8) {
        if port & 0x02 == 0 {
            self.latch = None;
            self.vram[usize::from(self.address)] = value;
            self.read_ahead = value;
            self.address = (self.address + 1) & VRAM_MASK;
            return;
        }

        let Some(first) = self.latch.take() else {
            // The first byte goes straight into the low address register.
            self.latch = Some(value);
            self.address = (self.address & 0x3f00) | u16::from(value);
            return;
        };

        if value & 0x80 != 0 {
            self.regs[usize::from(value & 0x07)] = first;
            return;
        }
        self.address = (u16::from(value & 0x3f) << 8) | u16::from(first);
        if value & 0x40 == 0 {
            self.read_ahead = self.vram[usize::from(self.address)];
            self.address = (self.address + 1) & VRAM_MASK;
        }
    }

    // ------------------------------------------------------------------
    // Accessors

    pub fn register(&self, register: usize) -> u8 {
        self.regs[register & 7]
    }

    pub fn peek_status(&self) -> u8 {
        self.status
    }

    pub fn vram(&self) -> &[u8] {
        &self.vram
    }

    pub fn vram_mut(&mut self) -> &mut [u8] {
        &mut self.vram
    }

    pub fn palette_rgb(&self, index: u8) -> u32 {
        palette_color(index)
    }

    pub fn current_line(&self) -> u32 {
        self.line
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    pub fn display_lines(&self) -> usize {
        ACTIVE_LINES
    }

    pub fn total_lines(&self) -> u32 {
        LINES_PER_FRAME
    }

    pub fn cpu_cycles_per_line(&self) -> f64 {
        (MASTER_CYCLES_PER_LINE * self.cpu_hz) as f64 / MASTER_CLOCK_HZ as f64
    }

    pub fn display_enabled(&self) -> bool {
        self.regs[1] & 0x40 != 0
    }

    /// /INT: frame flag with R1 IE set.
    pub fn irq_asserted(&self) -> bool {
        self.status & STATUS_F != 0 && self.regs[1] & 0x20 != 0
    }

    pub fn screen_mode(&self) -> ScreenMode {
        let m1 = self.regs[1] & 0x10 != 0;
        let m2 = self.regs[1] & 0x08 != 0;
        let m3 = self.regs[0] & 0x02 != 0;
        match (m1, m2, m3) {
            (false, false, false) => ScreenMode::Graphic1,
            (false, false, true) => ScreenMode::Graphic2,
            (false, true, false) => ScreenMode::Multicolor,
            (true, false, false) => ScreenMode::Text1,
            _ => ScreenMode::Undefined,
        }
    }

    // ------------------------------------------------------------------
    // Timing

    pub fn tick(&mut self, cpu_cycles: u64) {
        self.line_clock += cpu_cycles * MASTER_CLOCK_HZ;
        let per_line = MASTER_CYCLES_PER_LINE * self.cpu_hz;
        while self.line_clock >= per_line {
            self.line_clock -= per_line;
            self.end_line();
        }
    }

    fn end_line(&mut self) {
        if (self.line as usize) < ACTIVE_LINES {
            self.update_sprite_status(self.line as usize);
        }
        self.line += 1;
        if self.line as usize == ACTIVE_LINES {
            self.status |= STATUS_F;
            self.frame_count += 1;
        }
        if self.line >= LINES_PER_FRAME {
            self.line = 0;
        }
    }

    fn update_sprite_status(&mut self, y: usize) {
        let Some(line) = self.sprite_line(y) else {
            return;
        };
        if self.status & STATUS_5S == 0 {
            let number = line.overflow.unwrap_or(line.last_checked) & 0x1f;
            self.status = (self.status & (STATUS_F | STATUS_C)) | number;
            if line.overflow.is_some() {
                self.status |= STATUS_5S;
            }
        }
        if line.collision {
            self.status |= STATUS_C;
        }
    }

    // ------------------------------------------------------------------
    // Rendering

    fn vram_at(&self, address: u32) -> u8 {
        self.vram[(address & u32::from(VRAM_MASK)) as usize]
    }

    fn backdrop(&self) -> u32 {
        palette_color(self.regs[7] & 0x0f)
    }

    fn color(&self, code: u8) -> u32 {
        if code & 0x0f == 0 {
            self.backdrop()
        } else {
            palette_color(code)
        }
    }

    /// Render the current picture into a 512x192 canvas (pixels doubled
    /// horizontally; Text mode keeps its 8-pixel left border).
    pub fn frame(&self) -> VideoFrame {
        let mut pixels = vec![0u32; CANVAS_WIDTH * CANVAS_HEIGHT];
        let mut line = [0u32; CANVAS_WIDTH];
        for y in 0..CANVAS_HEIGHT {
            self.render_line(y, &mut line);
            pixels[y * CANVAS_WIDTH..(y + 1) * CANVAS_WIDTH].copy_from_slice(&line);
        }
        VideoFrame {
            width: CANVAS_WIDTH,
            height: CANVAS_HEIGHT,
            pixels_rgba: pixels,
            mode: self.screen_mode(),
            lines: ACTIVE_LINES,
        }
    }

    /// Colour of native pixel (x, y) of the current mode, sprites included.
    pub fn native_pixel(&self, x: usize, y: usize) -> u32 {
        let mut line = [0u32; CANVAS_WIDTH];
        self.render_line(y, &mut line);
        line[self.screen_mode().canvas_x(x)]
    }

    fn render_line(&self, y: usize, out: &mut [u32; CANVAS_WIDTH]) {
        out.fill(self.backdrop());
        if !self.display_enabled() {
            return;
        }
        let mode = self.screen_mode();
        let row = (y / 8) as u32;
        let line_in_char = (y & 7) as u32;
        let name_base = u32::from(self.regs[2] & 0x0f) << 10;
        let pattern_base = u32::from(self.regs[4] & 0x07) << 11;

        let mut put = |native_x: usize, code: u8| {
            let cx = mode.canvas_x(native_x);
            let rgb = self.color(code);
            out[cx] = rgb;
            out[cx + 1] = rgb;
        };

        match mode {
            ScreenMode::Text1 => {
                let fg = self.regs[7] >> 4;
                let bg = self.regs[7] & 0x0f;
                for column in 0..40u32 {
                    let character = u32::from(self.vram_at(name_base + row * 40 + column));
                    let pattern = self.vram_at(pattern_base + character * 8 + line_in_char);
                    for bit in 0..6usize {
                        let code = if pattern & (0x80 >> bit) != 0 { fg } else { bg };
                        put(column as usize * 6 + bit, code);
                    }
                }
            }
            ScreenMode::Graphic1 => {
                let color_base = u32::from(self.regs[3]) << 6;
                for column in 0..32u32 {
                    let character = u32::from(self.vram_at(name_base + row * 32 + column));
                    let pattern = self.vram_at(pattern_base + character * 8 + line_in_char);
                    let colors = self.vram_at(color_base + character / 8);
                    for bit in 0..8usize {
                        let code = if pattern & (0x80 >> bit) != 0 {
                            colors >> 4
                        } else {
                            colors & 0x0f
                        };
                        put(column as usize * 8 + bit, code);
                    }
                }
            }
            ScreenMode::Graphic2 => {
                let r3 = u32::from(self.regs[3]);
                let r4 = u32::from(self.regs[4]);
                let pattern_base = (r4 & 0x04) << 11;
                let pattern_mask = ((r4 & 0x03) << 11) | 0x7ff;
                let color_base = (r3 & 0x80) << 6;
                let color_mask = ((r3 & 0x7f) << 6) | 0x3f;
                let third = (row >> 3) & 0x03;
                for column in 0..32u32 {
                    let character = u32::from(self.vram_at(name_base + row * 32 + column));
                    let offset = (third * 256 + character) * 8 + line_in_char;
                    let pattern = self.vram_at(pattern_base | (offset & pattern_mask));
                    let colors = self.vram_at(color_base | (offset & color_mask));
                    for bit in 0..8usize {
                        let code = if pattern & (0x80 >> bit) != 0 {
                            colors >> 4
                        } else {
                            colors & 0x0f
                        };
                        put(column as usize * 8 + bit, code);
                    }
                }
            }
            ScreenMode::Multicolor => {
                for column in 0..32u32 {
                    let character = u32::from(self.vram_at(name_base + row * 32 + column));
                    let colors = self.vram_at(
                        pattern_base + character * 8 + (row & 3) * 2 + (line_in_char >> 2),
                    );
                    for x in 0..8usize {
                        let code = if x < 4 { colors >> 4 } else { colors & 0x0f };
                        put(column as usize * 8 + x, code);
                    }
                }
            }
            _ => return,
        }

        if let Some(sprites) = self.sprite_line(y) {
            for (x, color) in sprites.colors.iter().enumerate() {
                if let Some(color) = *color {
                    let rgb = palette_color(color);
                    out[x * 2] = rgb;
                    out[x * 2 + 1] = rgb;
                }
            }
        }
    }

    fn sprite_line(&self, y: usize) -> Option<SpriteLine> {
        if !self.display_enabled()
            || !matches!(
                self.screen_mode(),
                ScreenMode::Graphic1 | ScreenMode::Graphic2 | ScreenMode::Multicolor
            )
        {
            return None;
        }

        let size: u32 = if self.regs[1] & 0x02 != 0 { 16 } else { 8 };
        let magnify: u32 = if self.regs[1] & 0x01 != 0 { 2 } else { 1 };
        let attribute_base = u32::from(self.regs[5] & 0x7f) << 7;
        let pattern_base = u32::from(self.regs[6] & 0x07) << 11;
        let line = y as u32;

        let mut visible: Vec<(u8, u32)> = Vec::with_capacity(4);
        let mut overflow = None;
        let mut last_checked = 0u8;
        for number in 0..32u8 {
            last_checked = number;
            let sprite_y = self.vram_at(attribute_base + u32::from(number) * 4);
            if sprite_y == 208 {
                break;
            }
            let row = line.wrapping_sub(u32::from(sprite_y)).wrapping_sub(1) & 0xff;
            if row >= size * magnify {
                continue;
            }
            if visible.len() == 4 {
                overflow = Some(number);
                break;
            }
            visible.push((number, row / magnify));
        }

        let mut colors = [None; 256];
        let mut occupied = [false; 256];
        let mut collision = false;
        for (number, row) in visible {
            let entry = attribute_base + u32::from(number) * 4;
            let x = i32::from(self.vram_at(entry + 1));
            let mut pattern = u32::from(self.vram_at(entry + 2));
            if size == 16 {
                pattern &= 0xfc;
            }
            let attr = self.vram_at(entry + 3);
            let origin = x - if attr & 0x80 != 0 { 32 } else { 0 };
            let color = attr & 0x0f;
            let left = self.vram_at(pattern_base + pattern * 8 + row);
            let bits = if size == 16 {
                (u32::from(left) << 8)
                    | u32::from(self.vram_at(pattern_base + pattern * 8 + 16 + row))
            } else {
                u32::from(left) << 8
            };
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
                    // Collision counts every pattern bit, even transparent sprites.
                    if occupied[px] {
                        collision = true;
                    }
                    occupied[px] = true;
                    if color != 0 && colors[px].is_none() {
                        colors[px] = Some(color);
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_reg(vdp: &mut Tms9918a, reg: u8, value: u8) {
        vdp.write_port(PORT_CONTROL, value);
        vdp.write_port(PORT_CONTROL, 0x80 | reg);
    }

    fn with_regs(regs: &[(u8, u8)]) -> Tms9918a {
        let mut vdp = Tms9918a::new();
        for &(reg, value) in regs {
            set_reg(&mut vdp, reg, value);
        }
        vdp
    }

    fn cycles_for_lines(vdp: &Tms9918a, lines: u32) -> u64 {
        (vdp.cpu_cycles_per_line() * f64::from(lines)).ceil() as u64
    }

    #[test]
    fn register_write_uses_low_three_bits_of_register_number() {
        let mut vdp = Tms9918a::new();
        vdp.write_port(PORT_CONTROL, 0x5a);
        vdp.write_port(PORT_CONTROL, 0x8f); // R15 aliases R7
        assert_eq!(vdp.register(7), 0x5a);
    }

    #[test]
    fn data_writes_auto_increment_and_wrap_at_16k() {
        let mut vdp = Tms9918a::new();
        vdp.write_port(PORT_CONTROL, 0xff);
        vdp.write_port(PORT_CONTROL, 0x7f);
        vdp.write_port(PORT_DATA, 1);
        vdp.write_port(PORT_DATA, 2);
        assert_eq!(vdp.vram[0x3fff], 1);
        assert_eq!(vdp.vram[0], 2);
    }

    #[test]
    fn first_control_byte_loads_the_low_address_immediately() {
        let mut vdp = Tms9918a::new();
        vdp.write_port(PORT_CONTROL, 0x00);
        vdp.write_port(PORT_CONTROL, 0x41); // write at 0x0100
        vdp.write_port(PORT_CONTROL, 0x20); // half a command...
        let _ = vdp.read_port(PORT_CONTROL); // ...abandoned by a status read
        vdp.write_port(PORT_DATA, 0xaa);
        assert_eq!(vdp.vram[0x0120], 0xaa);
    }

    #[test]
    fn read_setup_prefetches() {
        let mut vdp = Tms9918a::new();
        vdp.vram[0x200] = 7;
        vdp.vram[0x201] = 8;
        vdp.write_port(PORT_CONTROL, 0x00);
        vdp.write_port(PORT_CONTROL, 0x02);
        vdp.vram[0x200] = 0;
        assert_eq!(vdp.read_port(PORT_DATA), 7);
        assert_eq!(vdp.read_port(PORT_DATA), 8);
    }

    #[test]
    fn status_read_clears_flags_and_the_latch() {
        let mut vdp = Tms9918a::new();
        vdp.status = STATUS_F | STATUS_C | 0x03;
        vdp.write_port(PORT_CONTROL, 0x12);
        assert_eq!(vdp.read_port(PORT_CONTROL), STATUS_F | STATUS_C | 0x03);
        assert_eq!(vdp.peek_status(), 0x03);
        set_reg(&mut vdp, 7, 0x34);
        assert_eq!(vdp.register(7), 0x34);
    }

    #[test]
    fn frame_flag_at_line_192_and_interrupt_with_ie() {
        let mut vdp = Tms9918a::new();
        vdp.tick(cycles_for_lines(&vdp, 191));
        assert_eq!(vdp.peek_status() & STATUS_F, 0);
        vdp.tick(cycles_for_lines(&vdp, 1));
        assert_ne!(vdp.peek_status() & STATUS_F, 0);
        assert!(!vdp.irq_asserted());
        set_reg(&mut vdp, 1, 0x20);
        assert!(vdp.irq_asserted());
        let _ = vdp.read_port(PORT_CONTROL);
        assert!(!vdp.irq_asserted());
    }

    #[test]
    fn frame_is_262_lines() {
        let mut vdp = Tms9918a::new();
        vdp.tick(cycles_for_lines(&vdp, 262));
        assert_eq!(vdp.current_line(), 0);
        assert_eq!(vdp.frame_count(), 1);
    }

    #[test]
    fn mode_bits_decode() {
        for (r0, r1, mode) in [
            (0x00, 0x40, ScreenMode::Graphic1),
            (0x02, 0x40, ScreenMode::Graphic2),
            (0x00, 0x48, ScreenMode::Multicolor),
            (0x00, 0x50, ScreenMode::Text1),
            (0x02, 0x50, ScreenMode::Undefined),
        ] {
            assert_eq!(with_regs(&[(0, r0), (1, r1)]).screen_mode(), mode);
        }
    }

    #[test]
    fn text_mode_renders_40_columns_of_6_pixels() {
        let mut vdp = with_regs(&[(1, 0xd0), (2, 0x00), (4, 0x01), (7, 0xf4)]);
        vdp.vram[1] = b'A';
        vdp.vram[0x800 + usize::from(b'A') * 8] = 0x84;
        assert_eq!(vdp.native_pixel(6, 0), palette_color(15));
        assert_eq!(vdp.native_pixel(7, 0), palette_color(4));
        assert_eq!(vdp.native_pixel(11, 0), palette_color(15));
        let frame = vdp.frame();
        assert_eq!((frame.width, frame.height), (512, 192));
        assert_eq!(frame.pixel((8 + 6) * 2, 0), palette_color(15));
        assert_eq!(frame.pixel(0, 0), palette_color(4)); // left border
    }

    #[test]
    fn graphic1_colour_per_eight_patterns_and_transparent_backdrop() {
        let mut vdp = with_regs(&[(1, 0xc0), (2, 0x06), (3, 0x80), (4, 0x00), (7, 0x05)]);
        vdp.vram[0x1800] = 9;
        vdp.vram[9 * 8] = 0x80;
        vdp.vram[0x2000 + 1] = 0x60;
        assert_eq!(vdp.native_pixel(0, 0), palette_color(6));
        assert_eq!(vdp.native_pixel(1, 0), palette_color(5)); // colour 0 -> backdrop
    }

    #[test]
    fn graphic2_bitmap_and_half_bitmap_masks() {
        let mut vdp = with_regs(&[
            (0, 0x02),
            (1, 0xc0),
            (2, 0x0e),
            (3, 0xff),
            (4, 0x03),
            (7, 0x01),
        ]);
        // name table 0x3800 holds 0..255 per third
        for i in 0..768usize {
            vdp.vram[0x3800 + i] = i as u8;
        }
        // pixel (9, 70): row 8 is the first row of third 1 -> name 1, line 6
        let offset = (256 + 1) * 8 + 6;
        vdp.vram[offset] = 0x40;
        vdp.vram[0x2000 + offset] = 0xa0;
        assert_eq!(vdp.native_pixel(9, 70), palette_color(0x0a));
        assert_eq!(vdp.native_pixel(8, 70), palette_color(1));

        // R4 = 0x00 masks the third bits: third 1 reads third 0's patterns
        set_reg(&mut vdp, 4, 0x00);
        vdp.vram[8 + 6] = 0x40;
        assert_eq!(vdp.native_pixel(9, 70), palette_color(0x0a));
    }

    #[test]
    fn multicolor_blocks() {
        let mut vdp = with_regs(&[(1, 0xc8), (2, 0x02), (4, 0x00)]);
        vdp.vram[0x800] = 3;
        vdp.vram[3 * 8] = 0x5a;
        vdp.vram[3 * 8 + 1] = 0x7c;
        assert_eq!(vdp.native_pixel(0, 0), palette_color(5));
        assert_eq!(vdp.native_pixel(4, 3), palette_color(0x0a));
        assert_eq!(vdp.native_pixel(0, 4), palette_color(7));
    }

    #[test]
    fn blanked_display_shows_backdrop() {
        let mut vdp = with_regs(&[(1, 0x80), (7, 0x06)]);
        vdp.vram[0] = 1;
        assert_eq!(vdp.native_pixel(0, 0), palette_color(6));
    }

    fn sprite_vdp(r1: u8) -> Tms9918a {
        // Graphics I, attributes 0x1b00, sprite patterns 0x3800
        let mut vdp = with_regs(&[(1, r1), (2, 0x06), (5, 0x36), (6, 0x07), (7, 0x01)]);
        vdp.vram[0x1b00 + 8 * 4] = 208;
        vdp
    }

    fn place(vdp: &mut Tms9918a, n: usize, x: u8, y: u8, pattern: u8, attr: u8) {
        let entry = 0x1b00 + n * 4;
        vdp.vram[entry] = y;
        vdp.vram[entry + 1] = x;
        vdp.vram[entry + 2] = pattern;
        vdp.vram[entry + 3] = attr;
    }

    #[test]
    fn sprites_draw_one_line_below_y_with_priority_and_early_clock() {
        let mut vdp = sprite_vdp(0xc0);
        for byte in 0..8 {
            vdp.vram[0x3800 + byte] = 0xff;
        }
        for n in 0..8 {
            place(&mut vdp, n, 200, 200, 0, 0);
        }
        place(&mut vdp, 0, 10, 9, 0, 8);
        place(&mut vdp, 1, 14, 9, 0, 3);
        place(&mut vdp, 2, 40, 9, 0, 0x80 | 7);
        assert_eq!(vdp.native_pixel(10, 9), palette_color(1));
        assert_eq!(vdp.native_pixel(10, 10), palette_color(8));
        assert_eq!(vdp.native_pixel(15, 10), palette_color(8));
        assert_eq!(vdp.native_pixel(19, 10), palette_color(3));
        assert_eq!(vdp.native_pixel(8, 10), palette_color(7));
    }

    #[test]
    fn sixteen_by_sixteen_and_magnified_sprites() {
        let mut vdp = sprite_vdp(0xc3);
        vdp.vram[0x3800 + 4 * 8 + 16] = 0x01; // pattern 4, top-right, row 0
        place(&mut vdp, 0, 0, 255, 6, 2);
        assert_eq!(vdp.native_pixel(30, 0), palette_color(2));
        assert_eq!(vdp.native_pixel(31, 1), palette_color(2));
        assert_eq!(vdp.native_pixel(29, 0), palette_color(1));
    }

    #[test]
    fn fifth_sprite_flag_and_terminator() {
        let mut vdp = sprite_vdp(0xc0);
        for byte in 0..8 {
            vdp.vram[0x3800 + byte] = 0xff;
        }
        for n in 0..6 {
            place(&mut vdp, n, (n * 20) as u8, 0, 0, 4);
        }
        place(&mut vdp, 6, 0, 208, 0, 0);
        assert_eq!(vdp.native_pixel(60, 1), palette_color(4));
        assert_eq!(vdp.native_pixel(80, 1), palette_color(1));
        vdp.update_sprite_status(1);
        assert_eq!(vdp.peek_status() & (STATUS_5S | 0x1f), STATUS_5S | 4);

        let mut vdp = sprite_vdp(0xc0);
        place(&mut vdp, 0, 0, 100, 0, 4);
        place(&mut vdp, 1, 0, 208, 0, 4);
        vdp.update_sprite_status(1);
        assert_eq!(vdp.peek_status() & (STATUS_5S | 0x1f), 1);
    }

    #[test]
    fn collision_includes_transparent_sprites() {
        let mut vdp = sprite_vdp(0xc0);
        vdp.vram[0x3800] = 0xff;
        place(&mut vdp, 0, 0, 0, 0, 0);
        place(&mut vdp, 1, 4, 0, 0, 0);
        place(&mut vdp, 2, 0, 208, 0, 0);
        vdp.update_sprite_status(1);
        assert_ne!(vdp.peek_status() & STATUS_C, 0);
    }

    #[test]
    fn no_sprites_in_text_mode() {
        let mut vdp = sprite_vdp(0xd0);
        vdp.vram[0x3800] = 0xff;
        place(&mut vdp, 0, 0, 0, 0, 4);
        assert!(vdp.sprite_line(1).is_none());
    }
}
