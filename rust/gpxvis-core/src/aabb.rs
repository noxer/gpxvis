//! Axis-aligned bounding box (port of `gpxutil::CAABB`).

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl Default for Aabb {
    fn default() -> Self {
        Self {
            min: [f64::INFINITY; 3],
            max: [f64::NEG_INFINITY; 3],
        }
    }
}

impl Aabb {
    pub fn is_valid(&self) -> bool {
        self.min[0] <= self.max[0]
    }

    pub fn add(&mut self, p: [f64; 3]) {
        for ((lo, hi), v) in self.min.iter_mut().zip(&mut self.max).zip(p) {
            *lo = lo.min(v);
            *hi = hi.max(v);
        }
    }

    pub fn merge(&mut self, other: &Aabb) {
        if other.is_valid() {
            self.add(other.min);
            self.add(other.max);
        }
    }

    pub fn center(&self) -> Option<[f64; 3]> {
        self.is_valid()
            .then(|| std::array::from_fn(|i| 0.5 * (self.min[i] + self.max[i])))
    }
}
