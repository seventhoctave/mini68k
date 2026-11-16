import json, sys, collections
d = json.load(open(sys.argv[1])); skip = set(sys.argv[2].split(',')) if len(sys.argv) > 2 else set()
for sec in ('violations', 'unconnected_items', 'schematic_parity'):
    c = collections.Counter(v['type'] for v in d.get(sec, []))
    print(sec, dict(c))
    for v in d.get(sec, []):
        if v['type'] in skip: continue
        print('   ', v['type'], '|', ' | '.join(i.get('description','') for i in v.get('items', []))[:200])
