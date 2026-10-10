//! GPS track loading and analysis (port of `gpx::CTrack`).

use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDateTime};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::aabb::Aabb;
use crate::projection::{project_mercator, projection_scale};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point {
    pub lon: f64,
    pub lat: f64,
    /// Projected position in km.
    pub x: f64,
    pub y: f64,
    pub h: f64,
    /// Length of the segment starting at this point, in km.
    pub len: f64,
    /// Duration of the segment starting at this point, in seconds.
    pub duration: f64,
    pub pos_on_track: f64,
    pub time_on_track: f64,
    /// Unix timestamp (UTC), 0 if unknown.
    pub timestamp: i64,
}

#[derive(Debug, Clone, Copy)]
struct LineSegment {
    idx: [usize; 2],
    dir: [f64; 2],
    n: [f64; 2],
    d: [f64; 2],
    inv_len: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PauseDetectorConfig {
    pub max_radius: f64,
    pub min_duration: f64,
    pub continue_radius: f64,
    pub continue_duration: f64,
}

impl Default for PauseDetectorConfig {
    fn default() -> Self {
        Self {
            max_radius: 0.025,
            min_duration: 120.0,
            continue_radius: 0.250,
            continue_duration: 300.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PauseStats {
    pub count: usize,
    pub duration: f64,
    pub distance: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("xml error: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("track contains only {0} points")]
    TooFewPoints(usize),
}

#[derive(Debug, Clone, Default)]
pub struct Track {
    points: Vec<Point>,
    segments: Vec<LineSegment>,
    aabb: Aabb,
    aabb_lon_lat: Aabb,
    total_len: f64,
    total_duration: f64,
    projection_scale: f64,
    filename: PathBuf,
}

impl Track {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        let path = path.as_ref();
        let mut track = Self::from_gpx_str(&std::fs::read_to_string(path)?)?;
        track.filename = path.to_path_buf();
        Ok(track)
    }

    /// Parse GPX data. Unlike the C++ version this uses a real (streaming)
    /// XML parser, so attribute order, namespaces and comments don't matter.
    pub fn from_gpx_str(xml: &str) -> Result<Self, LoadError> {
        let mut reader = Reader::from_str(xml);
        let mut points = Vec::new();
        let mut cur: Option<Point> = None;
        let mut text_target: Option<&'static str> = None;

        loop {
            match reader.read_event()? {
                Event::Start(e) => match e.local_name().as_ref() {
                    b"trkpt" => cur = Some(point_from_attrs(&e)),
                    b"ele" if cur.is_some() => text_target = Some("ele"),
                    b"time" if cur.is_some() => text_target = Some("time"),
                    _ => {}
                },
                Event::Empty(e) if e.local_name().as_ref() == b"trkpt" => {
                    points.push(point_from_attrs(&e))
                }
                Event::Text(t) => {
                    if let (Some(target), Some(pt)) = (text_target, cur.as_mut()) {
                        let s = t.unescape()?;
                        let s = s.trim();
                        match target {
                            "ele" => pt.h = s.parse().unwrap_or(0.0),
                            _ => pt.timestamp = parse_time(s),
                        }
                    }
                }
                Event::End(e) => match e.local_name().as_ref() {
                    b"trkpt" => points.extend(cur.take()),
                    b"ele" | b"time" => text_target = None,
                    _ => {}
                },
                Event::Eof => break,
                _ => {}
            }
        }
        Self::from_points(points)
    }

    /// Build a track from raw points; only `lon`, `lat`, `h` and `timestamp`
    /// need to be set, everything else is derived.
    pub fn from_points(mut points: Vec<Point>) -> Result<Self, LoadError> {
        if points.len() < 2 {
            return Err(LoadError::TooFewPoints(points.len()));
        }
        let mut t = Track::default();
        for p in &mut points {
            (p.x, p.y) = project_mercator(p.lon, p.lat);
            t.aabb.add([p.x, p.y, p.h]);
            t.aabb_lon_lat.add([p.lon, p.lat, p.h]);
        }
        // Same (slightly odd) averaging as the C++ code: the geo center
        // scale plus one scale per segment, divided by the point count.
        t.projection_scale = t
            .aabb_lon_lat
            .center()
            .map_or(0.0, |c| projection_scale(c[1]));
        for i in 1..points.len() {
            let (a, b) = (points[i - 1], points[i]);
            let p_scale = projection_scale(0.5 * (a.lat + b.lat));
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let len = (dx * dx + dy * dy).sqrt() * p_scale;
            t.projection_scale += p_scale;
            t.total_len += len;
            let mut dur = (b.timestamp - a.timestamp) as f64;
            if dur < 0.0 {
                // "time warp": clamp, like the C++ version
                points[i].timestamp = a.timestamp;
                dur = 0.0;
            }
            t.total_duration += dur;
            points[i - 1].len = len;
            points[i - 1].duration = dur;
            points[i].pos_on_track = t.total_len;
            points[i].time_on_track = t.total_duration;
        }
        t.projection_scale /= points.len() as f64;
        t.segments = points
            .windows(2)
            .enumerate()
            .map(|(i, w)| line_segment(i, &w[0], &w[1]))
            .collect();
        t.points = points;
        Ok(t)
    }

    pub fn points(&self) -> &[Point] {
        &self.points
    }
    pub fn len(&self) -> usize {
        self.points.len()
    }
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
    pub fn length_km(&self) -> f64 {
        self.total_len
    }
    pub fn duration_s(&self) -> f64 {
        self.total_duration
    }
    pub fn aabb(&self) -> &Aabb {
        &self.aabb
    }
    pub fn aabb_lon_lat(&self) -> &Aabb {
        &self.aabb_lon_lat
    }
    pub fn filename(&self) -> &Path {
        &self.filename
    }
    pub fn start_timestamp(&self) -> i64 {
        self.points.first().map_or(0, |p| p.timestamp)
    }

    /// Vertex data for the GPU, transformed into normalized space.
    pub fn vertices_2d(&self, origin: [f64; 2], scale: [f64; 2]) -> Vec<[f32; 2]> {
        self.points
            .iter()
            .map(|p| {
                [
                    ((p.x - origin[0]) * scale[0]) as f32,
                    ((p.y - origin[1]) * scale[1]) as f32,
                ]
            })
            .collect()
    }

    /// Fractional point index at the given distance (km) along the track.
    pub fn point_by_distance(&self, distance: f64) -> f32 {
        self.point_by(distance, self.total_len, |p| p.pos_on_track, |p| p.len)
    }

    /// Fractional point index at the given time (s) since the track start.
    pub fn point_by_duration(&self, duration: f64) -> f32 {
        self.point_by(
            duration,
            self.total_duration,
            |p| p.time_on_track,
            |p| p.duration,
        )
    }

    fn point_by(
        &self,
        v: f64,
        total: f64,
        key: impl Fn(&Point) -> f64,
        span: impl Fn(&Point) -> f64,
    ) -> f32 {
        let cnt = self.points.len();
        if cnt < 2 || v <= 0.0 {
            return 0.0;
        }
        if v >= total {
            return (cnt - 1) as f32;
        }
        // index of the last point with key < v (keys are monotonic)
        let i = self
            .points
            .partition_point(|p| key(p) < v)
            .saturating_sub(1);
        let p = &self.points[i];
        let rel = if span(p) > 0.0 {
            (((v - key(p)) / span(p)) as f32).clamp(0.0, 0.999999)
        } else {
            0.0
        };
        i as f32 + rel
    }

    pub fn distance_at(&self, anim_pos: f32) -> f64 {
        self.value_at(anim_pos, self.total_len, |p| (p.pos_on_track, p.len))
    }

    pub fn duration_at(&self, anim_pos: f32) -> f64 {
        self.value_at(anim_pos, self.total_duration, |p| {
            (p.time_on_track, p.duration)
        })
    }

    fn value_at(&self, anim_pos: f32, total: f64, f: impl Fn(&Point) -> (f64, f64)) -> f64 {
        if self.points.len() < 2 {
            return 0.0;
        }
        match self.points.get(anim_pos as usize) {
            Some(p) if anim_pos >= 0.0 => {
                let (base, span) = f(p);
                base + anim_pos.fract() as f64 * span
            }
            _ => total,
        }
    }

    /// Squared distance (in projected km) from (x, y) to the closest point on the track.
    pub fn distance_sqr_to(&self, x: f64, y: f64) -> f64 {
        self.segments
            .iter()
            .map(|ls| {
                let d = x * ls.n[0] + y * ls.n[1];
                let a = &self.points[ls.idx[0]];
                let b = &self.points[ls.idx[1]];
                let (px, py) = if d <= ls.d[0] {
                    (a.x, a.y)
                } else if d >= ls.d[1] {
                    (b.x, b.y)
                } else {
                    let t = (d - ls.d[0]) * ls.inv_len;
                    (a.x + t * ls.dir[0], a.y + t * ls.dir[1])
                };
                (x - px).powi(2) + (y - py).powi(2)
            })
            .fold(f64::MAX, f64::min)
    }

    /// Port of `CTrack::GetPauseStats`, including its quirk of ignoring the last point.
    pub fn pause_stats(&self, cfg: &PauseDetectorConfig) -> PauseStats {
        let mut stats = PauseStats::default();
        let Some(cnt) = self.points.len().checked_sub(1) else {
            return stats;
        };
        let radius_sqr = cfg.max_radius * cfg.max_radius;
        let mut last_pause_end: Option<usize> = None;
        let mut i = 0;
        while i < cnt {
            let p = &self.points[i];
            let mut sum = [p.x, p.y];
            let mut n = 1usize;
            for q in &self.points[i + 1..cnt] {
                let dx = q.x - sum[0] / n as f64;
                let dy = q.y - sum[1] / n as f64;
                if dx * dx + dy * dy > radius_sqr {
                    break;
                }
                sum[0] += q.x;
                sum[1] += q.y;
                n += 1;
            }
            if n > 1 {
                let j = i + n - 1;
                let q = &self.points[j];
                let dur = q.time_on_track - p.time_on_track;
                if dur >= cfg.min_duration {
                    stats.duration += dur;
                    stats.distance += q.pos_on_track - p.pos_on_track;
                    let continues_last = last_pause_end.is_some_and(|r| {
                        let r = &self.points[r];
                        p.time_on_track - r.time_on_track <= cfg.continue_duration
                            && (r.x - p.x).powi(2) + (r.y - p.y).powi(2)
                                <= cfg.continue_radius.powi(2)
                    });
                    if !continues_last {
                        stats.count += 1;
                    }
                    last_pause_end = Some(j);
                }
            }
            i += n;
        }
        stats
    }

    /// Same identity check as `gpx::IsEqual`, used for duplicate removal.
    pub fn same_points(&self, other: &Track) -> bool {
        self.points.len() == other.points.len()
            && self
                .points
                .iter()
                .zip(&other.points)
                .all(|(a, b)| a.lon == b.lon && a.lat == b.lat && a.timestamp == b.timestamp)
    }
}

fn point_from_attrs(e: &BytesStart) -> Point {
    let mut p = Point::default();
    for attr in e.attributes().flatten() {
        let v = std::str::from_utf8(&attr.value)
            .ok()
            .and_then(|s| s.trim().parse().ok());
        match attr.key.local_name().as_ref() {
            b"lat" => p.lat = v.unwrap_or(0.0),
            b"lon" => p.lon = v.unwrap_or(0.0),
            _ => {}
        }
    }
    p
}

/// GPX timestamps are ISO 8601 / RFC 3339, normally UTC ("Z").
/// The C++ version feeds them into `mktime`, i.e. interprets them as local time;
/// here they are converted to proper Unix timestamps.
fn parse_time(s: &str) -> i64 {
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.timestamp())
        .or_else(|_| {
            NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
                .map(|t| t.and_utc().timestamp())
        })
        .unwrap_or(0)
}

fn line_segment(i: usize, a: &Point, b: &Point) -> LineSegment {
    let dir = [b.x - a.x, b.y - a.y];
    let len_sqr = dir[0] * dir[0] + dir[1] * dir[1];
    let (n, inv_len) = if len_sqr < 1.0e-6 {
        ([1.0, 0.0], 1.0)
    } else {
        let len = len_sqr.sqrt();
        ([dir[0] / len, dir[1] / len], 1.0 / len)
    };
    LineSegment {
        idx: [i, i + 1],
        dir,
        n,
        d: [a.x * n[0] + a.y * n[1], b.x * n[0] + b.y * n[1]],
        inv_len,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GPX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="test" xmlns="http://www.topografix.com/GPX/1/1">
  <trk><name>t</name><trkseg>
    <trkpt lat="52.5000" lon="13.4000"><ele>34.0</ele><time>2024-05-01T10:00:00Z</time></trkpt>
    <!-- attribute order swapped on purpose -->
    <trkpt lon="13.4100" lat="52.5000"><ele>35.0</ele><time>2024-05-01T10:02:00Z</time></trkpt>
    <trkpt lat="52.5000" lon="13.4200"><time>2024-05-01T10:04:00.500Z</time><ele>36.5</ele></trkpt>
  </trkseg></trk>
</gpx>"#;

    #[test]
    fn parses_points() {
        let t = Track::from_gpx_str(GPX).unwrap();
        assert_eq!(t.len(), 3);
        assert_eq!(t.points()[1].lon, 13.41);
        assert_eq!(t.points()[2].h, 36.5);
        assert_eq!(t.start_timestamp(), 1714557600);
        assert_eq!(t.duration_s(), 240.0);
        // 0.02° of longitude at 52.5°N is ~1.357 km
        assert!((t.length_km() - 1.357).abs() < 0.01, "{}", t.length_km());
    }

    #[test]
    fn rejects_short_tracks() {
        assert!(matches!(
            Track::from_gpx_str("<gpx/>"),
            Err(LoadError::TooFewPoints(0))
        ));
    }

    #[test]
    fn anim_positions() {
        let t = Track::from_gpx_str(GPX).unwrap();
        assert_eq!(t.point_by_duration(0.0), 0.0);
        assert!((t.point_by_duration(60.0) - 0.5).abs() < 1e-6);
        assert!((t.point_by_duration(180.0) - 1.5).abs() < 1e-3);
        assert_eq!(t.point_by_duration(1e9), 2.0);
        let half = t.length_km() * 0.5;
        assert!((t.point_by_distance(half) - 1.0).abs() < 1e-3);
        assert!((t.distance_at(t.point_by_distance(half)) - half).abs() < 1e-3);
    }

    #[test]
    fn closest_distance() {
        let t = Track::from_gpx_str(GPX).unwrap();
        let p = t.points()[1];
        assert!(t.distance_sqr_to(p.x, p.y) < 1e-12);
        assert!((t.distance_sqr_to(p.x, p.y + 1.0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn detects_pause() {
        let mk = |lat: f64, t: i64| Point {
            lon: 13.4,
            lat,
            timestamp: t,
            ..Default::default()
        };
        let mut pts = vec![mk(52.5, 0), mk(52.501, 60)];
        // stand still for 5 minutes
        pts.extend((1..=5).map(|i| mk(52.501, 60 + i * 60)));
        pts.extend([mk(52.502, 420), mk(52.503, 480)]);
        let t = Track::from_points(pts).unwrap();
        let s = t.pause_stats(&PauseDetectorConfig::default());
        assert_eq!(s.count, 1);
        assert_eq!(s.duration, 300.0);
    }
}
