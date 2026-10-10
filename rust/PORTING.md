# Porting gpxvis to Rust — assessment

This document reviews the current C++ code base and lays out how a Rust port
could look. A small proof of concept of the platform‑independent core lives in
[`gpxvis-core/`](./gpxvis-core) (`cargo test` inside that directory).

## 1. What the project consists of today

Own code: about 6,000 lines of C++ plus 11 GLSL shaders. Vendored code (glad,
stb_image_write, Dear ImGui as a submodule) is not counted.

| File | Lines | Role | Platform / GL coupling |
|---|---:|---|---|
| `gpx.cpp/.h` | ~790 | GPX loading (string search, no XML parser), Mercator projection, track length and duration, lookups from animation position to point, pause detector, distance from a point to a track | none (just `GLfloat` in one signature) |
| `util.cpp/.h` | ~720 | Logging, GL error/debug helpers, shader loading, AABB, `durationToString`, Win32 UTF‑8 wrappers | mostly GL / Win32 |
| `img.cpp/.h` | ~170 | RGB buffer, writes TGA/PNG/BMP/JPG through stb | none |
| `vis.cpp/.h` (`CVis`) | ~700 | Offscreen GL renderer: 5 FBOs (R32F history accumulation, R8 neighbourhood, RGBA8 track and final), UBOs, an SSBO holding the polyline | OpenGL 4.5 (DSA bits: `glBindTextures`, `glGetTextureImage`, `glBufferStorage`) |
| `vis.cpp/.h` (`CAnimController`) | ~1,500 | Track list, sorting and dedup, animation state machine (track / track‑accu / history modes, fade in/out, day/week/month/year accumulation), frame info text, CSV stats | only through `CVis` |
| `mainapp.cpp` | ~2,230 | GLFW window, input, zoom and pan, frame export loop, command‑line parsing, **about 450 ImGui calls** (Main Controls, Track Manager, Info Window) | GLFW, ImGui, GL |
| `filedialog.cpp/.h` | ~510 | Hand‑written ImGui file and directory picker with `dirent` / `FindFirstFileW` | ImGui, OS |
| `shaders/*` | ~370 | Line expansion in the vertex shader (`gl_VertexID / 18`, points read from an SSBO), additive and MAX blending, gradient colouring of the history | GLSL 4.30 |

How it works, in short: each track becomes a polyline in an SSBO. The track
shader expands every segment into 18 vertices of quads and caps. History is
accumulated additively into an `R32F` texture. A final full‑screen pass
(`blend.fs`) turns the accumulated density into a colour gradient and blends
the animated track over it. Frames are read back with `glGetTextureImage` and
written as image sequences, then turned into video with ffmpeg (`ffmpeg.txt`).

### Issues worth fixing along the way

These are in the current code. A port fixes most of them for free.

* **GPX parsing** uses `strstr` on raw text. It breaks on prefixed
  elements (`<gpx:trkpt>`) and on self‑closing `<trkpt …/>`, which swallow
  the following point. It also picks up `<trkpt>` elements inside comments.
* **Timestamps**: GPX times are UTC (`...Z`), but `getTime` passes them to
  `mktime`, which reads them as local *standard* time (`tm_isdst = 0`), and
  any `+hh:mm` offset is ignored. Durations stay correct (the shift is
  constant, even across DST switches, as the comparison in
  [`compare/`](./compare) confirmed), but absolute times are off by the zone
  offset. Dates shown and day/week buckets are therefore effectively UTC
  dates, which differ from local dates for tracks recorded near midnight.
* **`CAABB::Add`** (`util.cpp`) compares z against `aabb[5]` in both
  branches: the minimum elevation stays the first point's value and the
  maximum becomes the last point's. Harmless today, because only x/y are
  used for rendering.
* **`img.cpp`**: `CImg::Write` returns `true` even when stb fails.
* **`filedialog.cpp`**: `realpath` into a fixed `PATH_MAX` buffer, and the
  return value is ignored.
* **`GetPauseStats`** never looks at the last point, because the loop runs to
  `cnt-1`. The PoC keeps this behaviour on purpose and documents it.
