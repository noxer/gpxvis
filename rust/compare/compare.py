#!/usr/bin/env python3
"""Compare the C++ track code (gpx.cpp) with the Rust port (gpxvis-core).

Generates synthetic GPX files, builds and runs a JSON dumper for each
implementation and reports where the results differ.

Usage: compare.py [--tz ZONE ...] [--workdir DIR]
"""
import argparse
import json
import math
import os
import subprocess
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
CORE = HERE.parent / 'gpxvis-core'

POINT_COLS = ['lon', 'lat', 'x', 'y', 'h', 'len', 'duration', 'posOnTrack', 'timeOnTrack', 'timestamp']
SCALARS = ['count', 'length', 'duration', 'start']
VECTORS = ['aabb', 'by_distance', 'by_duration', 'distance_at', 'duration_at', 'dist_sqr']
# The Rust port mirrors the C++ operation order, so results are expected to be
# bit-identical. Raise these (--tol) when experimenting with numeric changes.
REL_TOL = 0.0

# Differences caused by known bugs in the C++ version, keyed by (example, field).
# '*' matches any example. The field is the part of the message before ':'.
KNOWN = {
    ('10_syntax_variants', '*'): 'C++ text search reads commented-out points and merges self-closing <trkpt/> with the next one',
    ('*', 'aabb.z'): 'C++ CAABB::Add compares z only against the max slot: min elevation stays the first value, max becomes the last one',
    ('08_time_offset', 'start'): 'C++ ignores the UTC offset in timestamps',
    ('08_time_offset', 'points.timestamp'): 'C++ ignores the UTC offset in timestamps',
}
# With TZ != UTC, C++ reads GPX (UTC) times as local standard time
KNOWN_NON_UTC = {'start', 'points.timestamp'}


def build(workdir):
    exe = workdir / 'cpp_dump'
    subprocess.run(['g++', '-O2', '-I', str(REPO / 'glad/include'), '-I', str(REPO),
                    str(HERE / 'cpp_dump.cpp'), str(REPO / 'gpx.cpp'), str(REPO / 'util.cpp'),
                    str(REPO / 'glad/src/gl.c'), '-o', str(exe)], check=True)
    subprocess.run(['cargo', 'build', '-q', '--release', '--example', 'dump'], cwd=CORE, check=True)
    return exe, CORE / 'target/release/examples/dump'


def run(exe, out, files, tz):
    env = dict(os.environ, TZ=tz)
    subprocess.run([str(exe), str(out), *map(str, files)], env=env, check=True,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    return json.loads(out.read_text())


def close(a, b, tol):
    if a is None or b is None:
        return a is b
    return a == b or abs(a - b) <= tol * max(1.0, abs(a), abs(b))


def known_reason(example, msg, tz):
    field = msg.split(':', 1)[0].split(' ')[0]
    for (ex, f), reason in KNOWN.items():
        if ex in ('*', example) and f in ('*', field):
            return reason
    if tz != 'UTC' and field in KNOWN_NON_UTC:
        return f'C++ reads UTC times as local standard time (TZ={tz})'
    return None


def diff_track(c, r):
    """Return a list of human-readable differences between two dumps of one track."""
    if c['ok'] != r['ok']:
        return [f"load result: C++ {'ok' if c['ok'] else 'rejected'}, Rust {'ok' if r['ok'] else 'rejected'}"]
    if not c['ok']:
        return []
    out = []
    for k in SCALARS:
        if not close(c[k], r[k], REL_TOL):
            out.append(f'{k}: C++ {c[k]} vs Rust {r[k]}')
    if c['count'] != r['count']:
        return out  # per-point data is not comparable
    cz, rz = [c['aabb'][i] for i in (2, 5)], [r['aabb'][i] for i in (2, 5)]
    if not all(close(x, y, REL_TOL) for x, y in zip(cz, rz)):
        out.append(f'aabb.z (min, max): C++ {cz} vs Rust {rz}')
    for k in VECTORS:
        tol = REL_TOL
        bad = [i for i, (x, y) in enumerate(zip(c[k], r[k])) if not close(x, y, tol)
               and (k, i) not in (('aabb', 2), ('aabb', 5))]
        if bad:
            out.append(f'{k}: {len(bad)} of {len(c[k])} differ, e.g. [{bad[0]}] C++ {c[k][bad[0]]} vs Rust {r[k][bad[0]]}')
    for col, name in enumerate(POINT_COLS):
        bad = [i for i, (p, q) in enumerate(zip(c['points'], r['points'])) if not close(p[col], q[col], REL_TOL)]
        if bad:
            i = bad[0]
            out.append(f"points.{name}: {len(bad)} of {c['count']} differ, e.g. point {i}: "
                       f"C++ {c['points'][i][col]} vs Rust {r['points'][i][col]}")
    for k in ('pause_default', 'pause_tight'):
        if any(not close(c[k][f], r[k][f], REL_TOL) for f in ('count', 'duration', 'distance')):
            out.append(f'{k}: C++ {c[k]} vs Rust {r[k]}')
    return out


def main():
    global REL_TOL
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--tz', nargs='+', default=['UTC', 'Europe/Berlin'],
                    help='time zones to run the C++ version in (it parses GPX times as local time)')
    ap.add_argument('--tol', type=float, default=REL_TOL, help='relative tolerance (default: exact)')
    ap.add_argument('--workdir', type=Path, help='keep generated files here (default: temp dir)')
    args = ap.parse_args()
    REL_TOL = args.tol

    with tempfile.TemporaryDirectory() as tmp:
        work = args.workdir or Path(tmp)
        work.mkdir(parents=True, exist_ok=True)
        subprocess.run(['python3', str(HERE / 'gen_examples.py'), str(work / 'gpx')], check=True)
        files = sorted((work / 'gpx').glob('*.gpx'))
        cpp_exe, rs_exe = build(work)
        rs = run(rs_exe, work / 'rust.json', files, 'UTC')

        unexpected = 0
        for tz in args.tz:
            cpp = run(cpp_exe, work / f"cpp_{tz.replace('/', '_')}.json", files, tz)
            print(f'\n## C++ run with TZ={tz}\n')
            print('| example | points (C++ / Rust) | length km (C++ / Rust) | pauses (C++ / Rust) | result |')
            print('|---|---|---|---|---|')
            details = []
            for f in files:
                c, r = cpp[str(f)], rs[str(f)]
                d = [(x, known_reason(f.stem, x, tz)) for x in diff_track(c, r)]
                n_new = sum(1 for _, why in d if why is None)
                unexpected += n_new
                if not d:
                    status = 'identical'
                elif n_new:
                    status = f'**{n_new} unexpected**, {len(d) - n_new} known'
                else:
                    status = f'{len(d)} known'
                fmt = lambda t, k: (f'{t[k]:.4f}' if isinstance(t.get(k), float) else str(t.get(k, '–')))
                pauses = lambda t: str(t['pause_default']['count']) if t['ok'] else '–'
                print(f"| {f.stem} | {fmt(c, 'count')} / {fmt(r, 'count')} | {fmt(c, 'length')} / {fmt(r, 'length')} "
                      f"| {pauses(c)} / {pauses(r)} | {status} |")
                details += [(f.stem, x) for x in d]
            if details:
                print('\nDifferences:\n')
                for name, (x, why) in details:
                    print(f'- **{name}**: {x}  \n  ' + (f'_known: {why}_' if why else '**UNEXPECTED**'))
        print(f'\n{unexpected} unexpected difference(s)')
    return 1 if unexpected else 0


if __name__ == '__main__':
    raise SystemExit(main())
