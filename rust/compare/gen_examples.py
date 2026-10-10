#!/usr/bin/env python3
"""Generate synthetic GPX files for comparing the C++ and Rust track code.

Usage: gen_examples.py OUTDIR
"""
import math
import random
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

HEADER = ('<?xml version="1.0" encoding="UTF-8"?>\n'
          '<gpx version="1.1" creator="gpxvis-compare" xmlns="http://www.topografix.com/GPX/1/1">\n'
          '<trk><name>{name}</name><trkseg>\n')
FOOTER = '</trkseg></trk>\n</gpx>\n'

T0 = datetime(2024, 5, 1, 10, 0, 0, tzinfo=timezone.utc)


def iso(t):
    return t.strftime('%Y-%m-%dT%H:%M:%SZ')


def trkpt(lat, lon, ele=None, t=None):
    s = f'  <trkpt lat="{lat:.7f}" lon="{lon:.7f}">'
    if ele is not None:
        s += f'<ele>{ele:.1f}</ele>'
    if t is not None:
        s += f'<time>{t if isinstance(t, str) else iso(t)}</time>'
    return s + '</trkpt>\n'


def write(outdir, name, body):
    (outdir / f'{name}.gpx').write_text(HEADER.format(name=name) + body + FOOTER)


def walk(lat, lon, n, step_m, heading_deg, dt_s, t0=T0, jitter=0.0, rng=None):
    """Points moving roughly along a heading at constant speed."""
    pts = []
    h = math.radians(heading_deg)
    for i in range(n):
        if rng and jitter:
            h += rng.gauss(0, jitter)
        pts.append((lat, lon, 30.0 + 5 * math.sin(i / 10), t0 + timedelta(seconds=i * dt_s)))
        lat += step_m * math.cos(h) / 111_320.0
        lon += step_m * math.sin(h) / (111_320.0 * math.cos(math.radians(lat)))
    return pts


def stay(lat, lon, n, dt_s, t0, radius_m=3.0, rng=None):
    """Points jittering around a spot (a pause)."""
    rng = rng or random.Random(0)
    return [(lat + rng.uniform(-radius_m, radius_m) / 111_320.0,
             lon + rng.uniform(-radius_m, radius_m) / (111_320.0 * math.cos(math.radians(lat))),
             30.0, t0 + timedelta(seconds=(i + 1) * dt_s)) for i in range(n)]


def body(pts):
    return ''.join(trkpt(*p) for p in pts)


def main():
    outdir = Path(sys.argv[1])
    outdir.mkdir(parents=True, exist_ok=True)
    rng = random.Random(42)

    # 1. straight line east through Berlin, 5 m every 1 s
    write(outdir, '01_straight_berlin', body(walk(52.52, 13.40, 200, 5.0, 90, 1)))

    # 2. ride with two pauses close together (should merge into one) and a separate later pause
    pts = walk(48.137, 11.575, 300, 6.0, 30, 2, jitter=0.05, rng=rng)
    lat, lon, _, t = pts[-1]
    pts += stay(lat, lon, 90, 2, t, rng=rng)                # 3 min pause
    t = pts[-1][3]
    pts += walk(lat, lon, 20, 4.0, 120, 2, t0=t + timedelta(seconds=2))[1:]   # move ~80 m
    lat, lon, _, t = pts[-1]
    pts += stay(lat, lon, 75, 2, t, rng=rng)                # 2.5 min pause, continues the first
    t = pts[-1][3]
    pts += walk(lat, lon, 400, 6.0, 200, 2, t0=t + timedelta(seconds=2), jitter=0.05, rng=rng)[1:]
    lat, lon, _, t = pts[-1]
    pts += stay(lat, lon, 120, 2, t, rng=rng)               # 4 min pause, separate
    t = pts[-1][3]
    pts += walk(lat, lon, 100, 6.0, 300, 2, t0=t + timedelta(seconds=2))[1:]
    write(outdir, '02_pauses_munich', body(pts))

    # 3. high latitude (Tromsø, ~69.6°N), going north
    write(outdir, '03_high_latitude', body(walk(69.65, 18.95, 300, 10.0, 0, 3)))

    # 4. southern / western hemisphere (Patagonia)
    write(outdir, '04_south_west', body(walk(-50.94, -73.40, 300, 8.0, 225, 2, jitter=0.1, rng=rng)))

    # 5. time warp: timestamps jump backwards in the middle
    pts = walk(51.05, 13.74, 100, 5.0, 45, 1)
    pts[50] = (*pts[50][:3], pts[50][3] - timedelta(seconds=30))
    write(outdir, '05_time_warp', body(pts))

    # 6. duplicate positions and zero-length segments
    pts = walk(50.0, 8.0, 50, 5.0, 0, 1)
    pts = [p for p in pts for _ in (0, 1)]
    pts = [(*p[:3], T0 + timedelta(seconds=i)) for i, p in enumerate(pts)]
    write(outdir, '06_duplicate_points', body(pts))

    # 7. long random walk for numeric accuracy (10k points)
    write(outdir, '07_long_random', body(walk(46.5, 7.5, 10_000, 4.0, 0, 1, jitter=0.2, rng=rng)))

    # 8. timestamps with fractional seconds and a non-UTC offset (same instants as UTC)
    pts = walk(40.4, -3.7, 60, 5.0, 90, 1)
    b = ''.join(trkpt(lat, lon, e, (t.astimezone(timezone(timedelta(hours=2)))).strftime('%Y-%m-%dT%H:%M:%S.250+02:00'))
                for lat, lon, e, t in pts)
    write(outdir, '08_time_offset', b)

    # 9. UTC wall-clock times 01:30-03:29 on 2024-03-31. Read as Europe/Berlin local
    #    time (what mktime does), 02:00-03:00 doesn't exist (CET->CEST switch).
    t0 = datetime(2024, 3, 31, 1, 30, 0, tzinfo=timezone.utc)
    write(outdir, '09_dst_switch', body(walk(52.0, 13.0, 120, 50.0, 0, 60, t0=t0)))

    # 10. GPX syntax variations that are valid XML
    pts = walk(47.0, 8.0, 6, 10.0, 90, 10)
    b = trkpt(*pts[0])
    b += f'  <trkpt lon="{pts[1][1]:.7f}" lat="{pts[1][0]:.7f}"><ele>30</ele><time>{iso(pts[1][3])}</time></trkpt>\n'
    b += f'  <!-- <trkpt lat="0" lon="0"></trkpt> commented out -->\n'
    b += trkpt(*pts[2])
    b += f'  <trkpt lat="{pts[3][0]:.7f}" lon="{pts[3][1]:.7f}"/>\n'      # self-closing, no ele/time
    b += trkpt(*pts[4])
    b += f'  <trkpt lat="{pts[5][0]:.7f}" lon="{pts[5][1]:.7f}"><time>{iso(pts[5][3])}</time><ele>31.0</ele></trkpt>\n'
    write(outdir, '10_syntax_variants', b)

    # 11. too few points
    write(outdir, '11_single_point', body(walk(45.0, 7.0, 1, 5.0, 0, 1)))


if __name__ == '__main__':
    main()
