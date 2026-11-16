#!/usr/bin/env python3
"""Generate the mini68k v2 VDP plug-in boards from one netlist definition.

Run with KiCad's bundled Python (it needs pcbnew):
  /Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/bin/python3 \
      hw/tools/kicad/generate_vdp.py

Writes:
  hw/vdp-lib/vdp.kicad_sym           custom symbols (VDPs, DRAMs, 74HC logic, NPN, DE-15)
  hw/vdp-lib/vdp.pretty/             SDIP-64 footprint for the V9938
  hw/vdp-v9938/vdp-v9938.*           schematic + placed (unrouted) PCB
  hw/vdp-tms9918a/vdp-tms9918a.*     schematic + placed (unrouted) PCB
Route afterwards with route.py (Freerouting), then check with kicad-cli ERC/DRC.

Both boards plug onto the system board's bottom-edge bus headers J6 (A0-A19),
J7 (DB0-DB7) and J8 (DS, R/W, FC1, RESET, DTACK, VCC, GND) with right-angle
sockets, and decode their own select: A16*A15*A14 with /DS and /FC1, the
same terms the GAL uses for the DUART but with A14 = 1 ($1C000).
"""
import copy, os, pathlib, re, sys, uuid
import pcbnew

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import sx  # noqa: E402

HW = HERE.parent.parent
LIBDIR = HW / 'vdp-lib'
KSYM = pathlib.Path('/Applications/KiCad/KiCad.app/Contents/SharedSupport/symbols')
KFP = pathlib.Path('/Applications/KiCad/KiCad.app/Contents/SharedSupport/footprints')
SYSTEM_PRO = HW / 'mini68k-v2' / 'mini68k-v2.kicad_pro'
NS = uuid.UUID('6d696e69-3638-6b2d-7664-702d626f6172')   # fixed namespace: stable UUIDs
_uid = {'board': '', 'n': 0}

def U(key=None):
    """Deterministic UUIDs so regenerating keeps schematic <-> PCB links stable."""
    if key is None:
        _uid['n'] += 1
        key = '#%d' % _uid['n']
    return str(uuid.uuid5(NS, _uid['board'] + ':' + key))
S = sx.Sym

# ---------------------------------------------------------------------------
# System-board bus geometry (absolute X positions shared with mini68k-v2)
# Same 100.0 mm width and X span as the system board (x 89.065-189.065)
BOARD_X0, BOARD_X1 = 89.065, 189.065
PART_DX = 0.24
J6_X = [98.425 + 2.54 * k for k in range(20)]            # A0..A19 (system J6, back side)
J7_X = [168.91 - 2.54 * k for k in range(8)]             # DB0..DB7 (system J7)
J8_X = [186.69 - 2.54 * k for k in range(7)]             # J8 control header
J8_SIG = ['DS', 'R{slash}W', 'FC1', 'RESET', 'DTACK', 'VCC', 'GND']
TOP = 170.0                                              # daughter board top edge
SOCKET_Y = TOP + 9.6                                     # socket opening ~1 mm past the top edge

# ---------------------------------------------------------------------------
# Custom symbols: (name, ref, footprint, description, left pins, right pins)
# pin = (number, name, type); type: 'p' passive, 'i' input, 'o' output,
# 'b' bidirectional, 'oc' open collector, 'pw' power_in, 'nc' no_connect
PT = {'p': 'passive', 'i': 'input', 'o': 'output', 'b': 'bidirectional', 'oc': 'open_collector',
      'pw': 'power_in', 'nc': 'no_connect', 't': 'tri_state'}

def pins(spec):
    out = []
    for item in spec.split(','):
        num, name, typ = item.strip().split(':')
        out.append((num, name, typ))
    return out

