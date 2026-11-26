//! Small geometry helpers shared by the planner modules.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub x: f64,
    pub y: f64,
    /// Degrees, 0 = +x, positive counter-clockwise (ROS convention).
    pub yaw_deg: f64,
}

impl Pose {
    pub fn new(x: f64, y: f64, yaw_deg: f64) -> Self {
        Self { x, y, yaw_deg }
    }
}

/// Wrap to [-180, 180).
pub fn normalize_deg(mut d: f64) -> f64 {
    while d >= 180.0 {
        d -= 360.0;
    }
    while d < -180.0 {
        d += 360.0;
    }
    d
}

/// Signed shortest difference `a - b` in degrees.
pub fn angle_diff(a: f64, b: f64) -> f64 {
    normalize_deg(a - b)
}

pub fn distance(a: (f64, f64), b: (f64, f64)) -> f64 {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    (dx * dx + dy * dy).sqrt()
}

/// Bearing from `from` to `to` in degrees, 0 = +x.
pub fn bearing_to(from: (f64, f64), to: (f64, f64)) -> f64 {
    (to.1 - from.1).atan2(to.0 - from.0).to_degrees()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_angles() {
        assert_eq!(normalize_deg(190.0), -170.0);
        assert_eq!(normalize_deg(-190.0), 170.0);
        assert_eq!(normalize_deg(180.0), -180.0);
    }

    #[test]
    fn shortest_difference_crosses_the_seam() {
        assert!((angle_diff(170.0, -170.0) - (-20.0)).abs() < 1e-9);
        assert!((angle_diff(-170.0, 170.0) - 20.0).abs() < 1e-9);
    }

    #[test]
    fn bearing_and_distance() {
        assert!((bearing_to((0.0, 0.0), (1.0, 0.0)) - 0.0).abs() < 1e-9);
        assert!((bearing_to((0.0, 0.0), (0.0, 1.0)) - 90.0).abs() < 1e-9);
        assert!((distance((0.0, 0.0), (3.0, 4.0)) - 5.0).abs() < 1e-9);
    }
}
