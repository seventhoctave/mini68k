//! V9938 command engine: all twelve commands (STOP, POINT, PSET, SRCH,
//! LINE, LMMV, LMMM, LMCM, LMMC, HMMV, HMMM, YMMM, HMMC) with logical
//! operations, operating in GRAPHIC4..GRAPHIC7 as on the real chip.
//!
//! VRAM-only commands complete as soon as R46 is written. The CPU transfer
//! commands (LMMC, HMMC, LMCM) follow the real TR/CE handshake: CE stays
//! set until the last byte has passed through R44 / S#7.
//!
//! There is no expansion RAM on mini68k: MXD writes are discarded and MXS
//! reads return 0.

use super::{ScreenMode, S2_BD, S2_CE, S2_TR, V9938, VRAM_MASK};

const ARG_MAJ: u8 = 0x01;
const ARG_EQ: u8 = 0x02;
const ARG_DIX: u8 = 0x04;
const ARG_DIY: u8 = 0x08;
const ARG_MXS: u8 = 0x10;
const ARG_MXD: u8 = 0x20;

#[derive(Debug, Clone, Copy)]
struct Geometry {
    width: i32,
    bits_per_pixel: u32,
    line_bytes: u32,
    y_mask: i32,
}

impl Geometry {
    fn for_mode(mode: ScreenMode) -> Option<Self> {
        let (width, bits_per_pixel, line_bytes, y_mask) = match mode {
            ScreenMode::Graphic4 => (256, 4, 128, 1023),
            ScreenMode::Graphic5 => (512, 2, 128, 1023),
            ScreenMode::Graphic6 => (512, 4, 256, 511),
            ScreenMode::Graphic7 => (256, 8, 256, 511),
            _ => return None,
        };
        Some(Self {
            width,
            bits_per_pixel,
            line_bytes,
            y_mask,
        })
    }

    fn pixels_per_byte(self) -> i32 {
        (8 / self.bits_per_pixel) as i32
    }

    fn pixel_mask(self) -> u8 {
        ((1u16 << self.bits_per_pixel) - 1) as u8
    }

    /// Byte address, bit shift of the pixel within that byte.
    fn locate(self, x: i32, y: i32) -> (u32, u32) {
        let y = (y & self.y_mask) as u32;
        let x = x as u32;
        let bit = x * self.bits_per_pixel;
        let address = (y * self.line_bytes + bit / 8) & VRAM_MASK;
        let shift = 8 - self.bits_per_pixel - (bit % 8);
        (address, shift)
    }

