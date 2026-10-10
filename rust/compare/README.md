# C++ vs. Rust comparison

Runs the original track code (`gpx.cpp`, `util.cpp`) and the Rust port
(`../gpxvis-core`) on synthetic GPX files and diffs the results.

```sh
python3 compare.py                       # C++ runs with TZ=UTC and TZ=Europe/Berlin
python3 compare.py --tz America/New_York --workdir /tmp/gpxcmp   # keep generated files
```

Needs `g++` and `cargo`. GLFW and a GL context are **not** needed: the C++
driver links glad but never calls GL.

| File | Purpose |
|---|---|
| `gen_examples.py` | Writes the synthetic GPX files |
| `cpp_dump.cpp` | Loads tracks with `gpx::CTrack` and dumps results as JSON |
| `../gpxvis-core/examples/dump.rs` | Same for the Rust `Track` |
| `compare.py` | Builds both, runs them, prints a Markdown report. Exits non‑zero on any unexpected difference. |

Values compared for each track: every point (lon, lat, projected x/y,
elevation, segment length and duration, distance and time along the track,
timestamp), total length and duration, start time, bounding box, distance and
time → point lookups at 11 fractions (including out of range), point → distance
and time at 11 positions, squared distance from 5 query points, and pause
statistics with the default and a tighter detector config.

## Examples

| Example | What it exercises |
|---|---|
| `01_straight_berlin` | Simple baseline |
| `02_pauses_munich` | Two pauses close together (must merge into one) plus a separate one |
| `03_high_latitude` | Projection scale at ~69.6°N |
| `04_south_west` | Negative lat/lon |
| `05_time_warp` | Timestamp going backwards (clamping) |
| `06_duplicate_points` | Zero‑length segments |
| `07_long_random` | 10,000 point random walk (accumulated rounding) |
| `08_time_offset` | `+02:00` offsets and fractional seconds |
| `09_dst_switch` | UTC times that fall into the CET→CEST gap when read as local time |
| `10_syntax_variants` | Swapped attributes, commented‑out point, self‑closing `<trkpt/>`, swapped `ele`/`time` |
| `11_single_point` | Must be rejected |

## Results

Comparison is exact (`--tol 0`). The Rust code mirrors the C++ operation
order (e.g. `lon * PI / 180.0` rather than `to_radians()`, `sqrt(dx*dx+dy*dy)`
rather than `hypot`) to make this possible. With `to_radians`/`hypot` the
results differed by up to ~2e‑8 relative.

* **01–07, 09** (`TZ=UTC`): bit‑identical except for the elevation bounds.
* **Elevation bounds** (all files): differ because of a C++ bug in
  `CAABB::Add`, which compares z against the max slot in both branches.
* **08**: C++ ignores the `+02:00` offset, so its timestamps are 2 h later.
  Durations agree.
* **10**: C++ adds the commented‑out point at (0, 0), giving a ~11,000 km
  track, and merges the self‑closing point with the following one. Rust reads
  the file correctly (0.05 km).
* **11**: both reject it.
* **`TZ=Europe/Berlin`**: C++ timestamps shift by a constant −1 h, because
  `mktime` with `tm_isdst = 0` reads times as local standard time. Durations,
  lengths and pauses still agree, even across the DST switch in 09.
