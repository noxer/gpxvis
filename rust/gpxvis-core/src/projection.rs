//! Mercator projection with km as units (port of `gpx::projectMercator` & co).

use std::f64::consts::{FRAC_PI_4, PI};

/// Equator length in km.
const MERCATOR_SCALE_X: f64 = 40075.0167;
/// Meridian length according to the WGS84 flattening factor.
const MERCATOR_SCALE_Y: f64 = 39940.65274158;

pub fn project_mercator(lon: f64, lat: f64) -> (f64, f64) {
    let lon = lon.to_radians();
    let lat = lat.to_radians();
    let x = (MERCATOR_SCALE_X * (lon + PI)) / (2.0 * PI);
    let y = (MERCATOR_SCALE_Y * (PI + (FRAC_PI_4 + lat * 0.5).tan().ln())) / (2.0 * PI);
    (x, y)
}

pub fn unproject_mercator(x: f64, y: f64) -> (f64, f64) {
    let lon = ((x * 2.0 * PI) / MERCATOR_SCALE_X) - PI;
    let lat = 2.0 * ((((y * 2.0 * PI) / MERCATOR_SCALE_Y) - PI).exp().atan() - FRAC_PI_4);
    (lon.to_degrees(), lat.to_degrees())
}

/// Scale factor of the projection at the given latitude.
pub fn projection_scale(lat: f64) -> f64 {
    lat.clamp(-89.9, 89.9).to_radians().cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        for &(lon, lat) in &[(0.0, 0.0), (13.4, 52.5), (-122.4, 37.8), (151.2, -33.9)] {
            let (x, y) = project_mercator(lon, lat);
            let (lon2, lat2) = unproject_mercator(x, y);
            assert!((lon - lon2).abs() < 1e-9 && (lat - lat2).abs() < 1e-9);
        }
    }
}
