# KiCad helper scripts

Run these with KiCad's bundled Python, which provides `pcbnew`:

```sh
PY=/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/bin/python3
```

| Script | Purpose |
|--------|---------|
| `generate_vdp.py` | Writes `hw/vdp-lib` (symbols, SDIP-64 footprint) and the schematics and placed PCBs of `hw/vdp-v9938` and `hw/vdp-tms9918a` from one netlist definition. UUIDs are deterministic, so regenerating keeps schematic and PCB linked. |
| `route.py` | Autoroutes a board with Freerouting 2.5.0 (`/Applications/FreeRouting.app`). Makes N fresh attempts, keeps the best, continues while that improves, then refills zones. |
| `rename_nets.py` | Gives PCB nets the schematic's names (`/X`, `{slash}`, `unconnected-(...)`) so KiCad's schematic-parity check is clean. |
| `compare.py` | Compares pin-to-net connectivity between a schematic netlist (XML) and a PCB. |
| `drcsum.py` | Summarises a `kicad-cli pcb drc --format json` report. |
| `sx.py` | Minimal KiCad S-expression reader/writer and schematic pin geometry. |

## VDP boards from scratch

```sh
$PY hw/tools/kicad/generate_vdp.py
for b in vdp-v9938 vdp-tms9918a; do
  $PY hw/tools/kicad/route.py hw/$b/$b.kicad_pcb /tmp/route-$b 200 nolock 3
  kicad-cli sch export netlist --format kicadxml -o /tmp/$b.xml hw/$b/$b.kicad_sch
  $PY hw/tools/kicad/rename_nets.py /tmp/$b.xml hw/$b/$b.kicad_pcb
  kicad-cli sch erc hw/$b/$b.kicad_sch
  kicad-cli pcb drc --schematic-parity hw/$b/$b.kicad_pcb
done
```

`route.py` runs Freerouting 2.5.0 headless. (The 2019 build that used to be
in `FreeRouting.app` couldn't.) It cleans the exported DSN first, because
Freerouting rejects KiCad's zero-length "oval" pad paths.
