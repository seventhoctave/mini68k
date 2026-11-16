# mini68k emulator

This crate provides a usable emulator for the `mini68k` SBC. The implementation
is Rust on the host side with a locally vendored Musashi 68k core, a minimal
68681 device model, and models of the two video processors the v2 board can
take: a Yamaha V9938 or a TI TMS9918A.

## Supported hardware

- `v1`: 68008 software-visible behavior using Musashi's 68000 core, 32 KiB ROM,
  32 KiB RAM, and the SCC68681 DUART console
- `v2`: the same base machine plus one VDP, selected with `--vdp`: a V9938
  (default) or a TMS9918A. Only one is fitted, as on the real board.
- DUART interrupt delivery on level 2 using the DUART IVR value

## V9938 model

The VDP (`src/v9938/`) aims to match everything software can observe on the real
chip, not just the parts the BIOS uses:

- **Ports:** VRAM data with the read-ahead latch, the register/address latch
  (reset by status reads), palette writes through R16, and indirect register
  writes through R17 with auto-increment
- **VRAM addressing:** 17-bit through R14, carrying into R14 in GRAPHIC4-7 and
  wrapping at 16 KiB in the other modes
- **Status registers S#0-S#9:** vblank `F`, 5th/9th sprite and collision with
  coordinates, line interrupt `FH`, `TR`/`VR`/`HR`/`BD`/`EO`/`CE`, POINT/LMCM
  colour, SRCH result
- **Screen modes:** TEXT1, TEXT2 (with blink), MULTICOLOR, GRAPHIC1-7, using the
  R2/R3/R4/R10 table-mask behaviour, R23 vertical scroll, R18 display adjust,
  192/212 lines, page select, and R13 / EO alternate-page display
- **Sprites:** mode 1 (G1, G2, MC) and mode 2 (G3-G7) with 8x8/16x16, MAG,
  EC, CC (OR), IC, TP, SPD, the Y=208/216 terminators, 4/8-per-line limits,
  fixed GRAPHIC7 sprite colours, and GRAPHIC5 split sprite colours
- **Command engine:** all 12 commands (POINT, PSET, SRCH, LINE, LMMV, LMMM, LMCM,
  LMMC, HMMV, HMMM, YMMM, HMMC, STOP) and every logical operation. LMMC, HMMC
  and LMCM follow the real TR/CE handshake.
- **Video timing:** 262 lines (NTSC) or 313 lines (PAL) per frame on a 21.48 MHz
  master clock, paced against the configured CPU clock. Vblank and line
  interrupts drive `/INT`.

## TMS9918A model

`src/tms9918a.rs` covers what software can observe on the TMS9918A:

- **Ports:** data and control/status with `MODE` on CPU A1, the read-ahead
  latch, and the address quirk where the first control byte already loads
  the low address bits
- **VRAM:** 16 KiB with 14-bit wrap
- **Registers and status:** R0-R7, with the status register's `F`, `5S`, `C`
  and fifth-sprite number cleared on read
- **Screen modes:** Graphics I, Graphics II (including the R3/R4
  "half-bitmap" table masks), Multicolor and Text, in the fixed TMS9918A
  palette
- **Sprites:** 8x8/16x16, magnification, early clock, 4 per line, the
  Y=208 terminator, and collision (transparent sprites collide too)
- **Timing:** 262-line NTSC frames on a 10.74 MHz clock, with the vblank
  interrupt

Not modelled: analog output, external video (R0 EXTVID), 4K DRAM addressing,
and the CPU access window. Real hardware drops VRAM accesses that come too
fast. The emulator accepts them, so software that relies on the emulator
alone needs the same pacing as `bios/tms9918.S`.

## Interrupts and video output

`/INT` is unconnected on the v2 board, so by default the VDP interrupt does not
reach the CPU. Pass `--vdp-irq <level>` to wire it to a 68k autovector level.

The window and `Machine::video_frame()` show a fixed canvas: 512x212 for the
V9938 and 512x192 for the TMS9918A. 256-pixel modes are doubled horizontally,
and the border colour fills unused areas.

Not modelled: analog video, light pen/mouse, superimpose/digitize, expansion
RAM (MXD writes are dropped, MXS reads return 0), and command-engine cycle
timing (VRAM-only commands finish instantly). YJK colour is not modelled
because it belongs to the V9958, not the V9938.

## Memory map model

The v1 GAL only decodes `A16:A15`, so the emulator mirrors the hardware windows
across the 20-bit address space just like the real board:

- `A16:A15 = 00`: ROM
- `A16:A15 = 10`: RAM
- `A16:A15 = 11`: DUART in data space

That aliasing is why the BIOS can use short absolute addresses such as `0x8000`
for the DUART while still landing on the real device select lines.

For the v2 profile, the `11` region is further split by `A14`:

- `A16:A15:A14 = 110`: DUART
- `A16:A15:A14 = 111`: V9938

## Current limitations

- Functional CPU/peripheral timing only; not cycle-exact
- No debugger UI or snapshots yet
- DUART channel B, timer, and input/output ports are stubbed unless the BIOS
  needs them later

## Running

Build the BIOS and run the emulator through the top-level Docker workflow:

```sh
make bios-docker
make emu-run
```

For the v2 video paths (type `G` for the graphics demo, `T` to return to text):

```sh
make bios-docker V9938=1
make emu-run V9938=1 EMU_ARGS="--machine v2 --video window"

make bios-docker TMS9918=1
make emu-run TMS9918=1 EMU_ARGS="--machine v2 --vdp tms9918a --video window"
```

For a bounded smoke run:

```sh
docker compose run --rm dev cargo run --release -p mini68k-emu -- --rom bios/bios.bin --max-cycles 2000000
```

The CLI accepts:

- `--machine v1|v2`
- `--video headless|window`
- `--video-scale <n>`
- `--vdp v9938|tms9918a`: which VDP the v2 machine has (default v9938)
- `--load <file.s28>`: copy an S-record program into RAM before it runs
  (repeatable). Unlike sending it through the monitor's `S2` download, nothing
  is lost to the BIOS's 128-byte receive buffer.
- `--type <keys>`: type keys once the monitor shows its first `>` prompt;
  `\r` and `\n` are expanded, e.g. `--type 'J 10000\r'`
- `--cpu-hz <hz>`: CPU clock used to pace video timing (default 8000000)
- `--vdp-irq <level>`: wire V9938 `/INT` to a 68k interrupt level (default: unconnected)

## Demos

`demos/` holds programs that use the BIOS `TRAP #0` video API and run on
either VDP. `make demo-run` builds the demo and a matching v2 BIOS, then opens
the emulator window with the demo running. Add `TMS9918=1` for the TMS9918A.
`demos/bounce.S` bounces three sprite balls over a bitmap scene.

## Tests

`make emu-test` runs the unit tests and the integration tests. The
integration tests boot the real BIOS for each VDP and check the text console,
scrolling, the `G` graphics demo, and VDP interrupt delivery. They also write
snapshots to `target/v9938-*.png` and `target/tms9918-*.png`.