SYMBOLS = [
    ('V9938', 'U', 'vdp:SDIP-64_W19.05mm_P1.778mm',
     'Yamaha V9938 MSX-VIDEO VDP, 64-pin shrink DIP (data book p.114)',
     pins('40:CD0:b,39:CD1:b,38:CD2:b,37:CD3:b,36:CD4:b,35:CD5:b,34:CD6:b,32:CD7:b,29:MODE0:i,28:MODE1:i,'
          '31:~{CSR}:i,30:~{CSW}:i,9:~{RESET}:i,25:~{INT}:oc,26:~{LPS}:i,27:~{LPD}:i,63:XTAL1:i,64:XTAL2:p,'
          '8:CPUCLK:o,2:~{DHCLK}:oc,3:~{DLCLK}:oc,58:VCC:pw,1:GND:pw,20:GND_DAC:pw,33:VBB:nc'),
     pins('41:RD0:b,42:RD1:b,43:RD2:b,44:RD3:b,45:RD4:b,46:RD5:b,47:RD6:b,48:RD7:b,49:AD0:o,50:AD1:o,51:AD2:o,'
          '52:AD3:o,53:AD4:o,54:AD5:o,55:AD6:o,56:AD7:o,62:~{RAS}:o,61:~{CAS0}:o,60:~{CAS1}:o,59:~{CASX}:o,'
          '57:R/~{W}:o,4:~{VDS}:o,23:R:o,22:G:o,24:B:o,21:VIDEO:o,6:CSYNC:b,5:HSYNC:b,7:BLEO:oc,10:~{YS}:o,'
          '11:CBDR:o,19:C0:b,18:C1:b,17:C2:b,16:C3:b,15:C4:b,14:C5:b,13:C6:b,12:C7:b')),
    ('TMS9918A', 'U', 'Package_DIP:DIP-40_W15.24mm_LongPads',
     'TI TMS9918A VDP (data manual 3.7.1). CD0/AD0/RD0 are the MSBs',
     pins('24:CD0:b,23:CD1:b,22:CD2:b,21:CD3:b,20:CD4:b,19:CD5:b,18:CD6:b,17:CD7:b,13:MODE:i,15:~{CSR}:i,'
          '14:~{CSW}:i,34:~{RESET}/SYNC:i,16:~{INT}:o,40:XTAL1:i,39:XTAL2:p,38:CPUCLK:o,37:GROMCLK:o,'
          '33:VCC:pw,12:VSS:pw'),
     pins('32:RD0:i,31:RD1:i,30:RD2:i,29:RD3:i,28:RD4:i,27:RD5:i,26:RD6:i,25:RD7:i,10:AD0:o,9:AD1:o,8:AD2:o,'
          '7:AD3:o,6:AD4:o,5:AD5:o,4:AD6:o,3:AD7:o,1:~{RAS}:o,2:~{CAS}:o,11:R/~{W}:o,36:COMVID:o,35:EXTVDP:i')),
    ('uPD41464', 'U', 'Package_DIP:DIP-18_W7.62mm_LongPads',
     'NEC uPD41464 64K x 4 DRAM, 18-pin DIP (NEC data sheet 3B-1)',
     pins('14:A0:i,13:A1:i,12:A2:i,11:A3:i,8:A4:i,7:A5:i,6:A6:i,10:A7:i,5:~{RAS}:i,16:~{CAS}:i,4:~{WE}:i,'
          '1:~{OE}:i,9:VCC:pw'),
     pins('2:IO1:b,3:IO2:b,15:IO3:b,17:IO4:b,18:GND:pw')),
    ('4164', 'U', 'Package_DIP:DIP-16_W7.62mm_LongPads',
     '64K x 1 DRAM, +5 V single supply (4116 signal pinout, pin 9 = A7)',
     pins('5:A0:i,7:A1:i,6:A2:i,12:A3:i,11:A4:i,10:A5:i,13:A6:i,9:A7:i,4:~{RAS}:i,15:~{CAS}:i,3:~{W}:i,'
          '8:VCC:pw,1:NC:nc'),
     pins('2:D:i,14:Q:t,16:VSS:pw')),
    ('74HC138', 'U', 'Package_DIP:DIP-16_W7.62mm_LongPads', '3-to-8 line decoder',
     pins('1:A0:i,2:A1:i,3:A2:i,6:E2:i,4:~{E0}:i,5:~{E1}:i,16:VCC:pw,8:GND:pw'),
     pins('15:~{Y0}:o,14:~{Y1}:o,13:~{Y2}:o,12:~{Y3}:o,11:~{Y4}:o,10:~{Y5}:o,9:~{Y6}:o,7:~{Y7}:o')),
    ('74HC139', 'U', 'Package_DIP:DIP-16_W7.62mm_LongPads', 'dual 2-to-4 line decoder',
     pins('1:~{1E}:i,2:1A0:i,3:1A1:i,15:~{2E}:i,14:2A0:i,13:2A1:i,16:VCC:pw,8:GND:pw'),
     pins('4:~{1Y0}:o,5:~{1Y1}:o,6:~{1Y2}:o,7:~{1Y3}:o,12:~{2Y0}:o,11:~{2Y1}:o,10:~{2Y2}:o,9:~{2Y3}:o')),
    ('74HC125', 'U', 'Package_DIP:DIP-14_W7.62mm_LongPads', 'quad tri-state buffer, active-low enables',
     pins('2:1A:i,1:~{1OE}:i,5:2A:i,4:~{2OE}:i,9:3A:i,10:~{3OE}:i,12:4A:i,13:~{4OE}:i,14:VCC:pw,7:GND:pw'),
     pins('3:1Y:t,6:2Y:t,8:3Y:t,11:4Y:t')),
    ('NPN_EBC', 'Q', 'Package_TO_SOT_THT:TO-92_Inline_Wide', 'NPN transistor, E-B-C pinout (2N3904)',
     pins('2:B:i'), pins('3:C:p,1:E:p')),
    ('DE15_HD', 'J', 'Connector_Dsub:DSUB-15-HD_Socket_Horizontal_P2.29x1.90mm_EdgePinOffset3.03mm_Housed_MountingHolesOffset4.94mm',
     'DE-15 high-density socket (VGA style); 15 kHz RGB + composite sync',
     pins('1:R:p,2:G:p,3:B:p,4:ID0:p,5:GND:p,6:R_GND:p,7:G_GND:p,8:B_GND:p'),
     pins('9:KEY:p,10:SYNC_GND:p,11:ID1:p,12:ID2:p,13:HSYNC:p,14:VSYNC:p,15:ID3:p,SH:SHIELD:p')),
]
SYMDEF = {s[0]: s for s in SYMBOLS}
ANGLE_OF = {'L': 0, 'R': 180}  # pin angle: left-side pins point right (into the body)

def sym_geometry(name):
    """(half width, top y, pin positions {num: (x, y, outward_angle)})"""
    _, _, _, _, left, right = SYMDEF[name]
    rows = max(len(left), len(right))
    widest = max([len(p[1]) for p in left + right] + [4])
    half = max(7.62, round((widest * 1.27 + 2.54) / 2.54) * 2.54)
    top = (rows - 1) * 2.54 / 2
    top = round(top / 2.54) * 2.54
    pos = {}
    for i, (num, nm, typ) in enumerate(left):
        pos[num] = (-half - 2.54, top - i * 2.54, 180)
    for i, (num, nm, typ) in enumerate(right):
        pos[num] = (half + 2.54, top - i * 2.54, 0)
    return half, top, pos

def font():
    return [S('effects'), [S('font'), [S('size'), S('1.27'), S('1.27')]]]

def prop(name, value, x=0, y=0, hide=False):
    p = [S('property'), name, value, [S('at'), S(sx.fmt(x)), S(sx.fmt(y)), S('0')]]
    if hide:
        p.append([S('hide'), S('yes')])
    p.append(font())
    return p

