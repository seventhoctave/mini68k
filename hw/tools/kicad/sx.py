"""Minimal KiCad S-expression parse/serialize + schematic pin geometry."""
import math, re

class Sym(str):
    pass

def parse(text):
    tokens = re.findall(r'"(?:\\.|[^"\\])*"|\(|\)|[^\s()"]+', text)
    pos = 0
    def walk():
        nonlocal pos
        t = tokens[pos]; pos += 1
        if t == '(':
            out = []
            while tokens[pos] != ')':
                out.append(walk())
            pos += 1
            return out
        if t.startswith('"'):
            return t[1:-1].replace('\\"', '"').replace('\\\\', '\\')
        return Sym(t)
    return walk()

def dump(node, indent=0):
    if isinstance(node, list):
        if not node:
            return "()"
        simple = all(not isinstance(c, list) for c in node)
        if simple:
            return "(" + " ".join(dump(c) for c in node) + ")"
        pad = "\t" * (indent + 1)
        parts = ["(" + dump(node[0])]
        for c in node[1:]:
            parts.append("\n" + pad + dump(c, indent + 1))
        return "".join(parts) + "\n" + "\t" * indent + ")"
    if isinstance(node, Sym):
        return str(node)
    if isinstance(node, (int, float)):
        return fmt(node)
    return '"' + str(node).replace('\\', '\\\\').replace('"', '\\"') + '"'

def fmt(v):
    s = ("%.4f" % v).rstrip('0').rstrip('.')
    return "0" if s in ("-0", "") else s

def find(node, key):
    return [c for c in node if isinstance(c, list) and c and c[0] == key]

def first(node, key):
    r = find(node, key)
    return r[0] if r else None

def prop(node, name):
    for p in find(node, 'property'):
        if p[1] == name:
            return p
    return None

def lib_pins(lib_symbol, unit=1, style=1):
    """(number, name, x, y, angle, length) for pins of a lib symbol unit/body style."""
    out = []
    for sub in find(lib_symbol, 'symbol'):
        m = re.match(r'.*_(\d+)_(\d+)$', sub[1])
        u, st = (int(m.group(1)), int(m.group(2))) if m else (0, 0)
        if u not in (0, unit) or st not in (0, style):
            continue
        for p in find(sub, 'pin'):
            at = first(p, 'at'); ln = first(p, 'length')
            out.append((first(p, 'number')[1], first(p, 'name')[1], float(at[1]), float(at[2]),
                        float(at[3]) if len(at) > 3 else 0.0, float(ln[1])))
    return out

def pin_point(inst, px, py):
    """Schematic coordinate of a lib pin connection point for a symbol instance."""
    at = first(inst, 'at'); x0, y0 = float(at[1]), float(at[2]); rot = float(at[3]) if len(at) > 3 else 0
    mir = first(inst, 'mirror')
    x, y = px, -py                      # lib Y up -> sheet Y down
    if mir is not None:
        if mir[1] == 'x': y = -y
        if mir[1] == 'y': x = -x
    # mirror is applied first, then the rotation (clockwise in Y-down sheet coords)
    r = math.radians(rot)
    xr = x * math.cos(r) - y * math.sin(r)
    yr = x * math.sin(r) + y * math.cos(r)
    return round(x0 + xr, 2), round(y0 + yr, 2)

def instances(sch):
    return [c for c in sch if isinstance(c, list) and c and c[0] == 'symbol' and first(c, 'lib_id')]

def lib_by_id(sch):
    libs = first(sch, 'lib_symbols')
    return {s[1]: s for s in find(libs, 'symbol')}

def ref(inst):
    p = prop(inst, 'Reference')
    return p[2] if p else None

def inst_pins(inst, libs):
    """{pin number: (name, (x, y))} for a placed symbol."""
    lib = libs[first(inst, 'lib_id')[1]]
    unit = int(first(inst, 'unit')[1]) if first(inst, 'unit') else 1
    bs = first(inst, 'body_style')
    style = int(bs[1]) if bs else 1
    return {n: (nm, pin_point(inst, x, y)) for n, nm, x, y, a, l in lib_pins(lib, unit, style)}