* **Fixed `char` buffers** (`mysnprintf` into 64‑byte buffers for frame info)
  can truncate output.
* **macOS can't run it at all**: Apple's OpenGL stops at 4.1, and the
  renderer needs 4.3+ for SSBOs and 4.5 for DSA.

## 2. Mapping to the Rust ecosystem

| Concern | C++ today | Rust candidate(s) | Notes |
|---|---|---|---|
| GPX parsing | `strstr` | `quick-xml` (used in the PoC) or the `gpx` crate | `gpx` gives typed GPX 1.0/1.1. `quick-xml` is lighter and streams. |
| Time | `mktime`/`localtime` | `chrono` (or `jiff`) | Use UTC for durations and `Local`/`TimeZone` only for the day/week/month/year bucketing |
| Errors / logging | `info`/`warn` printf | `thiserror`, `log` + `env_logger` | |
| Images | stb_image_write | `image` | TGA/PNG/BMP/JPEG all supported. Could also pipe raw frames straight into `ffmpeg` stdin. |
| CLI | hand‑rolled `strcmp` | `clap` (derive) | Keep the existing flag names for compatibility |
| Window + input | GLFW | `winit` (or `glfw` crate for a 1:1 port) | |
| GL access | glad | `glow` (+ `glutin`) | Lets the shaders be reused unchanged |
| GPU, portable | — | `wgpu` | Vulkan / Metal / DX12 / GL, so macOS works and headless rendering is possible |
| GUI | Dear ImGui | `egui` (`eframe`, `egui-wgpu` / `egui_glow`) | `imgui-rs` is poorly maintained, so egui is the practical choice. It also has a similar immediate‑mode style. |
| File dialogs | own ImGui dialog + `dirent`/Win32 | `rfd` (native dialogs) or `egui-file-dialog` | Removes all of `filedialog.cpp` |
| Parallel loading | — | `rayon` | Loading hundreds of GPX files in parallel is a cheap win |

All the `#ifdef WIN32` code (wide strings, `wmain`, `fopen_wrapper`,
`sprintf_s`) goes away, because Rust strings and paths handle this already.

## 3. Rendering: two viable routes

### Route A — `glow` + OpenGL 4.5 (most faithful, fastest to get running)

* Shaders, FBO layout and blend state carry over 1:1. `CVis` becomes a
  struct with `glow::Context` calls replacing the `gl*` calls, plus `Drop` for
  cleanup instead of `DropGL()`.
* The window comes from `glutin` + `winit` with a 4.5 core context. Use
  `egui_glow` for the GUI, or the `glfw` crate plus a GL egui painter.
* Downsides: still no macOS, and GL calls stay `unsafe`.

### Route B — `wgpu` (recommended long term)

Every feature the renderer needs exists in wgpu:

| GL feature in use | wgpu equivalent |
|---|---|
| SSBO read in vertex shader + `gl_VertexID` expansion | `var<storage, read>` + `@builtin(vertex_index)`. Works on all native backends. |
| `GL_MAX` blending (track, neighbourhood) | `BlendOperation::Max` |
| Additive blending into `R32F` | Needs `Features::FLOAT32_BLENDABLE` (widely supported on desktop). Fallback: `R16Float`, which is always blendable, or a compute pass doing the accumulation. |
| `GL_LINE_STRIP` (simple history lines) | `PrimitiveTopology::LineStrip` (1px only, same as today in core GL) |
| Depth buffer for the track pass | Ordinary `Depth32Float` attachment |
| UBOs (`std140`) | Uniform buffers, built with `bytemuck` / `encase` |
| `glGetTextureImage` readback | `copy_texture_to_buffer` + `map_async`. Rows must be padded to 256 bytes. |
| Explicit `layout(location=…)` sampler uniforms + `glUniform1f` | Bind groups + a small uniform / push constant |

Shaders need rewriting in WGSL, about 370 lines and mostly mechanical. naga's
GLSL frontend can't take the current shaders as they are, because of
`layout(location, binding)` on plain uniforms. Either rewrite them by hand,
or adapt them to Vulkan‑style GLSL first.

What Route B adds:

