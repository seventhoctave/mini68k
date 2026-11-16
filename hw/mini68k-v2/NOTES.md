# mini68k v2: system board + plug-in VDP boards

v2 is three KiCad projects:

| Project | What it is |
|---------|------------|
| `hw/mini68k-v2/` | The system board: the 100x100 v1 board with the VDP bus added |
| `hw/vdp-v9938/` | VDP board with a Yamaha V9938 and 128 KB VRAM, RGB + composite out |
| `hw/vdp-tms9918a/` | VDP board with a TI TMS9918A and 16 KB VRAM, composite out |

You fit one VDP board at a time. Each plugs onto the system board's bottom
edge: right-angle sockets on the VDP board mate with right-angle headers on
the system board, and the two boards end up coplanar. Build the BIOS to match
the fitted board (`make -C bios V9938=1` or `TMS9918=1`).

## System board changes (from the 100x100 v1 board)

- **A14 into the GAL:** GAL U5 pin 11 was tied to GND. It now takes A14.
  `hw/gal/glue-v2.pld` qualifies `UARTCS` with `/A14`, so the DUART only
  answers at `$18000-$1BFFF`. The v1 equations in `glue.pld` are unchanged.
- **J8 control header:** a 7-pin right-angle header on the bottom edge,
  right of J7, carrying DS, R/W, FC1, RESET, DTACK, VCC and GND (pin 1 = DS,
  at the right). J6 (A0-A19) and J7 (DB0-DB7) are unchanged, but the bus now
  also feeds the VDP boards.
- **Mounting hole:** the bottom-right hole was removed to make room for J8.
- **Outline:** trimmed to exactly 100.0 x 100.0 mm (it was 100.33 x 100.33),
  centred where it was: x 89.065-189.065, y 63.665-163.665. The VDP boards
  use the same X span.
- **Re-routed:** the board was fully re-routed with Freerouting. The original
  v1 GND/VCC pours were fragmented (19 disconnected islands in its own DRC);
  power now has 0.5 mm tracks plus the pours.

The VDP has no interrupt on the bus. `/INT` stays unconnected, as the polled
BIOS and the emulator default expect.

## VDP board design (both boards)

- **Select:** a 74HC138 decodes `/VDP_CS` = A16 * A15 * A14 with /DS and
  /FC1 low. These are the GAL's DUART terms with A14 = 1, so the VDP sits at
  `$1C000`.
- **Strobes:** a 74HC139 makes `/CSW` (R/W = 0) and `/CSR` (R/W = 1) from
  `/VDP_CS`.
- **DTACK:** a 74HC125 pulls DTACK low while `/VDP_CS` is active. The system
  board's RN1 pulls it up.
- **Register mode lines:** MODE0 is driven by A1 and MODE1 by A2 (V9938); MODE
  is driven by A1 (TMS9918A).
- **Bus timing:** neither VDP has a `/WAIT` pin. The V9938 data book's pin
  table (p.114) has `*VDS` on pin 4; `/WAIT` arrived with the V9938's
  successor, the V9958. DTACK is therefore immediate. The V9938 accepts
  strobes of 186 ns to 2 us (p.121). The BIOS spaces VDP port accesses in
  software: `VDP_WAIT` in `bios/v9938.S` and `TMS_WAIT` in `bios/tms9918.S`.

### V9938 board

- **VRAM:** 4x uPD41464 (64Kx4), 128 KB. The pinout is from the NEC data
  sheet in `hw/datasheets/`.
  - DRAM A0-A7 are driven by AD0-AD7.
  - Each chip's I/O1-4 connect to the RD bus, which carries data both ways
    (the V9938 writes on RD0-7, p.124).
  - `/CAS0` selects bank 0 and `/CAS1` bank 1.
  - `/OE` is tied low, since the V9938 does early-write cycles.
- **Footprint:** the V9938 is a 64-pin shrink DIP (1.778 mm pitch, 19.05 mm
  rows, p.134). Its footprint is in `hw/vdp-lib/vdp.pretty`.
- **Crystal:** 21.47727 MHz, connected directly to XTAL1/XTAL2 as in the
  data book's example (p.113). The two load-capacitor footprints are fitted
  DNP.
- **RGB out:** R, G and B each get a 470 R load (the spec is given into
  470 R, p.127), then a 2N3904 emitter follower, then a 75 R series resistor
  to the DE-15. CSYNC goes through the same kind of follower with 100 R to
  DE-15 pin 13. Signal levels: 15 kHz RGB, so use a CRT or a scandoubler.
- **Composite out:** VIDEO goes through the same kind of buffer to the J4
  2-pin header (signal, GND), for wiring to a panel RCA jack.
- **Light pen:** `*LPS` and `*LPD` have 10 k pull-ups.

### TMS9918A board

- **VRAM:** 8x 4164 (64Kx1, +5 V) with A7 tied low, giving 16 KB. Wiring
  follows the TI manual, table 3-1, 16K column:
  - DRAM A0..A6 are driven by AD7..AD1.
  - Chip k has D driven by ADk and Q driving RDk.
  - TI numbers bits MSB-first (CD0, AD0 and RD0 are the MSBs), so CPU DB7
    goes to CD0.
- **Crystal:** 10.738635 MHz, with DNP load-capacitor footprints.
- **Composite out:** COMVID has a 330 R pull-down (manual fig. 3-3), then an
  emitter follower and 75 R to the J4 header.
- **EXTVDP:** 10 k to GND. It is the cascade input and is unused here.

## Check before fabrication

- **Mating:** check the right-angle header/socket mating gap and height with
  the actual parts. J6 is on the system board's back side, so the VDP board's
  J1 socket is on its back side too.
- **Video levels:** check the buffer values (series R, emitter R, AC
  coupling) against your monitor or scandoubler. They follow the data books'
  example topology but are untested.
- **DE-15:** check the footprint's edge position on the V9938 board.
- **Pacing:** check `VDP_WAIT_LOOPS` and `TMS_WAIT_LOOPS` on real hardware.
- **DRC:** both VDP boards route completely with no clearance errors. The
  remaining DRC items are silkscreen, a few single-spoke thermals, and the
  intentional end-to-end courtyard overlap of J2 and J3.

## Regenerating the VDP boards

The VDP boards are generated by `hw/tools/kicad/generate_vdp.py` from one
netlist, then autorouted; see `hw/tools/kicad/README.md`. Manual edits in
KiCad are fine, but re-running the generator overwrites them.
