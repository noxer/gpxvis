//! Dump the results of the Rust track code as JSON, in the same format as
//! `../compare/cpp_dump.cpp`. Usage: `cargo run --example dump -- OUT.json FILE.gpx...`

use gpxvis_core::{PauseDetectorConfig, Track};
use serde_json::{Map, Value, json};

const FRACTIONS: [f64; 11] = [-0.1, 0.0, 0.001, 0.1, 0.25, 0.5, 0.75, 0.9, 0.999, 1.0, 1.1];
const QUERY_OFFSETS: [[f64; 2]; 5] = [
    [0.0, 0.0],
    [0.01, 0.0],
    [0.0, -0.05],
    [0.3, 0.2],
    [-2.0, 5.0],
];

fn dump(t: &Track) -> Value {
    let p = t.points();
    let n = p.len();
    let mid = p[n / 2];
    let tight = PauseDetectorConfig {
        max_radius: 0.010,
        min_duration: 60.0,
        continue_radius: 0.050,
        continue_duration: 60.0,
    };
    let pause = |cfg: &PauseDetectorConfig| {
        let s = t.pause_stats(cfg);
        json!({"count": s.count, "duration": s.duration, "distance": s.distance})
    };
    let anim_pos = |f: f64| (f * (n - 1) as f64) as f32;
    let bb = t.aabb();
    json!({
        "ok": true,
        "count": n,
        "length": t.length_km(),
        "duration": t.duration_s(),
        "start": t.start_timestamp(),
        "aabb": [bb.min[0], bb.min[1], bb.min[2], bb.max[0], bb.max[1], bb.max[2]],
        "points": p.iter().map(|q| json!([q.lon, q.lat, q.x, q.y, q.h, q.len, q.duration,
                                           q.pos_on_track, q.time_on_track, q.timestamp as f64])).collect::<Vec<_>>(),
        "by_distance": FRACTIONS.map(|f| t.point_by_distance(f * t.length_km()) as f64),
        "by_duration": FRACTIONS.map(|f| t.point_by_duration(f * t.duration_s()) as f64),
        "distance_at": FRACTIONS.map(|f| t.distance_at(anim_pos(f))),
        "duration_at": FRACTIONS.map(|f| t.duration_at(anim_pos(f))),
        "dist_sqr": QUERY_OFFSETS.map(|o| t.distance_sqr_to(mid.x + o[0], mid.y + o[1])),
        "pause_default": pause(&PauseDetectorConfig::default()),
        "pause_tight": pause(&tight),
    })
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().expect("usage: dump OUT.json FILE.gpx...");
    let mut result = Map::new();
    for file in args {
        let entry = match Track::load(&file) {
            Ok(t) => dump(&t),
            Err(e) => {
                eprintln!("{file}: {e}");
                json!({"ok": false})
            }
        };
        result.insert(file, entry);
    }
    std::fs::write(out, Value::Object(result).to_string()).expect("can't write output");
}