def build_symbol(name, top_name=None):
    nm, ref, fp, desc, left, right = SYMDEF[name]
    half, top, pos = sym_geometry(name)
    rows = max(len(left), len(right))
    bottom = top - (rows - 1) * 2.54
    out = [S('symbol'), top_name or name, [S('pin_names'), [S('offset'), S('1.016')]],
           [S('exclude_from_sim'), S('no')], [S('in_bom'), S('yes')], [S('on_board'), S('yes')],
           prop('Reference', ref, 0, top + 3.81), prop('Value', name, 0, bottom - 3.81),
           prop('Footprint', fp, 0, 0, True), prop('Datasheet', '', 0, 0, True),
           prop('Description', desc, 0, 0, True)]
    body = [S('symbol'), name + '_0_1',
            [S('rectangle'), [S('start'), S(sx.fmt(-half)), S(sx.fmt(top + 2.54))],
             [S('end'), S(sx.fmt(half)), S(sx.fmt(bottom - 2.54))],
             [S('stroke'), [S('width'), S('0.254')], [S('type'), S('default')]],
             [S('fill'), [S('type'), S('background')]]]]
    unit = [S('symbol'), name + '_1_1']
    for num, pname, typ in left + right:
        x, y, outward = pos[num]
        into = (outward + 180) % 360
        unit.append([S('pin'), S(PT[typ]), S('line'), [S('at'), S(sx.fmt(x)), S(sx.fmt(y)), S(str(into))],
                     [S('length'), S('2.54')], [S('name'), pname, font()], [S('number'), num, font()]])
    out += [body, unit, [S('embedded_fonts'), S('no')]]
    return out

def write_symbol_lib():
    LIBDIR.mkdir(exist_ok=True)
    lib = [S('kicad_symbol_lib'), [S('version'), S('20241209')], [S('generator'), 'mini68k_generate_vdp'],
           [S('generator_version'), '1.0']]
    lib += [build_symbol(n) for n in SYMDEF]
    (LIBDIR / 'vdp.kicad_sym').write_text(sx.dump(lib) + '\n')

def write_sdip64():
    """Yamaha V9938 64-pin shrink DIP: 1.778 mm pitch, 19.05 mm rows (data book p.134)."""
    d = LIBDIR / 'vdp.pretty'; d.mkdir(parents=True, exist_ok=True)
    pitch, row, n = 1.778, 19.05, 32
    length = (n - 1) * pitch
    lines = ['(footprint "SDIP-64_W19.05mm_P1.778mm"', '\t(version 20241229)', '\t(generator "mini68k_generate_vdp")',
             '\t(layer "F.Cu")', '\t(descr "64-lead shrink DIP, 1.778 mm pitch, 19.05 mm row spacing (Yamaha V9938)")',
             '\t(tags "SDIP SDIP64 shrink DIP V9938")', '\t(attr through_hole)',
             '\t(property "Reference" "REF**" (at %.3f -2.5 0) (layer "F.SilkS") (effects (font (size 1 1) (thickness 0.15))))' % (row / 2),
             '\t(property "Value" "SDIP-64_W19.05mm_P1.778mm" (at %.3f %.3f 0) (layer "F.Fab") (effects (font (size 1 1) (thickness 0.15))))' % (row / 2, length + 2.5),
             '\t(fp_rect (start 1.0 -1.0) (end %.3f %.3f) (stroke (width 0.1) (type solid)) (fill no) (layer "F.Fab"))' % (row - 1.0, length + 1.0),
             '\t(fp_line (start 1.4 -1.1) (end %.3f -1.1) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))' % (row / 2 - 1),
             '\t(fp_line (start %.3f -1.1) (end %.3f -1.1) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))' % (row / 2 + 1, row - 1.4),
             '\t(fp_line (start 1.4 %.3f) (end %.3f %.3f) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))' % (length + 1.1, row - 1.4, length + 1.1),
             '\t(fp_arc (start %.3f -1.1) (mid %.3f -0.1) (end %.3f -1.1) (stroke (width 0.12) (type solid)) (layer "F.SilkS"))' % (row / 2 - 1, row / 2, row / 2 + 1),
             '\t(fp_rect (start -1.55 -1.6) (end %.3f %.3f) (stroke (width 0.05) (type solid)) (fill no) (layer "F.CrtYd"))' % (row + 1.55, length + 1.6)]
    for i in range(n):
        y = i * pitch
        shape = 'rect' if i == 0 else 'oval'
        lines.append('\t(pad "%d" thru_hole %s (at 0 %.4f) (size 2.4 1.3) (drill 0.8) (layers "*.Cu" "*.Mask"))' % (i + 1, shape, y))
        lines.append('\t(pad "%d" thru_hole oval (at %.3f %.4f) (size 2.4 1.3) (drill 0.8) (layers "*.Cu" "*.Mask"))' % (2 * n - i, row, y))
    lines.append(')')
    (d / 'SDIP-64_W19.05mm_P1.778mm.kicad_mod').write_text('\n'.join(lines) + '\n')

# ---------------------------------------------------------------------------
# Board netlists. part = dict(ref, sym, value, fp, nets{pin: net}, pcb=(x, y, rot, side))
# Nets named ~{X} are active low. Unlisted pins of custom symbols get no-connect flags.

def part(ref, sym, value, fp, nets, at, sch=None, dnp=False):
    return dict(ref=ref, sym=sym, value=value, fp=fp, nets=nets, at=at, sch=sch, dnp=dnp)

