# mini68k v2 — V9938 video revision

This directory is a placeholder for the v2 KiCad project. The KiCad
files themselves must be created in the GUI — copy `hw/mini68k/` as
the starting point and apply the changes below.

## Starting point
1. In KiCad, open `hw/mini68k/mini68k.kicad_pro` and "Save As" into
   this directory as `mini68k-v2.kicad_pro`. Do not edit v1 in place.
2. Grow the board outline from 100×100 to ~100×160 (Eurocard) to make
   room for the V9938 subcircuit.

## New parts
| Ref | Part                         | Package       | Notes |
|-----|------------------------------|---------------|-------|
| U?  | Yamaha V9938                 | SDIP-64       | MSX2 VDP |
| U?  | µPD41464 ×2                  | DIP-18        | 64K×4 DRAM = 128 KB VRAM |
| U?  | 74HC00 (or 74HC139)          | DIP-14 / 16   | A14 qualification of UARTCS |
| X?  | 21.47727 MHz crystal         | HC-49         | VDP dot-clock |
| C?  | 22 pF ×2                     | 0805 or disc  | Xtal load caps |
| C?  | 0.1 µF ×~6                   | 0805          | Decoupling on V9938 + VRAMs |
| C?  | 10 µF                        | tantalum      | Bulk on V9938 rail |
| R?  | 75 Ω ×3                      |               | R/G/B termination |
| C?  | 10 µF ×3                     |               | R/G/B DC-blocking |
| J?  | DE-15 VGA receptacle         | through-hole  | Video out |

## New schematic sheet: video.kicad_sch

### Address decode qualification
Disconnect v1 GAL `UARTCS` from `U7` `/CS`. Route it through a 1-of-2
split qualified by A14:

```
GAL UARTCS ──┐
             ├── NAND/decode ──► /UART_CS_N  (A14 = 0)  → U7 /CS
A14 ─────────┘                └► /VDP_CS_N   (A14 = 1)  → V9938 gating
```

Minimal discrete version using 74HC00 quad NAND:
- `UART_CS_N  = NOT ( NOT UARTCS  AND  NOT A14 )`
- `VDP_CS_N   = NOT ( NOT UARTCS  AND  A14       )`

or equivalently use half a 74HC139 enabled by UARTCS with A14 as the
select.

### V9938 wiring
- `D0..D7`  ← CPU data bus
- `MODE0`   ← A1   (CPU A1, because 68008 word-aligned register addresses
                   land even bytes at A0=0)
- `MODE1`   ← A2
- `/CSR`    ← `VDP_CS_N` OR NOT `R/W`      (read strobe when R/W=1)
- `/CSW`    ← `VDP_CS_N` OR `R/W`          (write strobe when R/W=0)
- `/RESET`  ← system `/RESET`
- `/INT`    ← leave **unconnected** in v1 (polling-only driver)
- `/WAIT`   ← wire into the DTACK path:
             `DTACK_N = UART_DTACK  AND  (VDP_CS_N  OR  VDP_WAIT_N)`
             Fallback jumper "WAIT_MODE": if V9938 `/WAIT` proves fussy,
             short it out and insert 1–2 fixed wait cycles via a small
             74HC161 counter clocked from CPU CLK, enabled by VDP_CS_N.
- `XTAL1`/`XTAL2` ← 21.47727 MHz crystal + 22 pF caps to GND
- `VDD` ← 5 V, decoupled

### VRAM (2× µPD41464)
Wire strictly per V9938 datasheet section 3 (VRAM connection). The CPU
bus does not touch these parts — the VDP drives RAS/CAS/RAD/WE/OE
directly. Place < 1 inch from U?(V9938).

### Video output
- V9938 pins `R`, `G`, `B` → 10 µF series → 75 Ω termination → DE-15
  pins 1, 2, 3
- V9938 composite sync (`CSYNC` / `HSYNC` pin per your part revision)
  → DE-15 pin 13; tie DE-15 pin 14 to same csync or leave NC
- DE-15 pin 5, 6, 7, 8, 10 → GND
- DE-15 shell → chassis / GND
- Silkscreen label: **"15 kHz — CRT or scandoubler required"**

## No GAL change
`hw/gal/glue.pld` is intentionally untouched. All v2 decode changes
live in discrete 74-series logic on the new schematic sheet, so the
existing GAL fuse map and programming flow are unaffected.

## Alternative VDP: TMS9918A
The VDP slot can hold a TI TMS9918A instead of the V9938. Only one is
fitted at a time; the BIOS is built for one or the other.

| Ref | Part                         | Package       | Notes |
|-----|------------------------------|---------------|-------|
| U?  | TI TMS9918A                  | DIP-40        | NTSC composite out |
| U?  | TMS4416 ×2 (or 4116 ×8)      | DIP-18/16     | 16 KB VRAM |
| X?  | 10.738635 MHz crystal        | HC-49         | VDP clock |
| J?  | RCA jack                     |               | composite video |

- Same `VDP_CS_N` qualification as above. `/CSR` and `/CSW` gate the
  same way.
- `MODE` ← A1 (data at `$1C000`, control/status at `$1C002`).
- `/INT` is left unconnected (polled driver).
- The TMS9918A has **no `/WAIT`**. Tie the DTACK contribution from the VDP
  select to "ready" (or keep the WAIT_MODE fixed-wait fallback). The BIOS
  spaces its VRAM accesses in software (`TMS_WAIT` in `bios/tms9918.S`).
- VRAM is wired per the TMS9918A data manual. The CPU bus never touches it.
- The video output is composite NTSC, not RGB. A TMS9928A gives Y/R-Y/B-Y,
  and a TMS9929A is the PAL part; the BIOS works on either, but the
  emulator only models the 9918A's NTSC timing.

## BIOS coupling
Build v2 ROMs with `make V9938=1` or `make TMS9918=1` in `bios/`, to
match the fitted VDP. Both drivers use the same addresses. The V9938 driver sits
at short-address base `0xC000` → `$FC000` effective → A16=A15=A14=1,
which only decodes to the VDP on v2 hardware. Building `V9938=1` and
burning into a v1 board will corrupt the DUART — do not do this.
