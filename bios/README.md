# mini68k BIOS

This directory contains the GAS translation of the original `mini68k` BIOS from:
http://www.christle.us/index.php?P=4

## Building

Build the default ROM image format:

```sh
make -C bios
```

That produces the BIOS outputs in this directory, including:

- `bios.bin`
- `bios.lst`
- `bios.o`

By default the Makefile emits Motorola S-record output. For emulator use, build
a raw binary image instead. For the v2 video BIOS, add `V9938=1` or
`TMS9918=1` (one or the other: only one VDP can be fitted):

```sh
make -C bios FORMAT=binary
make -C bios FORMAT=binary V9938=1
make -C bios FORMAT=binary TMS9918=1
```

## Toolchain

The BIOS build expects a 68k assembler and linker compatible with these names:

- `m68k-unknown-elf-as`
- `m68k-unknown-elf-ld`

The recommended path for this repository is the containerized toolchain from the
repo root:

```sh
make bios-docker
make bios-docker V9938=1
make bios-docker TMS9918=1
```

That runs the BIOS build inside the Docker development environment and writes
the resulting `bios/bios.bin` back into the workspace.

## Video build options (v2 only)

The v2 board has one VDP slot at short-address base `0xC000`. It can hold either
a Yamaha V9938 or a TI TMS9918A. Build the ROM for the chip that is fitted:

| Option | Driver | Chip |
|--------|--------|------|
| `V9938=1` | `v9938.S` | Yamaha V9938 (128 KiB VRAM) |
| `TMS9918=1` | `tms9918.S` | TI TMS9918A (16 KiB VRAM) |

The options are mutually exclusive: both the Makefile and the assembler reject
a build with both enabled. Do not use either on v1 hardware, where the VDP
address collides with the DUART.

Both drivers are polled (`/INT` is not connected on v2). They export the same
routines and the same `TRAP #0` API, so programs run on either chip.

| | V9938 | TMS9918A |
|---|---|---|
| Text console | 80x24 TEXT2, scrolling | 40x24 Text mode, scrolling |
| Bitmap | GRAPHIC4 256x212, any colour per pixel | Graphics II 256x192, one foreground/background colour pair per 8-pixel row segment |
| Drawing | VDP command engine | software (Bresenham lines, byte-at-a-time boxes) |
| Sprites | mode 2: per-line colours | one colour per sprite, max 4 per line |
| Palette | programmable (`VDP_SETPAL`) | fixed (`VDP_SETPAL` is a no-op) |
| Bus timing | no `/WAIT` on the V9938 either: the driver spaces port accesses in software (`VDP_WAIT`, 8 us) | no `/WAIT`: the driver spaces VRAM accesses in software (`TMS_WAIT`) |

On the TMS9918A, colour 0 in `VDP_PSET`/`VDP_BOX`/`VDP_LINE` clears pixels. Any
other colour sets them and becomes the foreground of the affected 8-pixel
segment.

### Monitor commands

| Command | Action |
|---------|--------|
| `G` | Graphics demo: GRAPHIC4 colour bars, outline, line fan, dots and two sprites |
| `T` | Back to TEXT2 (reloads the font, clears the screen) |

In graphics mode the serial console keeps working; the screen just stops
mirroring text until `T`.

### TRAP #0 video calls

Put the function number in `d0` and arguments in `d1`-`d5`/`a1`, then
`trap #0`. Calls preserve `d1`-`d7`/`a1`; `d0` and `a0` are scratch.
Coordinates are bitmap pixels: x 0-255, y 0-211 on the V9938 and y 0-191 on
the TMS9918A.

| d0 | Call | Arguments |
|----|------|-----------|
| 8  | `VDP_SETMODE` | `d1.b` 0 = text, otherwise bitmap (GRAPHIC4 / Graphics II) |
| 9  | `VDP_CLS` | `d1.b` colour (bitmap mode); text mode clears and homes the cursor |
| 10 | `VDP_PSET` | `d1.w` x, `d2.w` y, `d3.b` colour |
| 11 | `VDP_LINE` | `d1.w` x0, `d2.w` y0, `d3.w` x1, `d4.w` y1, `d5.b` colour |
| 12 | `VDP_BOX` | `d1.w` x, `d2.w` y, `d3.w` width, `d4.w` height, `d5.b` colour (filled) |
| 13 | `VDP_SETPAL` | `d1.b` index, `d2.w` = `0RRR0BBB 00000GGG` (no-op on TMS9918A) |
| 14 | `VDP_SPRPAT` | `d1.b` pattern 0-63, `a1` -> 32 bytes (quadrants: TL, BL, TR, BR) |
| 15 | `VDP_SPRSET` | `d1.b` sprite 0-31, `d2.w` x, `d3.w` y, `d4.b` pattern, `d5.b` colour |
| 16 | `VDP_SPRHIDE` | `d1.b` sprite 0-31 |

Example: a red diagonal line.

```asm
    moveq #8, %d0       /* VDP_SETMODE */
    moveq #1, %d1       /* bitmap mode */
    trap  #0
    moveq #11, %d0      /* VDP_LINE */
    moveq #0, %d1
    moveq #0, %d2
    movew #255, %d3
    movew #191, %d4
    moveq #8, %d5       /* red */
    trap  #0
```

### VRAM layout

V9938:

| Mode | Address | Contents |
|------|---------|----------|
| TEXT2 | `0x0000` | name table (80x24) |
| TEXT2 | `0x0800` | blink table (blink disabled) |
| TEXT2 | `0x1000` | 6x8 font (`font.bin`) |
| GRAPHIC4 | `0x0000`-`0x69FF` | bitmap page 0, 128 bytes per line |
| GRAPHIC4 | `0x7400` | sprite colour table |
| GRAPHIC4 | `0x7600` | sprite attribute table |
| GRAPHIC4 | `0x7800` | sprite pattern table |

TMS9918A:

| Mode | Address | Contents |
|------|---------|----------|
| Text | `0x0000` | name table (40x24) |
| Text | `0x0800` | 6x8 font (`font.bin`) |
| Graphics II | `0x0000` | bitmap pattern table (6 KiB) |
| Graphics II | `0x1800` | sprite pattern table |
| Graphics II | `0x2000` | colour table (6 KiB) |
| Graphics II | `0x3800` | name table (0-255 in each third) |
| Graphics II | `0x3B00` | sprite attribute table |

### Register settings to check on first silicon

The tables in `v9938.S` program NTSC timing (R9 `NT`=0), 64Kx4 DRAM (R8 `VR`=1,
which the uPD41464 VRAM needs), and TEXT2 R2 = `0x03` (the low two bits must
be 1). Check these against the V9938 datasheet when bringing up a board.

`tms9918.S` sets R1 `4/16K`=1 for 16 KiB DRAM. `TMS_WAIT_LOOPS` controls how
far apart VRAM accesses are on a live screen (at least 8 us at 8 MHz). Raise
it if characters or pixels go missing on real hardware.

### Font

`font.bin` is a 128-glyph 6x8 font (pixels in bits 7..2) generated by
`mkfont.py` and checked in so builds don't need Python. After editing the
glyph table, regenerate it with:

```sh
make -C bios font
```

## ROM programming

To burn the ROM with `minipro`:

```sh
make -C bios burn-rom
```

The default target ROM type is controlled by `ROM` in [Makefile](Makefile).