R_FP = 'Resistor_THT:R_Axial_DIN0207_L6.3mm_D2.5mm_P7.62mm_Horizontal'
C_FP = 'Capacitor_THT:C_Disc_D5.0mm_W2.5mm_P2.50mm'
CP_FP = 'Capacitor_THT:CP_Radial_D5.0mm_P2.50mm'
XT_FP = 'Crystal:Crystal_HC49-4H_Vertical'
HOLE = 'MountingHole:MountingHole_3.2mm_M3'

def common_parts():
    """Edge sockets, select decode and DTACK driver shared by both VDP boards.

    The decode sits at the top right, under the J2/J3 sockets that carry the
    control signals it uses."""
    ps = []
    ps.append(part('J1', 'Connector_Generic:Conn_01x20', 'A0-A19 (to J6)',
                   'Connector_PinSocket_2.54mm:PinSocket_1x20_P2.54mm_Horizontal', 'bus:J6', ('bus', J6_X, 'B')))
    ps.append(part('J2', 'Connector_Generic:Conn_01x08', 'DB0-DB7 (to J7)',
                   'Connector_PinSocket_2.54mm:PinSocket_1x08_P2.54mm_Horizontal', 'bus:J7', ('bus', J7_X, 'F')))
    ps.append(part('J3', 'Connector_Generic:Conn_01x07', 'control (to J8)',
                   'Connector_PinSocket_2.54mm:PinSocket_1x07_P2.54mm_Horizontal', 'bus:J8', ('bus', J8_X, 'F')))
    # /VDP_CS = A16*A15*A14 with /DS and /FC1 low (data space): 74HC138 /Y6 (A2..A0 = 1,1,0)
    ps.append(part('U1', 'vdp:74HC138', '74HC138', None,
                   {'1': 'GND', '2': 'A15', '3': 'A16', '6': 'A14', '4': 'DS', '5': 'FC1', '9': '~{VDP_CS}',
                    '16': 'VCC', '8': 'GND'}, (151.0, 185.5, 0, 'F')))
    # /CSW when R/W = 0, /CSR when R/W = 1
    ps.append(part('U2', 'vdp:74HC139', '74HC139', None,
                   {'1': '~{VDP_CS}', '2': 'R{slash}W', '3': 'GND', '4': '~{CSW}', '5': '~{CSR}',
                    '15': 'VCC', '14': 'GND', '13': 'GND', '16': 'VCC', '8': 'GND'}, (162.0, 185.5, 0, 'F')))
    # DTACK: pull low (open-drain style) while the VDP is selected; the CPU board pulls it up
    ps.append(part('U3', 'vdp:74HC125', '74HC125', None,
                   {'1': '~{VDP_CS}', '2': 'GND', '3': 'DTACK', '4': 'VCC', '5': 'GND', '10': 'VCC', '9': 'GND',
                    '13': 'VCC', '12': 'GND', '14': 'VCC', '7': 'GND'}, (173.0, 185.5, 0, 'F')))
    for ref, x in (('C1', 152.3), ('C2', 163.3), ('C3', 174.3)):
        ps.append(part(ref, 'Device:C', '100n', C_FP, {'1': 'VCC', '2': 'GND'}, (x, 207.5, 0, 'F')))
    ps.append(part('C4', 'Device:C_Polarized', '10u', CP_FP, {'1': 'VCC', '2': 'GND'}, (184.0, 213.0, 0, 'F')))
    return ps

def emitter_follower(n, base, out_net, connector_net, x, y, load=None, series='75'):
    """Video buffer row: optional base load, NPN follower, 470R emitter, series resistor to the connector."""
    ps = []
    if load:
        ps.append(part('R%d' % (10 * n + 1), 'Device:R', load, R_FP, {'1': base, '2': 'GND'}, (x, y, 0, 'F')))
    ps.append(part('Q%d' % n, 'vdp:NPN_EBC', '2N3904', None, {'2': base, '3': 'VCC', '1': out_net}, (x + 10.5, y, 0, 'F')))
    ps.append(part('R%d' % (10 * n + 2), 'Device:R', '470', R_FP, {'1': out_net, '2': 'GND'}, (x + 19.0, y, 0, 'F')))
    ps.append(part('R%d' % (10 * n + 3), 'Device:R', series, R_FP, {'1': out_net, '2': connector_net}, (x + 29.0, y, 0, 'F')))
    return ps

