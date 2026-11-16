"""Compare schematic netlist (kicadxml) connectivity with a .kicad_pcb (run with KiCad's python)."""
import sys, xml.etree.ElementTree as ET, collections, pcbnew
xmlp, pcbp = sys.argv[1], sys.argv[2]
ignore = set(sys.argv[3].split(',')) if len(sys.argv) > 3 else set()
t = ET.parse(xmlp).getroot()
sch = {}
for net in t.iter('net'):
    for n in net.iter('node'):
        sch[(n.get('ref'), n.get('pin'))] = net.get('name')
b = pcbnew.LoadBoard(pcbp)
pcb = {}
for fp in b.GetFootprints():
    for p in fp.Pads():
        if p.GetNumber():
            pcb[(fp.GetReference(), p.GetNumber())] = p.GetNetname()
common = {k for k in set(sch) & set(pcb) if k[0] not in ignore}
only_s = sorted(k for k in set(sch) - set(pcb) if k[0] not in ignore)
only_p = sorted(k for k in set(pcb) - set(sch) if k[0] not in ignore and not k[0].startswith('REF'))
pg = collections.defaultdict(set); sg = collections.defaultdict(set)
for k in common:
    pg[pcb[k]].add(k); sg[sch[k]].add(k)
bad = 0
for name, pins in sorted(pg.items()):
    sn = {sch[p] for p in pins}
    if len(sn) != 1 or sg[next(iter(sn))] != pins:
        bad += 1
        print('MISMATCH pcb', name or '<none>', sorted(pins)[:8], '-> sch', sorted(sn)[:5])
print('only in sch:', only_s[:30]); print('only in pcb:', only_p[:30])
print('mismatched nets:', bad, 'of', len(pg))