    fn byte_address(self, byte_x: i32, y: i32) -> u32 {
        (((y & self.y_mask) as u32) * self.line_bytes + byte_x as u32) & VRAM_MASK
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransferKind {
    /// CPU -> VRAM, logical, one pixel per R44 write.
    Lmmc,
    /// CPU -> VRAM, high speed, one byte per R44 write.
    Hmmc,
    /// VRAM -> CPU, logical, one pixel per S#7 read.
    Lmcm,
}

/// Walks a rectangle the way the engine does: NX along X (DIX), then the
/// next line (DIY).
#[derive(Debug, Clone, Copy)]
struct Walker {
    x0: i32,
    x: i32,
    y: i32,
    step_x: i32,
    step_y: i32,
    width: i32,
    column: i32,
    lines_left: i32,
}

impl Walker {
    fn new(x: i32, y: i32, nx: i32, ny: i32, arg: u8) -> Self {
        Self {
            x0: x,
            x,
            y,
            step_x: if arg & ARG_DIX != 0 { -1 } else { 1 },
            step_y: if arg & ARG_DIY != 0 { -1 } else { 1 },
            width: nx,
            column: 0,
            lines_left: ny,
        }
    }

    /// Advance; returns false when the rectangle is finished.
    fn advance(&mut self) -> bool {
        self.column += 1;
        self.x += self.step_x;
        if self.column >= self.width {
            self.column = 0;
            self.x = self.x0;
            self.y += self.step_y;
            self.lines_left -= 1;
        }
        self.lines_left > 0
    }
}

#[derive(Debug, Clone, Copy)]
struct Transfer {
    kind: TransferKind,
    op: u8,
    walker: Walker,
    arg: u8,
}

#[derive(Debug, Clone, Default)]
pub(super) struct CommandState {
    transfer: Option<Transfer>,
}

/// Apply a logical operation; `None` means "leave the destination alone".
fn logical(op: u8, source: u8, destination: u8, mask: u8) -> Option<u8> {
    let source = source & mask;
    if op & 0x08 != 0 && source == 0 {
        return None;
    }
    let value = match op & 0x07 {
        0 => source,
        1 => source & destination,
        2 => source | destination,
        3 => source ^ destination,
        4 => !source,
        _ => return None,
    };
    Some(value & mask)
}

impl V9938 {
    fn reg16(&self, low: usize, mask: u16) -> u16 {
        (u16::from(self.regs[low]) | (u16::from(self.regs[low + 1]) << 8)) & mask
    }

    fn set_reg16(&mut self, low: usize, value: i32, mask: u16) {
        let value = (value as u16) & mask;
        self.regs[low] = value as u8;
        self.regs[low + 1] = (value >> 8) as u8;
    }

    fn sx(&self) -> i32 {
        i32::from(self.reg16(32, 0x1ff))
    }
    fn sy(&self) -> i32 {
        i32::from(self.reg16(34, 0x3ff))
    }
    fn dx(&self) -> i32 {
        i32::from(self.reg16(36, 0x1ff))
    }
    fn dy(&self) -> i32 {
        i32::from(self.reg16(38, 0x3ff))
    }
    /// NX; 0 means 512.
    fn nx(&self) -> i32 {
        match self.reg16(40, 0x1ff) {
            0 => 512,
            n => i32::from(n),
        }
    }
    /// NY; 0 means 1024.
    fn ny(&self) -> i32 {
        match self.reg16(42, 0x3ff) {
            0 => 1024,
            n => i32::from(n),
        }
    }

    fn command_geometry(&self) -> Option<Geometry> {
        Geometry::for_mode(self.screen_mode())
    }

    fn get_pixel(&self, geometry: Geometry, x: i32, y: i32, expansion: bool) -> u8 {
        if expansion {
            return 0;
        }
        let (address, shift) = geometry.locate(x, y);
        (self.vram[address as usize] >> shift) & geometry.pixel_mask()
    }

    fn put_pixel(&mut self, geometry: Geometry, x: i32, y: i32, color: u8, op: u8, arg: u8) {
        if arg & ARG_MXD != 0 || !(0..geometry.width).contains(&x) {
            return;
        }
        let (address, shift) = geometry.locate(x, y);
        let mask = geometry.pixel_mask();
        let byte = &mut self.vram[address as usize];
        let current = (*byte >> shift) & mask;
        if let Some(value) = logical(op, color, current, mask) {
            *byte = (*byte & !(mask << shift)) | (value << shift);
        }
    }

    fn put_byte(&mut self, geometry: Geometry, byte_x: i32, y: i32, value: u8, arg: u8) {
        let bytes_per_line = geometry.width / geometry.pixels_per_byte();
        if arg & ARG_MXD != 0 || !(0..bytes_per_line).contains(&byte_x) {
            return;
        }
        let address = geometry.byte_address(byte_x, y);
        self.vram[address as usize] = value;
    }

    fn get_byte(&self, geometry: Geometry, byte_x: i32, y: i32, arg: u8) -> u8 {
        if arg & ARG_MXS != 0 {
            return 0;
        }
        self.vram[geometry.byte_address(byte_x, y) as usize]
    }

    fn finish_command(&mut self) {
        self.cmd.transfer = None;
        self.status[2] &= !(S2_CE | S2_TR);
    }

    /// Register write to R46.
    pub(super) fn command_start(&mut self, value: u8) {
        self.cmd.transfer = None;
        let Some(geometry) = self.command_geometry() else {
            self.finish_command();
            return;
        };
        let op = value & 0x0f;
        let arg = self.regs[45];
        let clr = self.regs[44];

        match value >> 4 {
            0x4 => {
                // POINT
                self.status[7] = self.get_pixel(geometry, self.sx(), self.sy(), arg & ARG_MXS != 0);
                self.finish_command();
            }
            0x5 => {
                // PSET
                self.put_pixel(geometry, self.dx(), self.dy(), clr, op, arg);
                self.finish_command();
            }
            0x6 => self.command_srch(geometry, arg, clr),
            0x7 => self.command_line(geometry, op, arg, clr),
            0x8 => {
                // LMMV
                let mut walker = Walker::new(self.dx(), self.dy(), self.nx(), self.ny(), arg);
                loop {
                    self.put_pixel(geometry, walker.x, walker.y, clr, op, arg);
                    if !walker.advance() {
                        break;
                    }
                }
                self.set_reg16(38, walker.y, 0x3ff);
                self.set_reg16(42, 0, 0x3ff);
                self.finish_command();
            }
            0x9 => {
                // LMMM
                let mut source = Walker::new(self.sx(), self.sy(), self.nx(), self.ny(), arg);
                let mut dest = Walker::new(self.dx(), self.dy(), self.nx(), self.ny(), arg);
                loop {
                    if (0..geometry.width).contains(&source.x) {
                        let color =
                            self.get_pixel(geometry, source.x, source.y, arg & ARG_MXS != 0);
                        self.put_pixel(geometry, dest.x, dest.y, color, op, arg);
                    }
                    source.advance();
                    if !dest.advance() {
                        break;
                    }
                }
                self.set_reg16(34, source.y, 0x3ff);
                self.set_reg16(38, dest.y, 0x3ff);
                self.set_reg16(42, 0, 0x3ff);
                self.finish_command();
            }
            0xa => {
                // LMCM
                let walker = Walker::new(self.sx(), self.sy(), self.nx(), self.ny(), arg);
                self.status[7] = self.get_pixel(geometry, walker.x, walker.y, arg & ARG_MXS != 0);
                self.cmd.transfer = Some(Transfer {
                    kind: TransferKind::Lmcm,
                    op,
                    walker,
                    arg,
                });
                self.status[2] |= S2_CE | S2_TR;
            }
            0xb => {
                // LMMC: first pixel comes from CLR as written before R46
                let walker = Walker::new(self.dx(), self.dy(), self.nx(), self.ny(), arg);
                self.cmd.transfer = Some(Transfer {
                    kind: TransferKind::Lmmc,
                    op,
                    walker,
                    arg,
                });
                self.status[2] |= S2_CE;
                self.transfer_cpu_byte(clr);
            }
            0xc => {
                // HMMV
                let ppb = geometry.pixels_per_byte();
                let nx = (self.nx() / ppb).max(1);
                let mut walker = Walker::new(self.dx() / ppb, self.dy(), nx, self.ny(), arg);
                loop {
                    self.put_byte(geometry, walker.x, walker.y, clr, arg);
                    if !walker.advance() {
                        break;
                    }
                }
                self.set_reg16(38, walker.y, 0x3ff);
                self.set_reg16(42, 0, 0x3ff);
                self.finish_command();
            }
            0xd => {
                // HMMM
                let ppb = geometry.pixels_per_byte();
                let nx = (self.nx() / ppb).max(1);
                let mut source = Walker::new(self.sx() / ppb, self.sy(), nx, self.ny(), arg);
                let mut dest = Walker::new(self.dx() / ppb, self.dy(), nx, self.ny(), arg);
                let bytes_per_line = geometry.width / ppb;
                loop {
                    if (0..bytes_per_line).contains(&source.x) {
                        let value = self.get_byte(geometry, source.x, source.y, arg);
                        self.put_byte(geometry, dest.x, dest.y, value, arg);
                    }
                    source.advance();
                    if !dest.advance() {
                        break;
                    }
                }
                self.set_reg16(34, source.y, 0x3ff);
                self.set_reg16(38, dest.y, 0x3ff);
                self.set_reg16(42, 0, 0x3ff);
                self.finish_command();
            }
            0xe => {
                // YMMM: from DX to the screen edge in the DIX direction
                let ppb = geometry.pixels_per_byte();
                let bytes_per_line = geometry.width / ppb;
                let start = self.dx() / ppb;
                let nx = if arg & ARG_DIX != 0 {
                    start + 1
                } else {
                    bytes_per_line - start
                };
                let mut source = Walker::new(start, self.sy(), nx, self.ny(), arg);
                let mut dest = Walker::new(start, self.dy(), nx, self.ny(), arg);
                loop {
                    let value = self.get_byte(geometry, source.x, source.y, 0);
                    self.put_byte(geometry, dest.x, dest.y, value, 0);
                    source.advance();
                    if !dest.advance() {
                        break;
                    }
                }
                self.set_reg16(34, source.y, 0x3ff);
                self.set_reg16(38, dest.y, 0x3ff);
                self.set_reg16(42, 0, 0x3ff);
                self.finish_command();
            }
            0xf => {
                // HMMC
                let ppb = geometry.pixels_per_byte();
                let nx = (self.nx() / ppb).max(1);
                let walker = Walker::new(self.dx() / ppb, self.dy(), nx, self.ny(), arg);
                self.cmd.transfer = Some(Transfer {
                    kind: TransferKind::Hmmc,
                    op,
                    walker,
                    arg,
                });
                self.status[2] |= S2_CE;
                self.transfer_cpu_byte(clr);
            }
            _ => {
                // STOP (0x0) and the undefined codes 0x1..0x3
                self.finish_command();
            }
        }
    }

    fn command_srch(&mut self, geometry: Geometry, arg: u8, clr: u8) {
        let target = clr & geometry.pixel_mask();
        let step = if arg & ARG_DIX != 0 { -1 } else { 1 };
        let y = self.sy();
        let mut x = self.sx();
        let stop_on_equal = arg & ARG_EQ == 0;
        self.status[2] &= !S2_BD;
        while (0..geometry.width).contains(&x) {
            let color = self.get_pixel(geometry, x, y, arg & ARG_MXS != 0);
            if (color == target) == stop_on_equal {
                self.status[2] |= S2_BD;
                self.status[8] = (x & 0xff) as u8;
                self.status[9] = ((x >> 8) & 0x01) as u8;
                break;
            }
            x += step;
        }
        self.finish_command();
    }

    fn command_line(&mut self, geometry: Geometry, op: u8, arg: u8, clr: u8) {
        let major = self.reg16(40, 0x1ff) as i32;
        let minor = self.reg16(42, 0x3ff) as i32;
        let step_x = if arg & ARG_DIX != 0 { -1 } else { 1 };
        let step_y = if arg & ARG_DIY != 0 { -1 } else { 1 };
        let mut x = self.dx();
        let mut y = self.dy();
        let mut error = (major - 1) >> 1;
        for _ in 0..=major {
            if !(0..geometry.width).contains(&x) {
                break;
            }
            self.put_pixel(geometry, x, y, clr, op, arg);
            if arg & ARG_MAJ == 0 {
                x += step_x;
            } else {
                y += step_y;
            }
            error -= minor;
            if error < 0 {
                error += major;
                if arg & ARG_MAJ == 0 {
                    y += step_y;
                } else {
                    x += step_x;
                }
            }
        }
        self.set_reg16(38, y, 0x3ff);
        self.finish_command();
    }

    /// One CPU byte for LMMC/HMMC (via R44, or CLR at command start).
    fn transfer_cpu_byte(&mut self, value: u8) {
        let Some(mut transfer) = self.cmd.transfer else {
            return;
        };
        let Some(geometry) = self.command_geometry() else {
            self.finish_command();
            return;
        };
        match transfer.kind {
            TransferKind::Lmmc => self.put_pixel(
                geometry,
                transfer.walker.x,
                transfer.walker.y,
                value,
                transfer.op,
                transfer.arg,
            ),
            TransferKind::Hmmc => self.put_byte(
                geometry,
                transfer.walker.x,
                transfer.walker.y,
                value,
                transfer.arg,
            ),
            TransferKind::Lmcm => return,
        }
        if transfer.walker.advance() {
            self.cmd.transfer = Some(transfer);
            self.status[2] |= S2_TR;
        } else {
            self.set_reg16(38, transfer.walker.y, 0x3ff);
            self.set_reg16(42, 0, 0x3ff);
            self.finish_command();
        }
    }

    /// Register write to R44.
    pub(super) fn command_clr_written(&mut self) {
        if matches!(
            self.cmd.transfer,
            Some(Transfer {
                kind: TransferKind::Lmmc | TransferKind::Hmmc,
                ..
            })
        ) {
            let value = self.regs[44];
            self.transfer_cpu_byte(value);
        }
    }

    /// Side effect of reading S#7: LMCM hands the next pixel to the CPU.
    pub(super) fn command_s7_read(&mut self) {
        let Some(mut transfer) = self.cmd.transfer else {
            return;
        };
        if transfer.kind != TransferKind::Lmcm {
            return;
        }
        let Some(geometry) = self.command_geometry() else {
            self.finish_command();
            return;
        };
        if transfer.walker.advance() {
            self.status[7] = self.get_pixel(
                geometry,
                transfer.walker.x,
                transfer.walker.y,
                transfer.arg & ARG_MXS != 0,
            );
            self.cmd.transfer = Some(transfer);
            self.status[2] |= S2_TR;
        } else {
            self.set_reg16(34, transfer.walker.y, 0x3ff);
            self.set_reg16(42, 0, 0x3ff);
            self.finish_command();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{PORT_COMMAND, PORT_INDIRECT, S2_BD, S2_CE, S2_TR, V9938};

    const G4: u8 = 0x06;
    const G5: u8 = 0x08;
    const G6: u8 = 0x0a;
    const G7: u8 = 0x0e;

    fn vdp(mode: u8) -> V9938 {
        let mut vdp = V9938::new();
        vdp.write_register(0, mode);
        vdp.write_register(1, 0x40);
        vdp.write_register(2, 0x1f);
        vdp
    }

    /// Program R32..R46 through port 3 like real software.
    #[allow(clippy::too_many_arguments)]
    fn command(
        vdp: &mut V9938,
        sx: u16,
        sy: u16,
        dx: u16,
        dy: u16,
        nx: u16,
        ny: u16,
        clr: u8,
        arg: u8,
        cmd: u8,
    ) {
        vdp.write_port(PORT_COMMAND, 32);
        vdp.write_port(PORT_COMMAND, 0x91);
        for word in [sx, sy, dx, dy, nx, ny] {
            vdp.write_port(PORT_INDIRECT, word as u8);
            vdp.write_port(PORT_INDIRECT, (word >> 8) as u8);
        }
        vdp.write_port(PORT_INDIRECT, clr);
        vdp.write_port(PORT_INDIRECT, arg);
        vdp.write_port(PORT_INDIRECT, cmd);
    }

    fn status(vdp: &mut V9938, index: u8) -> u8 {
        vdp.write_register(15, index);
        vdp.read_port(PORT_COMMAND)
    }

    fn pixel(vdp: &V9938, x: i32, y: i32) -> u8 {
        let geometry = super::Geometry::for_mode(vdp.screen_mode()).unwrap();
        vdp.get_pixel(geometry, x, y, false)
    }

    #[test]
    fn pset_writes_pixels_in_every_bitmap_mode() {
        for (mode, x, y, color, address, byte) in [
            (G4, 3, 2, 0x0a, 2 * 128 + 1, 0x0a),
            (G5, 5, 1, 0x03, 128 + 1, 0x30),
            (G6, 510, 1, 0x07, 256 + 255, 0x70),
            (G7, 7, 300, 0xe3, 300 * 256 + 7, 0xe3),
        ] {
            let mut vdp = vdp(mode);
            command(&mut vdp, 0, 0, x, y, 0, 0, color, 0, 0x50);
            assert_eq!(vdp.vram[address], byte, "mode {mode:02x}");
        }
    }

    #[test]
    fn logical_operations_and_transparent_variants() {
        let mut vdp = vdp(G4);
        vdp.vram[0] = 0x60;
        command(&mut vdp, 0, 0, 0, 0, 0, 0, 0x03, 0, 0x51); // AND
        assert_eq!(pixel(&vdp, 0, 0), 0x02);
        command(&mut vdp, 0, 0, 0, 0, 0, 0, 0x08, 0, 0x52); // OR
        assert_eq!(pixel(&vdp, 0, 0), 0x0a);
        command(&mut vdp, 0, 0, 0, 0, 0, 0, 0x0f, 0, 0x53); // EOR
        assert_eq!(pixel(&vdp, 0, 0), 0x05);
        command(&mut vdp, 0, 0, 0, 0, 0, 0, 0x0c, 0, 0x54); // NOT
        assert_eq!(pixel(&vdp, 0, 0), 0x03);
        command(&mut vdp, 0, 0, 0, 0, 0, 0, 0x00, 0, 0x58); // TIMP with 0
        assert_eq!(pixel(&vdp, 0, 0), 0x03);
        command(&mut vdp, 0, 0, 0, 0, 0, 0, 0x00, 0, 0x50); // IMP with 0
        assert_eq!(pixel(&vdp, 0, 0), 0x00);
        assert_eq!(pixel(&vdp, 1, 0), 0x00);
    }

    #[test]
    fn point_reads_into_s7() {
        let mut vdp = vdp(G4);
        vdp.vram[5 * 128 + 2] = 0x0c;
        command(&mut vdp, 5, 5, 0, 0, 0, 0, 0, 0, 0x40);
        assert_eq!(status(&mut vdp, 7), 0x0c);
    }

    fn line_points(dx: u16, dy: u16, major: u16, minor: u16, arg: u8) -> Vec<(i32, i32)> {
        let mut vdp = vdp(G7);
        command(&mut vdp, 0, 0, dx, dy, major, minor, 0xff, arg, 0x70);
        let mut points = Vec::new();
        for y in 0..64 {
            for x in 0..64 {
                if pixel(&vdp, x, y) != 0 {
                    points.push((x, y));
                }
            }
        }
        points
    }

    #[test]
    fn line_draws_all_octants_including_both_endpoints() {
        // (dx, dy) from (32,32) to each octant's far corner, using the BIOS
        // convention: NX = long side, NY = short side.
        let cases: [(i32, i32); 8] = [
            (10, 4),
            (4, 10),
            (-4, 10),
            (-10, 4),
            (-10, -4),
            (-4, -10),
            (4, -10),
            (10, -4),
        ];
        for (dx, dy) in cases {
            let mut arg = 0u8;
            if dx < 0 {
                arg |= 0x04;
            }
            if dy < 0 {
                arg |= 0x08;
            }
            let (major, minor) = if dy.abs() > dx.abs() {
                arg |= 0x01;
                (dy.abs(), dx.abs())
            } else {
                (dx.abs(), dy.abs())
            };
            let points = line_points(32, 32, major as u16, minor as u16, arg);
            assert_eq!(points.len() as i32, major + 1, "({dx},{dy})");
            assert!(points.contains(&(32, 32)), "start ({dx},{dy})");
            assert!(points.contains(&(32 + dx, 32 + dy)), "end ({dx},{dy})");
        }
    }

    #[test]
    fn line_with_zero_length_is_a_single_dot() {
        assert_eq!(line_points(5, 6, 0, 0, 0), vec![(5, 6)]);
    }

    #[test]
    fn lmmv_fills_rectangles_in_both_directions() {
        let mut vdp = vdp(G4);
        command(&mut vdp, 0, 0, 10, 20, 3, 2, 0x07, 0, 0x80);
        assert_eq!(pixel(&vdp, 10, 20), 7);
        assert_eq!(pixel(&vdp, 12, 21), 7);
        assert_eq!(pixel(&vdp, 13, 20), 0);
        assert_eq!(pixel(&vdp, 10, 22), 0);
        assert_eq!(vdp.reg16(38, 0x3ff), 22); // DY advanced past the box

        command(&mut vdp, 0, 0, 50, 50, 2, 2, 0x05, 0x0c, 0x80); // DIX/DIY
        assert_eq!(pixel(&vdp, 49, 49), 5);
        assert_eq!(pixel(&vdp, 51, 50), 0);
    }

    #[test]
    fn lmmv_clips_at_the_right_edge() {
        let mut vdp = vdp(G4);
        command(&mut vdp, 0, 0, 254, 0, 4, 1, 0x09, 0, 0x80);
        assert_eq!(pixel(&vdp, 255, 0), 9);
        assert_eq!(pixel(&vdp, 0, 1), 0);
        assert_eq!(pixel(&vdp, 0, 0), 0);
    }

    #[test]
    fn lmmm_copies_with_logical_op() {
        let mut vdp = vdp(G4);
        command(&mut vdp, 0, 0, 0, 0, 2, 2, 0x06, 0, 0x80);
        command(&mut vdp, 0, 0, 100, 100, 2, 2, 0x09, 0, 0x80);
        command(&mut vdp, 0, 0, 100, 100, 2, 2, 0, 0, 0x92); // OR
        assert_eq!(pixel(&vdp, 101, 101), 0x0f);
        command(&mut vdp, 200, 0, 0, 0, 2, 2, 0, 0, 0x98); // TIMP: 0 source
        assert_eq!(pixel(&vdp, 1, 1), 0x06);
    }

    #[test]
    fn hmmv_and_hmmm_work_in_bytes() {
        let mut vdp = vdp(G4);
        command(&mut vdp, 0, 0, 3, 0, 4, 1, 0xab, 0, 0xc0);
        assert_eq!(&vdp.vram[0..4], &[0x00, 0xab, 0xab, 0x00]);

        command(&mut vdp, 2, 0, 20, 10, 4, 1, 0, 0, 0xd0);
        assert_eq!(&vdp.vram[10 * 128 + 10..10 * 128 + 12], &[0xab, 0xab]);
    }

    #[test]
    fn hmmv_in_g5_and_g7_uses_mode_byte_width() {
        let mut g5 = vdp(G5);
        command(&mut g5, 0, 0, 8, 0, 8, 1, 0xff, 0, 0xc0);
        assert_eq!(&g5.vram[1..5], &[0x00, 0xff, 0xff, 0x00]);
        let mut g7 = vdp(G7);
        command(&mut g7, 0, 0, 8, 0, 8, 1, 0x11, 0, 0xc0);
        assert_eq!(
            &g7.vram[7..17],
            &[0, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0]
        );
    }

    #[test]
    fn ymmm_copies_lines_from_dx_to_the_edge() {
        let mut vdp = vdp(G4);
        for byte in 0..128 {
            vdp.vram[5 * 128 + byte] = byte as u8;
        }
        command(&mut vdp, 0, 5, 200, 9, 0, 1, 0, 0, 0xe0);
        assert_eq!(vdp.vram[9 * 128 + 99], 0);
        assert_eq!(vdp.vram[9 * 128 + 100], 100);
        assert_eq!(vdp.vram[9 * 128 + 127], 127);

        command(&mut vdp, 0, 5, 4, 12, 0, 1, 0, 0x04, 0xe0); // DIX: leftwards
        assert_eq!(vdp.vram[12 * 128], 0);
        assert_eq!(vdp.vram[12 * 128 + 2], 2);
        assert_eq!(vdp.vram[12 * 128 + 3], 0);
    }

    #[test]
    fn srch_finds_colour_and_border_in_both_directions() {
        let mut vdp = vdp(G4);
        command(&mut vdp, 0, 0, 40, 3, 1, 1, 0x0e, 0, 0x50);
        command(&mut vdp, 10, 3, 0, 0, 0, 0, 0x0e, 0, 0x60);
        assert_ne!(status(&mut vdp, 2) & S2_BD, 0);
        assert_eq!(status(&mut vdp, 8), 40);
        assert_eq!(status(&mut vdp, 9) & 1, 0);

        command(&mut vdp, 100, 3, 0, 0, 0, 0, 0x0e, 0x04, 0x60); // leftwards
        assert_eq!(status(&mut vdp, 8), 40);

        command(&mut vdp, 41, 3, 0, 0, 0, 0, 0x0e, 0, 0x60); // not found
        assert_eq!(status(&mut vdp, 2) & S2_BD, 0);

        command(&mut vdp, 40, 3, 0, 0, 0, 0, 0x0e, 0x02, 0x60); // EQ: find != 14
        assert_ne!(status(&mut vdp, 2) & S2_BD, 0);
        assert_eq!(status(&mut vdp, 8), 41);
    }

    #[test]
    fn lmmc_takes_pixels_through_r44_with_tr_and_ce() {
        let mut vdp = vdp(G4);
        command(&mut vdp, 0, 0, 2, 1, 2, 2, 0x01, 0, 0xb0);
        assert_eq!(pixel(&vdp, 2, 1), 1);
        assert_eq!(status(&mut vdp, 2) & (S2_CE | S2_TR), S2_CE | S2_TR);

        vdp.write_register(17, 0x80 | 44);
        for color in [2u8, 3, 4] {
            vdp.write_port(PORT_INDIRECT, color);
        }
        assert_eq!(pixel(&vdp, 3, 1), 2);
        assert_eq!(pixel(&vdp, 2, 2), 3);
        assert_eq!(pixel(&vdp, 3, 2), 4);
        assert_eq!(status(&mut vdp, 2) & S2_CE, 0);
    }

    #[test]
    fn hmmc_takes_bytes_through_r44() {
        let mut vdp = vdp(G4);
        command(&mut vdp, 0, 0, 0, 0, 4, 1, 0x12, 0, 0xf0);
        assert_ne!(status(&mut vdp, 2) & S2_CE, 0);
        vdp.write_register(44, 0x34);
        assert_eq!(&vdp.vram[0..2], &[0x12, 0x34]);
        assert_eq!(status(&mut vdp, 2) & S2_CE, 0);
    }

    #[test]
    fn lmcm_hands_pixels_to_the_cpu_through_s7() {
        let mut vdp = vdp(G4);
        vdp.vram[0] = 0x12;
        vdp.vram[128] = 0x34;
        command(&mut vdp, 0, 0, 0, 0, 2, 2, 0, 0, 0xa0);
        let mut read = Vec::new();
        while status(&mut vdp, 2) & S2_CE != 0 {
            assert_ne!(status(&mut vdp, 2) & S2_TR, 0);
            read.push(status(&mut vdp, 7));
        }
        assert_eq!(read, vec![1, 2, 3, 4]);
    }

    #[test]
    fn stop_aborts_a_cpu_transfer() {
        let mut vdp = vdp(G4);
        command(&mut vdp, 0, 0, 0, 0, 8, 8, 0x01, 0, 0xb0);
        assert_ne!(status(&mut vdp, 2) & S2_CE, 0);
        vdp.write_register(46, 0x00);
        assert_eq!(status(&mut vdp, 2) & S2_CE, 0);
        vdp.write_register(44, 0x05);
        assert_eq!(pixel(&vdp, 1, 0), 0);
    }

    #[test]
    fn commands_are_ignored_outside_bitmap_modes() {
        let mut vdp = vdp(0x00);
        command(&mut vdp, 0, 0, 0, 0, 4, 4, 0xff, 0, 0xc0);
        assert!(vdp.vram.iter().all(|byte| *byte == 0));
        assert_eq!(status(&mut vdp, 2) & S2_CE, 0);
    }

    #[test]
    fn mxd_discards_writes_to_expansion_ram() {
        let mut vdp = vdp(G4);
        command(&mut vdp, 0, 0, 0, 0, 2, 1, 0x0f, 0x20, 0x80);
        assert_eq!(vdp.vram[0], 0);
    }
}