def v9938_board():
    """Layout: DRAMs top-left facing the V9938's VRAM pins (top row), video
    buffers under its video pins (bottom row), DE-15 on the bottom edge."""
    ps = common_parts()
    vdp = {'40': 'DB0', '39': 'DB1', '38': 'DB2', '37': 'DB3', '36': 'DB4', '35': 'DB5', '34': 'DB6', '32': 'DB7',
           '29': 'A1', '28': 'A2', '31': '~{CSR}', '30': '~{CSW}', '9': 'RESET', '26': '~{LPS}', '27': '~{LPD}',
           '63': 'XTAL1', '64': 'XTAL2', '58': 'VCC', '1': 'GND', '20': 'GND',
           '62': '~{RAS}', '61': '~{CAS0}', '60': '~{CAS1}', '57': 'VR{slash}~{W}',
           '23': 'VDP_R', '22': 'VDP_G', '24': 'VDP_B', '21': 'VDP_VIDEO', '6': 'VDP_CSYNC'}
    for i in range(8):
        vdp[str(41 + i)] = 'RD%d' % i
        vdp[str(49 + i)] = 'AD%d' % i
    # rot 90: pins 1-32 run left->right along y=233, pins 64->33 along y=213.95
    ps.append(part('U4', 'vdp:V9938', 'V9938', None, vdp, (93.0, 233.0, 90, 'F')))
    ps.append(part('C5', 'Device:C', '100n', C_FP, {'1': 'VCC', '2': 'GND'}, (107.5, 208.6, 0, 'F')))
    ps.append(part('C6', 'Device:C', '100n', C_FP, {'1': 'VCC', '2': 'GND'}, (119.5, 208.6, 0, 'F')))
    ps.append(part('Y1', 'Device:Crystal', '21.47727MHz', XT_FP, {'1': 'XTAL1', '2': 'XTAL2'}, (92.5, 197.0, 90, 'F')))
    ps.append(part('C7', 'Device:C', 'DNP 22p', C_FP, {'1': 'XTAL1', '2': 'GND'}, (96.95, 190.5, 90, 'F'), dnp=True))
    ps.append(part('C8', 'Device:C', 'DNP 22p', C_FP, {'1': 'XTAL2', '2': 'GND'}, (96.95, 197.0, 90, 'F'), dnp=True))
    ps.append(part('R1', 'Device:R', '10k', R_FP, {'1': '~{LPS}', '2': 'VCC'}, (136.0, 237.5, 0, 'F')))
    ps.append(part('R2', 'Device:R', '10k', R_FP, {'1': '~{LPD}', '2': 'VCC'}, (136.0, 241.5, 0, 'F')))
    # 4 x uPD41464 = 128 KB: bank 0 on /CAS0, bank 1 on /CAS1; /OE tied low (early-write VDP)
    for k in range(4):
        ref = 'U%d' % (5 + k)
        cas = '~{CAS0}' if k < 2 else '~{CAS1}'
        base = 0 if k % 2 == 0 else 4
        m = {'14': 'AD0', '13': 'AD1', '12': 'AD2', '11': 'AD3', '8': 'AD4', '7': 'AD5', '6': 'AD6', '10': 'AD7',
             '5': '~{RAS}', '16': cas, '4': 'VR{slash}~{W}', '1': 'GND', '9': 'VCC', '18': 'GND',
             '2': 'RD%d' % base, '3': 'RD%d' % (base + 1), '15': 'RD%d' % (base + 2), '17': 'RD%d' % (base + 3)}
        x = 100.0 + k * 12.0
        ps.append(part(ref, 'vdp:uPD41464', 'uPD41464', None, m, (x, 184.0, 0, 'F')))
        ps.append(part('C%d' % (9 + k), 'Device:C', '100n', C_FP, {'1': 'VCC', '2': 'GND'},
                       (x + 1.27, 208.6, 0, 'F')))
    # Video: R, G, B (470R loads per data book p.127) on the left, CSYNC and composite on the right
    ps += emitter_follower(1, 'VDP_R', 'R_BUF', 'VGA_R', 90.5, 239.0, load='470')
    ps += emitter_follower(2, 'VDP_G', 'G_BUF', 'VGA_G', 90.5, 245.0, load='470')
    ps += emitter_follower(3, 'VDP_B', 'B_BUF', 'VGA_B', 90.5, 251.0, load='470')
    ps += emitter_follower(4, 'VDP_CSYNC', 'SYNC_BUF', 'VGA_CSYNC', 150.5, 239.0, series='100')
    ps += emitter_follower(5, 'VDP_VIDEO', 'COMP_BUF', 'COMPOSITE', 150.5, 245.0, load='470')
    ps.append(part('J4', 'Connector_Generic:Conn_01x02', 'composite out',
                   'Connector_PinHeader_2.54mm:PinHeader_1x02_P2.54mm_Vertical', {'1': 'COMPOSITE', '2': 'GND'},
                   (186.0, 252.0, 0, 'F')))
    ps.append(part('J5', 'vdp:DE15_HD', 'RGB + CSYNC (15 kHz)', None,
                   {'1': 'VGA_R', '2': 'VGA_G', '3': 'VGA_B', '5': 'GND', '6': 'GND', '7': 'GND', '8': 'GND',
                    '10': 'GND', '13': 'VGA_CSYNC', 'SH': 'GND'}, (130.0, 267.0, 180, 'F')))
    for i, (x, y) in enumerate(((92.7, 265.0), (185.4, 265.0))):
        ps.append(part('H%d' % (i + 1), 'Mechanical:MountingHole', 'M3', HOLE, {}, (x, y, 0, 'F')))
    return dict(name='vdp-v9938', title='mini68k v2 VDP board: Yamaha V9938', bottom=270.0, parts=ps)

