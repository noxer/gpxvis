//! Proof-of-concept port of the platform-independent core of gpxvis
//! (`gpx.cpp` and the AABB part of `util.cpp`) to Rust.
//!
//! Nothing here touches OpenGL, so it can be shared by a GL renderer,
//! a wgpu renderer, a headless frame exporter or a stats-only CLI.

pub mod aabb;
pub mod projection;
pub mod track;

pub use aabb::Aabb;
pub use track::{LoadError, PauseDetectorConfig, PauseStats, Point, Track};
