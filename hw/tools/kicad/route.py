#!/usr/bin/env python3
"""Autoroute a KiCad board with Freerouting and import the result.

Run with KiCad's bundled Python (needs pcbnew):
  python3 route.py BOARD.kicad_pcb WORKDIR [passes] [lock|nolock] [attempts]

- Routes from a zone-less copy of the board (with the project's .kicad_pro
  beside it so net classes and via sizes apply), so power pins get real
  tracks; the session is then imported into the real board and its zones are
  refilled.
- 'lock' keeps existing copper fixed; 'nolock' lets the router rework it.
- Freerouting stops when its score plateaus and results vary run to run, so
  it makes `attempts` fresh tries, keeps the best, then continues from that
  result while it keeps improving.
"""
import os, re, shutil, subprocess, sys
import pcbnew

JAR = '/Applications/FreeRouting.app/Contents/MacOS/freerouting-executable.jar'


def export_dsn(pcbp, work, lock_existing):
    tmp = os.path.join(work, 'nozones.kicad_pcb')
    shutil.copy(pcbp, tmp)
    pro = os.path.splitext(pcbp)[0] + '.kicad_pro'
    if os.path.exists(pro):
        shutil.copy(pro, os.path.splitext(tmp)[0] + '.kicad_pro')
    a = pcbnew.LoadBoard(tmp)
    for z in list(a.Zones()):
        if not z.GetIsRuleArea():
            a.Delete(z)
    if lock_existing:
        for t in a.GetTracks():
            t.SetLocked(True)
    dsn = os.path.join(work, 'route.dsn')
    assert pcbnew.ExportSpecctraDSN(a, dsn), 'DSN export failed'
    # Freerouting rejects zero-length polylines: KiCad writes round "oval" pads
    # as a zero-length path (make them circles) and can emit sub-resolution wires.
    txt = open(dsn).read()
    txt = re.sub(r'\(path (\S+) ([\d.]+)  0 0  0 0\)', r'(circle \1 \2)', txt)

    def keep_wire(m):
        nums = [float(x) for x in m.group(1).split()]
        return m.group(0) if len(set(zip(nums[0::2], nums[1::2]))) >= 2 else ''
    txt = re.sub(r'\(wire \(path \S+ [\d.]+\s+([-\d.\s]+?)\).*', keep_wire, txt)
    open(dsn, 'w').write(txt)
    return dsn


def run_freerouting(dsn, ses, work, passes):
    if os.path.exists(ses):
        os.remove(ses)
    r = subprocess.run(['java', '-Djava.awt.headless=true', '-jar', JAR, '-de', dsn, '-do', ses, '-mp', str(passes),
                        '--gui.enabled=false', '--usage_and_diagnostic_data.disable_analytics=true',
                        '--profile.allow_telemetry=false', '--logging.file.location=' + os.path.join(work, 'fr.log')],
                       capture_output=True, text=True, timeout=3000)
    out = r.stdout + r.stderr
    m = re.findall(r'final score: ([\d.]+) \((\d+) unrouted and (\d+) violations\)', out)
    if not m or not os.path.exists(ses):
        print('\n'.join(l for l in out.splitlines() if 'rror' in l or 'xception' in l)[-800:])
        return None
    score, unrouted, viol = m[-1]
    return int(unrouted), int(viol), float(score)


def main():
    pcbp, work = sys.argv[1], sys.argv[2]
    passes = int(sys.argv[3]) if len(sys.argv) > 3 else 100
    lock_existing = (sys.argv[4] if len(sys.argv) > 4 else 'lock') == 'lock'
    attempts = int(sys.argv[5]) if len(sys.argv) > 5 else 1
    os.makedirs(work, exist_ok=True)
    start = os.path.join(work, 'start.kicad_pcb')
    shutil.copy(pcbp, start)
    best = None
    for i in range(attempts):
        dsn = export_dsn(start, work, lock_existing)
        ses = os.path.join(work, 'try%d.ses' % i)
        res = run_freerouting(dsn, ses, work, passes)
        print('attempt %d: %s' % (i + 1, res))
        if res and (best is None or (res[0], res[1], -res[2]) < (best[0][0], best[0][1], -best[0][2])):
            best = (res, ses)
        if best and best[0][0] == 0 and best[0][1] == 0:
            break
    assert best, 'no successful route'
    b = pcbnew.LoadBoard(pcbp)
    assert pcbnew.ImportSpecctraSES(b, best[1]), 'SES import failed'
    b.Save(pcbp)
    # continue from the best result while it improves
    current = best[0]
    for n in range(4):
        if current[0] == 0:
            break
        dsn = export_dsn(pcbp, work, False)
        ses = os.path.join(work, 'cont%d.ses' % n)
        res = run_freerouting(dsn, ses, work, passes)
        print('continue %d: %s' % (n + 1, res))
        if not res or (res[0], res[1]) >= (current[0], current[1]):
            break
        b = pcbnew.LoadBoard(pcbp)
        assert pcbnew.ImportSpecctraSES(b, ses), 'SES import failed'
        b.Save(pcbp)
        current = res
    b = pcbnew.LoadBoard(pcbp)
    filler = pcbnew.ZONE_FILLER(b)
    filler.Fill(b.Zones())
    b.Save(pcbp)
    print('final: %d unrouted, %d violations, %d tracks' % (current[0], current[1], len(b.GetTracks())))


if __name__ == '__main__':
    main()