def tms_board():
    """Layout: TMS9918A top-left (AD pins on its bottom row), the eight DRAMs
    in a row under it, composite buffer at the bottom."""
    ps = common_parts()
    for p in ps:
        if p['ref'] == 'C4':
            p['at'] = (178.0, 260.5, 0, 'F')
    # CD0 is the MSB on TI parts: CPU DB7 -> CD0 ... DB0 -> CD7
    vdp = {'24': 'DB7', '23': 'DB6', '22': 'DB5', '21': 'DB4', '20': 'DB3', '19': 'DB2', '18': 'DB1', '17': 'DB0',
           '13': 'A1', '15': '~{CSR}', '14': '~{CSW}', '34': 'RESET', '40': 'XTAL1', '39': 'XTAL2',
           '33': 'VCC', '12': 'GND', '1': '~{RAS}', '2': '~{CAS}', '11': 'VR{slash}~{W}', '36': 'COMVID',
           '35': 'EXTVDP'}
    for k in range(8):
        vdp[str(32 - k)] = 'RD%d' % k       # RD0 pin 32 ... RD7 pin 25
        vdp[str(10 - k)] = 'AD%d' % k       # AD0 pin 10 ... AD7 pin 3
    # rot 90: pins 1-20 run left->right along y=207, pins 40->21 along y=191.76
    ps.append(part('U4', 'vdp:TMS9918A', 'TMS9918A', None, vdp, (92.0, 207.0, 90, 'F')))
    ps.append(part('C5', 'Device:C', '100n', C_FP, {'1': 'VCC', '2': 'GND'}, (130.5, 186.0, 0, 'F')))
    ps.append(part('Y1', 'Device:Crystal', '10.738635MHz', XT_FP, {'1': 'XTAL1', '2': 'XTAL2'}, (96.0, 186.0, 0, 'F')))
    ps.append(part('C6', 'Device:C', 'DNP 22p', C_FP, {'1': 'XTAL1', '2': 'GND'}, (106.5, 186.0, 0, 'F'), dnp=True))
    ps.append(part('C7', 'Device:C', 'DNP 22p', C_FP, {'1': 'XTAL2', '2': 'GND'}, (112.5, 186.0, 0, 'F'), dnp=True))
    ps.append(part('R1', 'Device:R', '10k', R_FP, {'1': 'EXTVDP', '2': 'GND'}, (118.5, 186.0, 0, 'F')))
    # 8 x 4164 (+5 V 64Kx1), A7 tied low: 16 KB. Chip k: D <- ADk, Q -> RDk;
    # address AD7..AD1 -> A0..A6 (manual table 3-1, 4116 column)
    addr = {'5': 'AD7', '7': 'AD6', '6': 'AD5', '12': 'AD4', '11': 'AD3', '10': 'AD2', '13': 'AD1', '9': 'GND'}
    for k in range(8):
        ref = 'U%d' % (5 + k)
        m = dict(addr)
        m.update({'2': 'AD%d' % k, '14': 'RD%d' % k, '4': '~{RAS}', '15': '~{CAS}', '3': 'VR{slash}~{W}',
                  '8': 'VCC', '16': 'GND'})
        # two rows of four, turned 90 deg so the address bus runs under each
        # chip between its pin rows (pins 1-8 along y0, 16-9 along y0-7.62)
        x = 95.0 + (k % 4) * 23.0
        y = 228.0 if k < 4 else 254.0
        ps.append(part(ref, 'vdp:4164', '4164', None, m, (x, y, 90, 'F')))
        ps.append(part('C%d' % (8 + k), 'Device:C', '100n', C_FP, {'1': 'VCC', '2': 'GND'},
                       (x + 5.0, 214.5 if k < 4 else 241.5, 0, 'F')))
    # Composite: 330R pull-down on the source-follower output (manual fig. 3-3), then a buffer
    ps += emitter_follower(1, 'COMVID', 'COMP_BUF', 'COMPOSITE', 100.0, 260.5, load='330')
    ps.append(part('J4', 'Connector_Generic:Conn_01x02', 'composite out',
                   'Connector_PinHeader_2.54mm:PinHeader_1x02_P2.54mm_Vertical', {'1': 'COMPOSITE', '2': 'GND'},
                   (150.0, 259.2, 0, 'F')))
    for i, (x, y) in enumerate(((92.7, 266.0), (185.4, 266.0))):
        ps.append(part('H%d' % (i + 1), 'Mechanical:MountingHole', 'M3', HOLE, {}, (x, y, 0, 'F')))
    return dict(name='vdp-tms9918a', title='mini68k v2 VDP board: TI TMS9918A', bottom=270.0, parts=ps,
                bus={'A1', 'A14', 'A15', 'A16'}, dx=0.0)

# ---------------------------------------------------------------------------
# PCB

def fp_ref(p):
    if p['fp']:
        return p['fp']
    return SYMDEF[p['sym'].split(':')[1]][2]

def load_fp(fpid):
    lib, name = fpid.split(':')
    path = LIBDIR / 'vdp.pretty' if lib == 'vdp' else KFP / (lib + '.pretty')
    fp = pcbnew.FootprintLoad(str(path), name)
    assert fp, fpid
    fp.SetFPID(pcbnew.LIB_ID(lib, name))
    return fp

def flip(fp):
    try:
        fp.Flip(fp.GetPosition(), pcbnew.FLIP_DIRECTION_TOP_BOTTOM)
    except Exception:
        fp.Flip(fp.GetPosition(), False)

def place_socket(b, fp, xs, side):
    """Rotate/flip a right-angle socket so its body faces the top edge and its pads sit on xs."""
    mm = pcbnew.FromMM
    if side == 'B':
        flip(fp)
    want = sorted(xs)
    for rot in (90, 270, 0, 180):
        fp.SetOrientationDegrees(rot)
        for anchor in (min(xs), max(xs)):
            pads = sorted(fp.Pads(), key=lambda p: int(p.GetNumber()))
            p1 = pads[0].GetPosition() - fp.GetPosition()
            fp.SetPosition(pcbnew.VECTOR2I(mm(anchor) - p1.x, mm(SOCKET_Y) - p1.y))
            fp.BuildCourtyardCaches()
            bb = fp.GetCourtyard(pcbnew.F_CrtYd if side == 'F' else pcbnew.B_CrtYd).BBox()
            got = sorted(pcbnew.ToMM(p.GetPosition().x) for p in pads)
            body_up = bb.GetCenter().y < pads[0].GetPosition().y
            if body_up and all(abs(a - b) < 0.02 for a, b in zip(got, want)):
                return {p.GetNumber(): pcbnew.ToMM(p.GetPosition().x) for p in pads}
    raise RuntimeError('cannot orient socket ' + fp.GetReference())

def bus_signal(header, x):
    if header == 'J6':
        return 'A%d' % J6_X.index(min(J6_X, key=lambda v: abs(v - x)))
    if header == 'J7':
        return 'DB%d' % J7_X.index(min(J7_X, key=lambda v: abs(v - x)))
    return J8_SIG[J8_X.index(min(J8_X, key=lambda v: abs(v - x)))]

USED_BUS = {'A1', 'A2', 'A14', 'A15', 'A16'}  # per-board override via board['bus']

