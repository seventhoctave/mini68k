"""Rename PCB nets to the schematic's net names (connectivity must already match)."""
import sys, xml.etree.ElementTree as ET, collections, pcbnew
xmlp, pcbp = sys.argv[1], sys.argv[2]
def escape(name):
    # the XML netlist prints '/' inside names unescaped; KiCad stores it as {slash}
    # (a leading '/' is the sheet path, not part of the name)
    if name.startswith('/'):
        return '/' + name[1:].replace('/', '{slash}')
    return name.replace('/', '{slash}')

sch = {}
for net in ET.parse(xmlp).getroot().iter('net'):
    for n in net.iter('node'):
        sch[(n.get('ref'), n.get('pin'))] = escape(net.get('name'))
b = pcbnew.LoadBoard(pcbp)
want = collections.defaultdict(set)
for fp in b.GetFootprints():
    for p in fp.Pads():
        k = (fp.GetReference(), p.GetNumber())
        if k in sch and p.GetNetCode() > 0:
            want[p.GetNetCode()].add(sch[k])
n = 0
for code, names in want.items():
    ni = b.FindNet(code)
    if len(names) == 1:
        new = next(iter(names))
        if ni.GetNetname() != new:
            ni.SetNetname(new); n += 1
    else:
        print('ambiguous', ni.GetNetname(), names)
# pads left without a net get the schematic's per-pin "unconnected-(...)" net
added = 0
for fp in b.GetFootprints():
    for p in fp.Pads():
        k = (fp.GetReference(), p.GetNumber())
        if p.GetNetCode() == 0 and k in sch:
            ni = b.FindNet(sch[k])
            if ni is None:
                ni = pcbnew.NETINFO_ITEM(b, sch[k]); b.Add(ni)
            p.SetNet(ni); added += 1
b.Save(pcbp); print('renamed', n, 'nets; gave', added, 'unconnected pads their schematic net')