* macOS / Metal support
* Truly **headless frame export** (`--output-frames` without a window), which
  makes server or CI rendering possible
* `eframe` with the wgpu backend gives window, input and GUI in one crate
* No `unsafe` in the renderer

## 4. Proposed crate layout

```
rust/
  gpxvis-core/     # tracks, projection, pause stats, AABB, sorting/dedup, CSV  (no GPU)   <- PoC exists
  gpxvis-anim/     # CAnimController state machine, driving a `Renderer` trait   (no GPU)
  gpxvis-render/   # CVis equivalent (wgpu or glow), headless-capable
  gpxvis/          # binary: clap CLI, eframe/egui GUI, frame export
```

Splitting the animation controller from the renderer through a small trait
(`set_polygon`, `draw_track(up_to)`, `add_history`, `clear_history`,
`mix_track_and_background`, …) makes the state machine unit‑testable with a
mock renderer. That is where most of the regressions in recent commits were
(see `git log`: "fix track_accu…", "fix pause in track_accu mode", …).

## 5. Effort estimate

| Part | Est. Rust LOC | Effort | Risk |
|---|---:|---|---|
| core (gpx, projection, stats, AABB, CSV) | 500 | 1–2 days (PoC covers ~70%) | low |
| animation controller | 900 | 3–5 days | **medium**: subtle phase / fade / accumulation behaviour must match |
| renderer (glow) / (wgpu) | 600 / 900 | 2–3 / 4–6 days | low / medium (`R32F` blending, readback) |
| GUI (3 windows, ~450 ImGui calls → egui) | 1,200 | 4–6 days | low, just volume |
| CLI, frame export, misc | 300 | 1 day | low |
| **Total** | **~3,500–3,800** | **~2.5–4 weeks** for one developer | |

The Rust version should come out at roughly 60% of the C++ size: no file
dialog, no Win32 shims, no manual GL resource bookkeeping, and no printf
buffer handling.

## 6. Suggested plan

1. **Core first** (started here). Finish it: sorting and dedup, CSV output
   (`StatsToCSV`), `durationToString`, local‑time bucketing helpers. Add
   golden tests by running a few real GPX files through both the C++ and the
   Rust version and comparing length, duration and pause stats.
2. **Headless renderer + CLI** (`--output-frames`, `--output-stats`, the
   animation flags). This is already useful on its own for producing videos,
   and lets frames be compared pixel by pixel against the C++ output.
3. **Animation controller** behind the renderer trait, with mock‑renderer
   tests for each mode (track, track‑accu with count/day/week/month/year,
   history).
4. **Interactive app**: eframe + egui windows, zoom and pan, closest‑track
   picking (`distance_sqr_to` is already ported), `rfd` dialogs.
5. Retire the C++ version once frame output matches.

A cheaper middle ground is also possible: port only `gpx.cpp` to Rust as a
static library behind a C ABI (`cbindgen`) and link it into the existing C++
app. That gets the robust XML parsing and correct time handling without
touching the renderer or GUI. With this little non‑GUI logic, though, a full
port is cleaner than living with a mixed build.

## 7. What the PoC shows

`gpxvis-core` (~580 lines including tests) ports:

* Mercator projection and its inverse, projection scale
* GPX loading with a real streaming XML parser (`quick-xml`): handles
  namespaces, attribute order, comments, fractional seconds, and proper UTC
  timestamps through `chrono`
* Per‑segment length and duration, "time warp" clamping
* `point_by_distance` / `point_by_duration` (binary search via
  `partition_point`), `distance_at` / `duration_at`
* Distance from a point to a track for closest‑track picking
* The pause detector, with the same behaviour as the C++ code
* GPU vertex generation (`vertices_2d`)

Tests: `cargo test` passes 6 tests covering parsing, projection round trip,
animation position lookups, point‑to‑track distance and pause detection.
`cargo clippy --all-targets` is clean.

Comparison with the C++ code: [`compare/`](./compare) runs both
implementations on 11 synthetic GPX files. On well‑formed input the Rust
results are **bit‑identical** to the C++ ones (every point, length, duration,
lookup, distance and pause statistic). All remaining differences come from
known C++ bugs listed in section 1.