def build_pcb(board, outdir):
    mm = pcbnew.FromMM
    b = pcbnew.BOARD()
    b.SetCopperLayerCount(2)
    nets = {}
    def net(name):
        # match KiCad's schematic net names: local labels on the root sheet get a '/'
        if name not in ('VCC', 'GND'):
            name = '/' + name
        if name not in nets:
            ni = pcbnew.NETINFO_ITEM(b, name); b.Add(ni); nets[name] = ni
        return nets[name]
    # outline
    pts = [(BOARD_X0, TOP), (BOARD_X1, TOP), (BOARD_X1, board['bottom']), (BOARD_X0, board['bottom'])]
    for i in range(4):
        s = pcbnew.PCB_SHAPE(b); s.SetShape(pcbnew.SHAPE_T_SEGMENT); s.SetLayer(pcbnew.Edge_Cuts)
        s.SetStart(pcbnew.VECTOR2I(mm(pts[i][0]), mm(pts[i][1])))
        s.SetEnd(pcbnew.VECTOR2I(mm(pts[(i + 1) % 4][0]), mm(pts[(i + 1) % 4][1])))
        s.SetWidth(mm(0.1)); b.Add(s)
    for p in board['parts']:
        fp = load_fp(fp_ref(p))
        b.Add(fp)
        fp.SetReference(p['ref']); fp.SetValue(p['value'])
        fp.SetPath(pcbnew.KIID_PATH('/' + p['uuid']))
        if p['dnp']:
            fp.SetDNP(True)
        if p['at'][0] == 'bus':
            _, xs, side = p['at']
            pad_x = place_socket(b, fp, xs, side)
            header = p['nets'].split(':')[1]
            p['nets'] = {}
            for num, x in pad_x.items():
                sig = bus_signal(header, x)
                if header != 'J6' or sig in board.get('bus', USED_BUS):
                    p['nets'][num] = sig
        else:
            x, y, rot, side = p['at']
            if not p['sym'].startswith('Mechanical:'):
                x += board.get('dx', PART_DX)   # per-board nudge; tuned so Freerouting completes
            fp.SetPosition(pcbnew.VECTOR2I(mm(x), mm(y)))
            fp.SetOrientationDegrees(rot)
            if side == 'B':
                flip(fp)
        for pad in fp.Pads():
            n = p['nets'].get(pad.GetNumber())
            if n:
                pad.SetNet(net(n))
    # ground pours, both layers
    for layer in (pcbnew.F_Cu, pcbnew.B_Cu):
        z = pcbnew.ZONE(b); z.SetLayer(layer); z.SetNet(net('GND'))
        o = z.Outline(); o.NewOutline()
        for x, y in [(BOARD_X0 + 0.5, TOP + 0.5), (BOARD_X1 - 0.5, TOP + 0.5),
                     (BOARD_X1 - 0.5, board['bottom'] - 0.5), (BOARD_X0 + 0.5, board['bottom'] - 0.5)]:
            o.Append(mm(x), mm(y))
        z.SetIsFilled(False)
        b.Add(z)
    path = outdir / (board['name'] + '.kicad_pcb')
    b.Save(str(path))
    return path

# ---------------------------------------------------------------------------
# Schematic

def kicad_lib_symbol(lib_id):
    lib, name = lib_id.split(':')
    if lib == 'vdp':
        return build_symbol(name, lib_id)
    tree = sx.parse((KSYM / (lib + '.kicad_sym')).read_text())
    for s in sx.find(tree, 'symbol'):
        if s[1] == name:
            s = copy.deepcopy(s); s[1] = lib_id
            assert not sx.first(s, 'extends'), lib_id
            return s
    raise KeyError(lib_id)

def label(name, x, y, angle):
    just = {0: 'left', 180: 'right', 90: 'left', 270: 'right'}[angle]
    return [S('label'), name, [S('at'), S(sx.fmt(x)), S(sx.fmt(y)), S(str(angle))],
            [S('effects'), [S('font'), [S('size'), S('1.27'), S('1.27')]], [S('justify'), S(just), S('bottom')]],
            [S('uuid'), U()]]

def no_connect(x, y):
    return [S('no_connect'), [S('at'), S(sx.fmt(x)), S(sx.fmt(y))], [S('uuid'), U()]]

def instance(lib_id, ref, value, fp, x, y, angle, sym_uuid, project, root, pin_numbers, dnp=False, hidden_ref=False):
    in_bom = not hidden_ref and lib_id != 'Mechanical:MountingHole'
    inst = [S('symbol'), [S('lib_id'), lib_id], [S('at'), S(sx.fmt(x)), S(sx.fmt(y)), S(str(angle))],
            [S('unit'), S('1')], [S('body_style'), S('1')], [S('exclude_from_sim'), S('no')],
            [S('in_bom'), S('yes' if in_bom else 'no')], [S('on_board'), S('no' if hidden_ref else 'yes')],
            [S('dnp'), S('yes' if dnp else 'no')], [S('uuid'), sym_uuid],
            prop('Reference', ref, x, y - 2.54, hidden_ref), prop('Value', value, x, y + 2.54, hidden_ref),
            prop('Footprint', fp or '', x, y, True), prop('Datasheet', '', x, y, True),
            prop('Description', '', x, y, True)]
    for n in pin_numbers:
        inst.append([S('pin'), n, [S('uuid'), U()]])
    inst.append([S('instances'), [S('project'), project,
                 [S('path'), '/' + root, [S('reference'), ref], [S('unit'), S('1')]]]])
    return inst

def outward_angle(lib_angle):
    return (int(round(lib_angle)) + 180) % 360

