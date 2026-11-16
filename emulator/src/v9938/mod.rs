//! Yamaha V9938 video display processor.
//!
//! Models everything software can observe on the real chip: the four CPU
//! ports, control/status registers, 128 KiB VRAM with the read-ahead latch,
//! the palette, every screen mode, sprite modes 1 and 2 (including overflow
//! and collision status), the command engine, vertical/horizontal blanking
//! and line timing, and the vblank/line interrupt output.
//!
//! Not modelled: analog output, light pen/mouse, superimpose/digitize, and
//! command-engine cycle timing (VRAM-only commands complete immediately).

mod command;
mod render;
mod sprites;

pub use render::{ScreenMode, VideoFrame, CANVAS_HEIGHT, CANVAS_WIDTH};

pub const VRAM_SIZE: usize = 128 * 1024;
const VRAM_MASK: u32 = 0x1_ffff;
const REGISTER_COUNT: usize = 64;
const STATUS_COUNT: usize = 10;
/// R0..R23 and R32..R46 exist; everything above R46 is ignored.
const LAST_REGISTER: usize = 46;

pub const PORT_DATA: u8 = 0x00;
pub const PORT_COMMAND: u8 = 0x02;
pub const PORT_PALETTE: u8 = 0x04;
pub const PORT_INDIRECT: u8 = 0x06;

pub const MASTER_CLOCK_HZ: u64 = 21_477_270;
pub const MASTER_CYCLES_PER_LINE: u64 = 1368;
pub const NTSC_LINES: u32 = 262;
pub const PAL_LINES: u32 = 313;
/// Master cycles into a line where horizontal blanking (S#2 HR) begins.
const HBLANK_START: u64 = 1024;
pub const DEFAULT_CPU_HZ: u64 = 8_000_000;
/// R13 blink/alternate periods are counted in units of 10 frames.
const BLINK_UNIT_FRAMES: u32 = 10;

pub(crate) const S0_F: u8 = 0x80;
pub(crate) const S0_5S: u8 = 0x40;
pub(crate) const S0_C: u8 = 0x20;
pub(crate) const S1_FH: u8 = 0x01;
pub(crate) const S2_TR: u8 = 0x80;
pub(crate) const S2_VR: u8 = 0x40;
pub(crate) const S2_HR: u8 = 0x20;
pub(crate) const S2_BD: u8 = 0x10;
const S2_FIXED: u8 = 0x0c;
pub(crate) const S2_EO: u8 = 0x02;
pub(crate) const S2_CE: u8 = 0x01;

/// MSX2 power-on palette, (R, G, B) 0..7.
pub const DEFAULT_PALETTE: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (0, 0, 0),
    (1, 6, 1),
    (3, 7, 3),
    (1, 1, 7),
    (2, 3, 7),
    (5, 1, 1),
    (2, 6, 7),
    (7, 1, 1),
    (7, 3, 3),
    (6, 6, 1),
    (6, 6, 4),
    (1, 4, 1),
    (6, 2, 5),
    (5, 5, 5),
    (7, 7, 7),
];

/// Expand a 3-bit-per-channel colour to 0x00RRGGBB.
pub fn rgb9(r: u8, g: u8, b: u8) -> u32 {
    let expand = |v: u8| u32::from(v & 7) * 255 / 7;
    (expand(r) << 16) | (expand(g) << 8) | expand(b)
}

/// RGB of an entry in the power-on palette.
pub fn default_palette_color(index: u8) -> u32 {
    let (r, g, b) = DEFAULT_PALETTE[usize::from(index & 0x0f)];
    rgb9(r, g, b)
}

#[derive(Debug, Clone)]
pub struct V9938 {
    regs: [u8; REGISTER_COUNT],
    status: [u8; STATUS_COUNT],
    vram: Vec<u8>,
    latch: Option<u8>,
    address: u32,
    read_ahead: u8,
    palette: [(u8, u8, u8); 16],
    palette_latch: Option<u8>,
    cmd: command::CommandState,
    cpu_hz: u64,
    /// Elapsed time within the current line, in units of master cycles * cpu_hz.
    line_clock: u64,
    line: u32,
    frame_count: u64,
    /// True while R13's "on" period is running: TEXT2 blink colours are
    /// shown and G4-G7 alternate-page display shows the even page.
    blink_phase: bool,
    blink_counter: u32,
}

