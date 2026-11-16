# mini68k emulator

This crate provides a usable emulator for the `mini68k` SBC. The implementation
is Rust on the host side with a locally vendored Musashi 68k core, a minimal
68681 device model, and a Yamaha V9938 video model for the v2 board profile.

## Supported hardware

- `v1`: 68008 software-visible behavior using Musashi's 68000 core, 32 KiB ROM,
  32 KiB RAM, and the SCC68681 DUART console
- `v2`: the same base machine plus a V9938 with 128 KiB VRAM
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

`/INT` is unconnected on the v2 board, so by default the VDP interrupt does not
reach the CPU. Pass `--vdp-irq <level>` to wire it to a 68k autovector level.

The window and `Machine::video_frame()` show a fixed 512x212 canvas:
256-pixel modes are doubled horizontally, and the border colour fills unused
areas.

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

For the v2 V9938 path (type `G` for the graphics demo, `T` to return to text):

```sh
make bios-docker V9938=1
make emu-run V9938=1 EMU_ARGS="--machine v2 --video window"
```

For a bounded smoke run:

```sh
docker compose run --rm dev cargo run --release -p mini68k-emu -- --rom bios/bios.bin --max-cycles 2000000
```

The CLI accepts:

- `--machine v1|v2`
- `--video headless|window`
- `--video-scale <n>`
- `--cpu-hz <hz>`: CPU clock used to pace video timing (default 8000000)
- `--vdp-irq <level>`: wire V9938 `/INT` to a 68k interrupt level (default: unconnected)

## Tests

`make emu-test` runs the unit tests and the integration tests. The
integration tests boot the real BIOS and check the text console, scrolling,
the `G` graphics demo, and VDP interrupt delivery. They also write snapshots
to `target/v9938-text.png`, `target/v9938-scroll.png` and
`target/v9938-demo.png`.