def build_schematic(board, outdir):
    root = U('root')
    project = board['name']
    lib_ids = sorted({p['sym'] for p in board['parts']} | {'power:VCC', 'power:GND', 'power:PWR_FLAG'})
    libsyms = {lid: kicad_lib_symbol(lid) for lid in lib_ids}
    items = []
    pwr = [0]
    def power(lib_id, x, y, angle):
        pwr[0] += 1
        ref = '#PWR%03d' % pwr[0] if lib_id != 'power:PWR_FLAG' else '#FLG%03d' % pwr[0]
        val = lib_id.split(':')[1]
        items.append(instance(lib_id, ref, val, '', x, y, angle, U(), project, root, ['1'], hidden_ref=True))
    # grid placement on the sheet
    col_x = [2.54 * round(v / 2.54) for v in (40, 110, 180, 250, 320, 390, 460, 530)]
    cursor = {c: 40.64 for c in range(len(col_x))}
    order = sorted(board['parts'], key=lambda p: (0 if p['sym'].startswith('vdp:') else 1, p['ref']))
    col = 0
    for p in order:
        lib = libsyms[p['sym']]
        sym_pins = sx.lib_pins(lib)
        if not sym_pins and p['sym'] != 'Mechanical:MountingHole':
            continue
        ys = [y for _, _, _, y, _, _ in sym_pins] or [0]
        height = (max(ys) - min(ys)) + 15.24
        if cursor[col] + height > 400:
            col += 1
        x = col_x[col]
        y = cursor[col] + max(ys) + 5.08
        y = round(y / 2.54) * 2.54
        cursor[col] = y - min(ys) + 12.7
        inst = instance(p['sym'], p['ref'], p['value'], fp_ref(p), x, y, 0, p['uuid'], project, root,
                        [n for n, *_ in sym_pins], dnp=p['dnp'])
        items.append(inst)
        for num, pname, px, py, pa, plen in sym_pins:
            cx, cy = round(x + px, 2), round(y - py, 2)
            netname = p['nets'].get(num)
            ang = outward_angle(pa)
            if netname in ('VCC', 'GND'):
                power('power:' + netname, cx, cy, {'VCC': {90: 0, 270: 180, 180: 90, 0: 270},
                                                    'GND': {270: 0, 90: 180, 180: 270, 0: 90}}[netname][ang])
            elif netname:
                items.append(label(netname, cx, cy, ang))
            else:
                items.append(no_connect(cx, cy))
    # PWR_FLAGs at the J3 power pins
    j3 = next(p for p in board['parts'] if p['ref'] == 'J3')
    j3i = next(i for i in items if i[0] == 'symbol' and sx.ref(i) == 'J3')
    jp = {n: pt for n, (nm, pt) in sx.inst_pins(j3i, libsyms).items()}
    for num, netname in j3['nets'].items():
        if netname in ('VCC', 'GND'):
            power('power:PWR_FLAG', jp[num][0], jp[num][1], 0)
    sch = [S('kicad_sch'), [S('version'), S('20260306')], [S('generator'), 'eeschema'],
           [S('generator_version'), '10.0'], [S('uuid'), root], [S('paper'), 'A2'],
           [S('title_block'), [S('title'), board['title']], [S('rev'), 'v2'],
            [S('comment'), S('1'), 'Generated by hw/tools/kicad/generate_vdp.py']],
           [S('lib_symbols')] + [libsyms[k] for k in lib_ids]] + items + [
           [S('sheet_instances'), [S('path'), '/', [S('page'), '1']]], [S('embedded_fonts'), S('no')]]
    path = outdir / (board['name'] + '.kicad_sch')
    path.write_text(sx.dump(sch) + '\n')
    return path

# ---------------------------------------------------------------------------

def write_project(board, outdir):
    import json
    pro = json.loads(SYSTEM_PRO.read_text())
    pro['meta']['filename'] = board['name'] + '.kicad_pro'
    ns = pro['net_settings']
    ns['netclass_patterns'] = [{'netclass': 'Power', 'pattern': 'VCC'}, {'netclass': 'Power', 'pattern': 'GND'}]
    # Denser than the system board: 0.8/0.4 mm vias fit between DIP pads
    for c in ns['classes']:
        c.update(clearance=0.2, via_diameter=0.8, via_drill=0.4)
        if c['name'] == 'Power':
            c['track_width'] = 0.4
    rules = pro['board']['design_settings']['rules']
    rules.update(min_via_diameter=0.8, min_through_hole_diameter=0.4, min_clearance=0.2)
    (outdir / (board['name'] + '.kicad_pro')).write_text(json.dumps(pro, indent=2))
    (outdir / 'sym-lib-table').write_text(
        '(sym_lib_table\n\t(version 7)\n\t(lib (name "vdp") (type "KiCad") (uri "${KIPRJMOD}/../vdp-lib/vdp.kicad_sym") (options "") (descr "mini68k VDP boards"))\n)\n')
    (outdir / 'fp-lib-table').write_text(
        '(fp_lib_table\n\t(version 7)\n\t(lib (name "vdp") (type "KiCad") (uri "${KIPRJMOD}/../vdp-lib/vdp.pretty") (options "") (descr "mini68k VDP boards"))\n)\n')

def main():
    write_symbol_lib()
    write_sdip64()
    only = sys.argv[1] if len(sys.argv) > 1 else None
    for board in (v9938_board(), tms_board()):
        if only and board['name'] != only:
            continue
        if len(sys.argv) > 2:
            board['dx'] = float(sys.argv[2])
        outdir = HW / board['name']
        outdir.mkdir(exist_ok=True)
        _uid.update(board=board['name'], n=0)
        for p in board['parts']:
            p['uuid'] = U('part:' + p['ref'])
        write_project(board, outdir)
        pcb = build_pcb(board, outdir)        # resolves socket pad -> bus signal
        sch = build_schematic(board, outdir)
        print('wrote', sch.relative_to(HW), pcb.relative_to(HW), len(board['parts']), 'parts')

if __name__ == '__main__':
    main()