impl Default for V9938 {
    fn default() -> Self {
        Self::new()
    }
}

impl V9938 {
    pub fn new() -> Self {
        Self::with_cpu_hz(DEFAULT_CPU_HZ)
    }

    pub fn with_cpu_hz(cpu_hz: u64) -> Self {
        let mut vdp = Self {
            regs: [0; REGISTER_COUNT],
            status: [0; STATUS_COUNT],
            vram: vec![0; VRAM_SIZE],
            latch: None,
            address: 0,
            read_ahead: 0,
            palette: DEFAULT_PALETTE,
            palette_latch: None,
            cmd: command::CommandState::default(),
            cpu_hz: cpu_hz.max(1),
            line_clock: 0,
            line: 0,
            frame_count: 0,
            blink_phase: false,
            blink_counter: 0,
        };
        vdp.reset();
        vdp
    }

    pub fn reset(&mut self) {
        self.regs.fill(0);
        self.status.fill(0);
        self.vram.fill(0);
        self.latch = None;
        self.address = 0;
        self.read_ahead = 0;
        self.palette = DEFAULT_PALETTE;
        self.palette_latch = None;
        self.cmd = command::CommandState::default();
        self.line_clock = 0;
        self.line = 0;
        self.frame_count = 0;
        self.blink_phase = false;
        self.blink_counter = 0;
    }

    // ------------------------------------------------------------------
    // CPU ports

    pub fn read_port(&mut self, port: u8) -> u8 {
        match port & 0x06 {
            PORT_DATA => {
                self.latch = None;
                let value = self.read_ahead;
                self.read_ahead = self.vram[self.address as usize];
                self.increment_address();
                value
            }
            PORT_COMMAND => {
                self.latch = None;
                self.read_status()
            }
            _ => 0xff,
        }
    }

    pub fn write_port(&mut self, port: u8, value: u8) {
        match port & 0x06 {
            PORT_DATA => {
                self.latch = None;
                self.vram[self.address as usize] = value;
                self.read_ahead = value;
                self.increment_address();
            }
            PORT_COMMAND => self.write_control(value),
            PORT_PALETTE => {
                if let Some(first) = self.palette_latch.take() {
                    let index = usize::from(self.regs[16] & 0x0f);
                    self.palette[index] = ((first >> 4) & 7, value & 7, first & 7);
                    self.regs[16] = self.regs[16].wrapping_add(1) & 0x0f;
                } else {
                    self.palette_latch = Some(value);
                }
            }
            _ => {
                let target = self.regs[17] & 0x3f;
                if target != 17 {
                    self.write_register(target, value);
                }
                if self.regs[17] & 0x80 == 0 {
                    self.regs[17] = self.regs[17].wrapping_add(1) & 0x3f;
                }
            }
        }
    }

    fn write_control(&mut self, value: u8) {
        let Some(first) = self.latch.take() else {
            self.latch = Some(value);
            return;
        };

        if value & 0x80 != 0 {
            self.write_register(value & 0x3f, first);
            return;
        }

        self.address = (u32::from(self.regs[14] & 0x07) << 14)
            | (u32::from(value & 0x3f) << 8)
            | u32::from(first);
        if value & 0x40 == 0 {
            self.read_ahead = self.vram[self.address as usize];
            self.increment_address();
        }
    }

    fn increment_address(&mut self) {
        let next = self.address + 1;
        if self.screen_mode().is_bitmap() {
            self.address = next & VRAM_MASK;
            self.regs[14] = ((self.address >> 14) & 0x07) as u8;
        } else {
            self.address = (self.address & !0x3fff) | (next & 0x3fff);
        }
    }

