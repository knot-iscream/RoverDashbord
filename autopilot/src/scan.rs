//! `rover/lidar/scan` parsing and angular binning.
//!
//! Wire shape (from `scan_to_mqtt.py`):
//! `{"ok":1,"model":24,"count":N,"points":[[angle_deg, range_mm, quality], ...]}`

use serde_json::Value;

use crate::geo::normalize_deg;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScanPoint {
    pub angle_deg: f64,
    pub range_m: f64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScanFrame {
    pub points: Vec<ScanPoint>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScanError {
    MissingPoints,
    BadPoint(usize),
    Empty,
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScanError::MissingPoints => write!(f, "payload has no points array"),
            ScanError::BadPoint(i) => write!(f, "point {i} is not [angle, mm, quality]"),
            ScanError::Empty => write!(f, "scan has no usable points"),
        }
    }
}

impl std::error::Error for ScanError {}

/// Parse one full turn. Bad points are skipped rather than failing the frame:
/// an RPLIDAR turn always contains a few junk returns.
pub fn parse(payload: &Value) -> Result<ScanFrame, ScanError> {
    let rows = payload
        .get("points")
        .and_then(|v| v.as_array())
        .ok_or(ScanError::MissingPoints)?;
    let mut points = Vec::with_capacity(rows.len());
    for (i, row) in rows.iter().enumerate() {
        let Some(a) = row.as_array() else {
            continue;
        };
        let angle = a.first().and_then(|v| v.as_f64());
        let mm = a.get(1).and_then(|v| v.as_f64());
        let quality = a.get(2).and_then(|v| v.as_i64()).unwrap_or(1);
        let (Some(angle), Some(mm)) = (angle, mm) else {
            if i == 0 {
                return Err(ScanError::BadPoint(i));
            }
            continue;
        };
        if quality <= 0 {
            continue;
        }
        let range_m = mm / 1000.0;
        if !(0.15..=12.0).contains(&range_m) {
            continue;
        }
        points.push(ScanPoint {
            angle_deg: normalize_deg(angle),
            range_m,
        });
    }
    if points.is_empty() {
        return Err(ScanError::Empty);
    }
    Ok(ScanFrame { points })
}

/// Fixed angular bins, each holding the nearest return in that sector.
#[derive(Debug, Clone, PartialEq)]
pub struct Bins {
    pub bin_deg: f64,
    pub ranges: Vec<Option<f64>>,
}

impl Bins {
    /// `offset_deg` converts the lidar frame to the rover frame.
    pub fn from_frame(frame: &ScanFrame, bin_deg: f64, offset_deg: f64) -> Self {
        let n = (360.0 / bin_deg).round().max(1.0) as usize;
        let mut ranges = vec![None; n];
        for p in &frame.points {
            let a = normalize_deg(p.angle_deg + offset_deg);
            let idx = (((a + 180.0) / bin_deg).floor() as i64).rem_euclid(n as i64) as usize;
            ranges[idx] = Some(ranges[idx].map_or(p.range_m, |r: f64| r.min(p.range_m)));
        }
        Self { bin_deg, ranges }
    }

    pub fn from_ranges(ranges: Vec<Option<f64>>, bin_deg: f64) -> Self {
        Self { bin_deg, ranges }
    }

    pub fn index_of(&self, angle_deg: f64) -> usize {
        let n = self.ranges.len();
        (((normalize_deg(angle_deg) + 180.0) / self.bin_deg).floor() as i64).rem_euclid(n as i64)
            as usize
    }

    /// Nearest return in the bin containing `angle_deg`.
    pub fn range_at(&self, angle_deg: f64) -> Option<f64> {
        self.ranges[self.index_of(angle_deg)]
    }

    /// Nearest return in an arc. `None` when the arc has no returns at all.
    pub fn min_in_arc(&self, center_deg: f64, half_width_deg: f64) -> Option<f64> {
        let n = self.ranges.len() as i64;
        let half = (half_width_deg / self.bin_deg).ceil() as i64;
        let c = self.index_of(center_deg) as i64;
        let mut best: Option<f64> = None;
        for k in -half..=half {
            let idx = (c + k).rem_euclid(n) as usize;
            if let Some(r) = self.ranges[idx] {
                best = Some(best.map_or(r, |b: f64| b.min(r)));
            }
        }
        best
    }

    /// An empty bin counts as open space: no return usually means clear air.
    pub fn is_open(&self, angle_deg: f64, threshold_m: f64) -> bool {
        self.range_at(angle_deg).is_none_or(|r| r > threshold_m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_metres_from_millimetres_and_drops_junk() {
        let frame = parse(&json!({
            "points": [[0.0, 1500.0, 15], [90.0, 90.0, 15], [180.0, 0.0, 0], [-90.0, 2000.0, 15]]
        }))
        .unwrap();
        assert_eq!(frame.points.len(), 2);
        assert!((frame.points[0].range_m - 1.5).abs() < 1e-9);
        assert!((frame.points[1].angle_deg + 90.0).abs() < 1e-9);
    }

    #[test]
    fn missing_points_is_an_error() {
        assert_eq!(parse(&json!({"ok": 1})), Err(ScanError::MissingPoints));
    }

    #[test]
    fn bins_keep_the_nearest_return() {
        let frame = ScanFrame {
            points: vec![
                ScanPoint {
                    angle_deg: 0.5,
                    range_m: 2.0,
                },
                ScanPoint {
                    angle_deg: 1.0,
                    range_m: 1.0,
                },
                ScanPoint {
                    angle_deg: 0.0,
                    range_m: 3.0,
                },
            ],
        };
        let bins = Bins::from_frame(&frame, 2.0, 0.0);
        assert_eq!(bins.range_at(0.5), Some(1.0));
        assert_eq!(bins.ranges.len(), 180);
    }

    #[test]
    fn yaw_offset_moves_the_return() {
        let frame = ScanFrame {
            points: vec![ScanPoint {
                angle_deg: 0.0,
                range_m: 2.0,
            }],
        };
        let bins = Bins::from_frame(&frame, 2.0, 90.0);
        assert_eq!(bins.range_at(90.0), Some(2.0));
        assert_eq!(bins.range_at(0.0), None);
    }

    #[test]
    fn arc_minimum_wraps_around_the_seam() {
        let helper = Bins::from_ranges(vec![None; 180], 2.0);
        let idx = helper.index_of(-179.0);
        let mut ranges = vec![None; 180];
        ranges[idx] = Some(1.2);
        let bins = Bins::from_ranges(ranges, 2.0);
        let m = bins.min_in_arc(179.0, 4.0).unwrap();
        assert!((m - 1.2).abs() < 1e-9);
    }
}