    /// Write a control register exactly as a port 1 or port 3 write would.
    pub fn write_register(&mut self, register: u8, value: u8) {
        let register = usize::from(register & 0x3f);
        if register > LAST_REGISTER {
            return;
        }
        self.regs[register] = value;
        match register {
            14 => {
                self.address = (self.address & 0x3fff) | (u32::from(value & 0x07) << 14);
            }
            16 => self.palette_latch = None,
            44 => self.command_clr_written(),
            46 => self.command_start(value),
            _ => {}
        }
    }

    fn read_status(&mut self) -> u8 {
        let index = usize::from(self.regs[15] & 0x0f);
        let value = self.peek_status(index);
        match index {
            0 => self.status[0] &= !(S0_F | S0_5S | S0_C),
            1 => self.status[1] &= !S1_FH,
            5 => {
                self.status[3] = 0;
                self.status[4] = 0;
                self.status[5] = 0;
                self.status[6] = 0;
            }
            7 => self.command_s7_read(),
            _ => {}
        }
        value
    }

    /// Status register value without read side effects.
    pub fn peek_status(&self, index: usize) -> u8 {
        match index {
            0 | 3 | 5 | 7 | 8 => self.status[index],
            1 => self.status[1] & 0xc1,
            2 => {
                let mut value = (self.status[2] & (S2_TR | S2_BD | S2_EO | S2_CE)) | S2_FIXED;
                if self.line >= self.display_lines() as u32 {
                    value |= S2_VR;
                }
                if self.line_clock / self.cpu_hz >= HBLANK_START {
                    value |= S2_HR;
                }
                value
            }
            4 | 9 => 0xfe | (self.status[index] & 0x01),
            6 => 0xfc | (self.status[6] & 0x03),
            _ => 0xff,
        }
    }

    // ------------------------------------------------------------------
    // Accessors

    pub fn register(&self, register: usize) -> u8 {
        self.regs[register & 0x3f]
    }

    pub fn vram(&self) -> &[u8] {
        &self.vram
    }

    pub fn vram_mut(&mut self) -> &mut [u8] {
        &mut self.vram
    }

    pub fn palette_rgb(&self, index: u8) -> u32 {
        let (r, g, b) = self.palette[usize::from(index & 0x0f)];
        rgb9(r, g, b)
    }

    pub fn vram_address(&self) -> u32 {
        self.address
    }

    pub fn current_line(&self) -> u32 {
        self.line
    }

    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    pub fn cpu_hz(&self) -> u64 {
        self.cpu_hz
    }

    pub fn set_cpu_hz(&mut self, cpu_hz: u64) {
        self.cpu_hz = cpu_hz.max(1);
        self.line_clock = 0;
    }

    pub fn display_lines(&self) -> usize {
        if self.regs[9] & 0x80 != 0 {
            212
        } else {
            192
        }
    }

    pub fn total_lines(&self) -> u32 {
        if self.regs[9] & 0x02 != 0 {
            PAL_LINES
        } else {
            NTSC_LINES
        }
    }

    /// CPU cycles per scanline at the configured CPU clock.
    pub fn cpu_cycles_per_line(&self) -> f64 {
        (MASTER_CYCLES_PER_LINE * self.cpu_hz) as f64 / MASTER_CLOCK_HZ as f64
    }

    /// State of the /INT output: (F and IE0) or (FH and IE1).
    pub fn irq_asserted(&self) -> bool {
        let vblank = self.status[0] & S0_F != 0 && self.regs[1] & 0x20 != 0;
        let line = self.status[1] & S1_FH != 0 && self.regs[0] & 0x10 != 0;
        vblank || line
    }

    // ------------------------------------------------------------------
    // Timing

    /// Advance video timing by `cpu_cycles` CPU clocks.
    pub fn tick(&mut self, cpu_cycles: u64) {
        self.line_clock += cpu_cycles * MASTER_CLOCK_HZ;
        let per_line = MASTER_CYCLES_PER_LINE * self.cpu_hz;
        while self.line_clock >= per_line {
            self.line_clock -= per_line;
            self.end_line();
        }
    }

    fn end_line(&mut self) {
        let display = self.display_lines() as u32;
        if self.line < display {
            self.update_sprite_status(self.line as usize);
        }

        self.line += 1;
        if self.line == display {
            self.status[0] |= S0_F;
            self.end_frame();
        }
        if self.line >= self.total_lines() {
            self.line = 0;
            self.status[2] ^= S2_EO;
        }

        if self.line < display {
            let scrolled = (self.line + u32::from(self.regs[23])) & 0xff;
            if scrolled == u32::from(self.regs[19]) {
                self.status[1] |= S1_FH;
            }
        }
    }

    fn end_frame(&mut self) {
        self.frame_count += 1;
        let on = u32::from(self.regs[13] >> 4);
        let off = u32::from(self.regs[13] & 0x0f);
        if on == 0 {
            self.blink_phase = false;
            self.blink_counter = 0;
            return;
        }
        if off == 0 {
            self.blink_phase = true;
            self.blink_counter = 0;
            return;
        }
        self.blink_counter += 1;
        let period = if self.blink_phase { on } else { off } * BLINK_UNIT_FRAMES;
        if self.blink_counter >= period {
            self.blink_phase = !self.blink_phase;
            self.blink_counter = 0;
        }
    }

    fn update_sprite_status(&mut self, y: usize) {
        let Some(line) = self.sprite_line(y) else {
            return;
        };
        if self.status[0] & S0_5S == 0 {
            let number = line.overflow.unwrap_or(line.last_checked) & 0x1f;
            self.status[0] = (self.status[0] & (S0_F | S0_C)) | number;
            if line.overflow.is_some() {
                self.status[0] |= S0_5S;
            }
        }
        if let Some((x, sy)) = line.collision {
            if self.status[0] & S0_C == 0 {
                self.status[0] |= S0_C;
                let x = x + 12;
                let sy = sy + 8;
                self.status[3] = (x & 0xff) as u8;
                self.status[4] = ((x >> 8) & 0x01) as u8;
                self.status[5] = (sy & 0xff) as u8;
                self.status[6] = ((sy >> 8) & 0x03) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_reg(vdp: &mut V9938, reg: u8, value: u8) {
        vdp.write_port(PORT_COMMAND, value);
        vdp.write_port(PORT_COMMAND, 0x80 | reg);
    }

    fn read_status(vdp: &mut V9938, index: u8) -> u8 {
        set_reg(vdp, 15, index);
        vdp.read_port(PORT_COMMAND)
    }

    #[test]
    fn register_writes_use_the_command_latch() {
        let mut vdp = V9938::new();
        set_reg(&mut vdp, 7, 0x12);
        assert_eq!(vdp.register(7), 0x12);
    }

    #[test]
    fn status_read_resets_the_latch() {
        let mut vdp = V9938::new();
        vdp.write_port(PORT_COMMAND, 0x55); // half a register write
        let _ = vdp.read_port(PORT_COMMAND);
        set_reg(&mut vdp, 7, 0xf4);
        assert_eq!(vdp.register(7), 0xf4);
    }

    #[test]
    fn data_port_writes_auto_increment_vram() {
        let mut vdp = V9938::new();
        vdp.write_port(PORT_COMMAND, 0x00);
        vdp.write_port(PORT_COMMAND, 0x40);
        vdp.write_port(PORT_DATA, 0xaa);
        vdp.write_port(PORT_DATA, 0xbb);
        assert_eq!(vdp.vram[0], 0xaa);
        assert_eq!(vdp.vram[1], 0xbb);
    }

    #[test]
    fn read_setup_prefetches_into_the_read_ahead_latch() {
        let mut vdp = V9938::new();
        vdp.vram[0x100] = 1;
        vdp.vram[0x101] = 2;
        vdp.write_port(PORT_COMMAND, 0x00);
        vdp.write_port(PORT_COMMAND, 0x01); // read setup at 0x0100
        vdp.vram[0x100] = 9; // already latched
        assert_eq!(vdp.read_port(PORT_DATA), 1);
        assert_eq!(vdp.read_port(PORT_DATA), 2);
    }

    #[test]
    fn palette_port_writes_pairs_at_r16_and_increments() {
        let mut vdp = V9938::new();
        set_reg(&mut vdp, 16, 3);
        vdp.write_port(PORT_PALETTE, 0x52); // R=5 B=2
        vdp.write_port(PORT_PALETTE, 0x06); // G=6
        vdp.write_port(PORT_PALETTE, 0x70);
        vdp.write_port(PORT_PALETTE, 0x00);
        assert_eq!(vdp.palette[3], (5, 6, 2));
        assert_eq!(vdp.palette[4], (7, 0, 0));
        assert_eq!(vdp.register(16), 5);
        assert_eq!(vdp.palette_rgb(3), rgb9(5, 6, 2));
    }

    #[test]
    fn auto_increment_pointers_wrap_without_overflow() {
        let mut vdp = V9938::new();
        set_reg(&mut vdp, 16, 0xff);
        vdp.write_port(PORT_PALETTE, 0x77);
        vdp.write_port(PORT_PALETTE, 0x07);
        assert_eq!(vdp.palette[15], (7, 7, 7));
        assert_eq!(vdp.register(16), 0);
        set_reg(&mut vdp, 17, 0x7f);
        vdp.write_port(PORT_INDIRECT, 0x00);
        assert_eq!(vdp.register(17), 0);
    }

    #[test]
    fn indirect_port_auto_increments_unless_aii() {
        let mut vdp = V9938::new();
        set_reg(&mut vdp, 17, 36);
        vdp.write_port(PORT_INDIRECT, 0x11);
        vdp.write_port(PORT_INDIRECT, 0x22);
        assert_eq!(vdp.register(36), 0x11);
        assert_eq!(vdp.register(37), 0x22);
        assert_eq!(vdp.register(17), 38);

        set_reg(&mut vdp, 17, 0x80 | 7);
        vdp.write_port(PORT_INDIRECT, 0x33);
        vdp.write_port(PORT_INDIRECT, 0x44);
        assert_eq!(vdp.register(7), 0x44);
        assert_eq!(vdp.register(8), 0);
    }

    #[test]
    fn r14_supplies_high_address_bits_and_carries_in_bitmap_modes() {
        let mut vdp = V9938::new();
        set_reg(&mut vdp, 0, 0x06); // G4
        set_reg(&mut vdp, 14, 0x01);
        vdp.write_port(PORT_COMMAND, 0xff);
        vdp.write_port(PORT_COMMAND, 0x7f); // write at 0x7fff
        vdp.write_port(PORT_DATA, 1);
        vdp.write_port(PORT_DATA, 2);
        assert_eq!(vdp.vram[0x7fff], 1);
        assert_eq!(vdp.vram[0x8000], 2);
        assert_eq!(vdp.register(14), 2);
    }

    #[test]
    fn address_wraps_within_16k_in_character_modes() {
        let mut vdp = V9938::new();
        set_reg(&mut vdp, 0, 0x00); // G1
        set_reg(&mut vdp, 14, 0x01);
        vdp.write_port(PORT_COMMAND, 0xff);
        vdp.write_port(PORT_COMMAND, 0x7f);
        vdp.write_port(PORT_DATA, 1);
        vdp.write_port(PORT_DATA, 2);
        assert_eq!(vdp.vram[0x7fff], 1);
        assert_eq!(vdp.vram[0x4000], 2);
        assert_eq!(vdp.register(14), 1);
    }

    #[test]
    fn status_two_has_fixed_bits_and_reports_idle_engine() {
        let mut vdp = V9938::new();
        let s2 = read_status(&mut vdp, 2);
        assert_eq!(s2 & 0x0c, 0x0c);
        assert_eq!(s2 & S2_CE, 0);
    }

    #[test]
    fn status_one_reports_v9938_id() {
        let mut vdp = V9938::new();
        assert_eq!(read_status(&mut vdp, 1) & 0x3e, 0);
    }

    fn cycles_for_lines(vdp: &V9938, lines: u32) -> u64 {
        (vdp.cpu_cycles_per_line() * f64::from(lines)).ceil() as u64
    }

    #[test]
    fn vblank_sets_f_and_vr_and_reading_s0_clears_f() {
        let mut vdp = V9938::new();
        let cycles = cycles_for_lines(&vdp, 192);
        vdp.tick(cycles);
        assert_eq!(vdp.current_line(), 192);
        assert_ne!(vdp.peek_status(0) & S0_F, 0);
        assert_ne!(vdp.peek_status(2) & S2_VR, 0);
        assert_ne!(read_status(&mut vdp, 0) & S0_F, 0);
        assert_eq!(read_status(&mut vdp, 0) & S0_F, 0);
    }

    #[test]
    fn frame_has_262_ntsc_or_313_pal_lines() {
        let mut vdp = V9938::new();
        vdp.tick(cycles_for_lines(&vdp, 262));
        assert_eq!(vdp.current_line(), 0);
        assert_eq!(vdp.frame_count(), 1);

        let mut pal = V9938::new();
        set_reg(&mut pal, 9, 0x02);
        pal.tick(cycles_for_lines(&pal, 262));
        assert_eq!(pal.current_line(), 262);
    }

    #[test]
    fn hr_is_set_during_horizontal_blanking() {
        let mut vdp = V9938::new();
        let into_line = |master: u64| master * DEFAULT_CPU_HZ / MASTER_CLOCK_HZ;
        vdp.tick(into_line(100));
        assert_eq!(vdp.peek_status(2) & S2_HR, 0);
        vdp.tick(into_line(1000));
        assert_ne!(vdp.peek_status(2) & S2_HR, 0);
    }

    #[test]
    fn line_interrupt_matches_r19_offset_by_r23() {
        let mut vdp = V9938::new();
        set_reg(&mut vdp, 19, 50);
        set_reg(&mut vdp, 23, 10);
        set_reg(&mut vdp, 0, 0x10); // IE1
        vdp.tick(cycles_for_lines(&vdp, 39));
        assert!(!vdp.irq_asserted());
        vdp.tick(cycles_for_lines(&vdp, 1));
        assert_eq!(vdp.current_line(), 40);
        assert!(vdp.irq_asserted());
        assert_ne!(read_status(&mut vdp, 1) & S1_FH, 0);
        assert!(!vdp.irq_asserted());
    }

    #[test]
    fn vblank_interrupt_requires_ie0_and_clears_on_s0_read() {
        let mut vdp = V9938::new();
        vdp.tick(cycles_for_lines(&vdp, 192));
        assert!(!vdp.irq_asserted());
        set_reg(&mut vdp, 1, 0x20);
        assert!(vdp.irq_asserted());
        let _ = read_status(&mut vdp, 0);
        assert!(!vdp.irq_asserted());
    }

    #[test]
    fn r13_alternates_blink_phase_in_ten_frame_units() {
        let mut vdp = V9938::new();
        set_reg(&mut vdp, 13, 0x12); // on 1 unit, off 2 units
        let frame = cycles_for_lines(&vdp, 262);
        for _ in 0..20 {
            vdp.tick(frame);
        }
        assert!(vdp.blink_phase);
        for _ in 0..10 {
            vdp.tick(frame);
        }
        assert!(!vdp.blink_phase);
    }

    #[test]
    fn s5_read_clears_collision_coordinates() {
        let mut vdp = V9938::new();
        vdp.status[3] = 0x12;
        vdp.status[5] = 0x34;
        assert_eq!(read_status(&mut vdp, 3), 0x12);
        assert_eq!(read_status(&mut vdp, 5), 0x34);
        assert_eq!(read_status(&mut vdp, 3), 0);
    }
}
