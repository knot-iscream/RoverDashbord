//! 2D occupancy grid carried over MQTT from the ROS bridge.
//!
//! Wire shape:
//! `{"resolution":0.1,"width":200,"height":200,"origin":[x,y],
//!   "rle":[[count,value], ...]}`
//! value: 0 unknown, 1 free, 2 occupied. Row-major, x fastest, origin is the
//! lower-left corner of cell (0,0).

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    Unknown = 0,
    Free = 1,
    Occupied = 2,
}

impl Cell {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Cell::Unknown),
            1 => Some(Cell::Free),
            2 => Some(Cell::Occupied),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    pub width: usize,
    pub height: usize,
    pub resolution: f64,
    pub origin: (f64, f64),
    pub cells: Vec<Cell>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GridError {
    Missing(&'static str),
    Bad(&'static str),
    TooLarge,
    RleMismatch { got: usize, want: usize },
}

impl std::fmt::Display for GridError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GridError::Missing(k) => write!(f, "missing {k}"),
            GridError::Bad(k) => write!(f, "bad {k}"),
            GridError::TooLarge => write!(f, "grid too large"),
            GridError::RleMismatch { got, want } => {
                write!(f, "rle covers {got} cells, want {want}")
            }
        }
    }
}

impl std::error::Error for GridError {}

impl Grid {
    pub fn decode(payload: &Value) -> Result<Self, GridError> {
        let resolution = payload
            .get("resolution")
            .and_then(|v| v.as_f64())
            .ok_or(GridError::Missing("resolution"))?;
        if !resolution.is_finite() || resolution <= 0.0 {
            return Err(GridError::Bad("resolution"));
        }
        let width = payload
            .get("width")
            .and_then(|v| v.as_u64())
            .ok_or(GridError::Missing("width"))? as usize;
        let height = payload
            .get("height")
            .and_then(|v| v.as_u64())
            .ok_or(GridError::Missing("height"))? as usize;
        let total = width.checked_mul(height).ok_or(GridError::TooLarge)?;
        if width == 0 || height == 0 || width > 4000 || height > 4000 || total > 4_000_000 {
            return Err(GridError::TooLarge);
        }
        let origin = payload
            .get("origin")
            .and_then(|v| v.as_array())
            .filter(|a| a.len() >= 2)
            .ok_or(GridError::Missing("origin"))?;
        let ox = origin[0].as_f64().ok_or(GridError::Bad("origin x"))?;
        let oy = origin[1].as_f64().ok_or(GridError::Bad("origin y"))?;
        if !ox.is_finite() || !oy.is_finite() {
            return Err(GridError::Bad("origin"));
        }
        let rle = payload
            .get("rle")
            .and_then(|v| v.as_array())
            .ok_or(GridError::Missing("rle"))?;

        let mut cells = Vec::with_capacity(total);
        for pair in rle {
            let Some(p) = pair.as_array().filter(|p| p.len() >= 2) else {
                return Err(GridError::Bad("rle pair"));
            };
            let count = p[0].as_u64().ok_or(GridError::Bad("rle count"))? as usize;
            let value = p[1].as_u64().ok_or(GridError::Bad("rle value"))? as u8;
            let cell = Cell::from_u8(value).ok_or(GridError::Bad("rle value"))?;
            if count == 0 {
                return Err(GridError::Bad("rle count"));
            }
            if cells.len() + count > total {
                return Err(GridError::RleMismatch {
                    got: cells.len() + count,
                    want: total,
                });
            }
            cells.extend(std::iter::repeat_n(cell, count));
        }
        if cells.len() != total {
            return Err(GridError::RleMismatch {
                got: cells.len(),
                want: total,
            });
        }
        Ok(Self {
            width,
            height,
            resolution,
            origin: (ox, oy),
            cells,
        })
    }

    /// Test helper: rows top-to-bottom, `.` free, `#` occupied, `?` unknown.
    pub fn from_rows(rows: &[&str], resolution: f64, origin: (f64, f64)) -> Self {
        let height = rows.len();
        let width = rows.first().map(|r| r.len()).unwrap_or(0);
        let mut cells = vec![Cell::Unknown; width * height];
        for (row_idx, row) in rows.iter().enumerate() {
            let y = height - 1 - row_idx;
            for (x, ch) in row.chars().enumerate() {
                if x >= width {
                    break;
                }
                cells[y * width + x] = match ch {
                    '#' => Cell::Occupied,
                    '.' => Cell::Free,
                    _ => Cell::Unknown,
                };
            }
        }
        Self {
            width,
            height,
            resolution,
            origin,
            cells,
        }
    }

    pub fn get(&self, x: usize, y: usize) -> Cell {
        if x >= self.width || y >= self.height {
            Cell::Unknown
        } else {
            self.cells[y * self.width + x]
        }
    }

    pub fn set(&mut self, x: usize, y: usize, cell: Cell) {
        if x < self.width && y < self.height {
            self.cells[y * self.width + x] = cell;
        }
    }

    pub fn world_to_cell(&self, p: (f64, f64)) -> Option<(usize, usize)> {
        let x = ((p.0 - self.origin.0) / self.resolution).floor();
        let y = ((p.1 - self.origin.1) / self.resolution).floor();
        if x < 0.0 || y < 0.0 {
            return None;
        }
        let (x, y) = (x as usize, y as usize);
        (x < self.width && y < self.height).then_some((x, y))
    }

    pub fn cell_to_world(&self, x: usize, y: usize) -> (f64, f64) {
        (
            self.origin.0 + (x as f64 + 0.5) * self.resolution,
            self.origin.1 + (y as f64 + 0.5) * self.resolution,
        )
    }

    pub fn is_free(&self, p: (f64, f64)) -> bool {
        self.world_to_cell(p)
            .is_some_and(|(x, y)| self.get(x, y) == Cell::Free)
    }

    /// Grow obstacles by `radius_m`, circularly. Conservative on purpose:
    /// the global path should stay clear of the local reflex's limits.
    pub fn inflate(&self, radius_m: f64) -> Self {
        let mut out = self.clone();
        let r_cells = (radius_m / self.resolution).ceil().max(0.0) as i64;
        if r_cells == 0 {
            return out;
        }
        let mut offsets = Vec::new();
        for dy in -r_cells..=r_cells {
            for dx in -r_cells..=r_cells {
                if dx * dx + dy * dy <= r_cells * r_cells {
                    offsets.push((dx, dy));
                }
            }
        }
        for y in 0..self.height {
            for x in 0..self.width {
                if self.get(x, y) != Cell::Occupied {
                    continue;
                }
                for (dx, dy) in &offsets {
                    let nx = x as i64 + dx;
                    let ny = y as i64 + dy;
                    if nx >= 0
                        && ny >= 0
                        && (nx as usize) < self.width
                        && (ny as usize) < self.height
                    {
                        out.set(nx as usize, ny as usize, Cell::Occupied);
                    }
                }
            }
        }
        out
    }

    /// Nearest free cell, searching outward by Chebyshev rings. Used when a
    /// start or goal lands inside inflation.
    pub fn nearest_free(&self, p: (f64, f64), max_cells: i64) -> Option<(usize, usize)> {
        let (cx, cy) = self.world_to_cell(p)?;
        if self.get(cx, cy) == Cell::Free {
            return Some((cx, cy));
        }
        for r in 1..=max_cells {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let nx = cx as i64 + dx;
                    let ny = cy as i64 + dy;
                    if nx < 0 || ny < 0 || nx as usize >= self.width || ny as usize >= self.height {
                        continue;
                    }
                    if self.get(nx as usize, ny as usize) == Cell::Free {
                        return Some((nx as usize, ny as usize));
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decodes_rle_and_validates_total() {
        let g = Grid::decode(&json!({
            "resolution": 0.5, "width": 2, "height": 2, "origin": [0.0, 0.0],
            "rle": [[2, 1], [2, 2]]
        }))
        .unwrap();
        assert_eq!(g.get(0, 0), Cell::Free);
        assert_eq!(g.get(1, 0), Cell::Free);
        assert_eq!(g.get(0, 1), Cell::Occupied);
        assert_eq!(g.get(1, 1), Cell::Occupied);
    }

    #[test]
    fn rejects_short_rle() {
        let e = Grid::decode(&json!({
            "resolution": 0.5, "width": 2, "height": 2, "origin": [0.0, 0.0],
            "rle": [[3, 1]]
        }))
        .unwrap_err();
        assert!(matches!(e, GridError::RleMismatch { .. }));
    }

    #[test]
    fn world_and_cell_round_trip() {
        let g = Grid::from_rows(&["...", "..."], 0.1, (-1.0, -1.0));
        let (x, y) = g.cell_to_world(2, 1);
        assert_eq!(g.world_to_cell((x, y)), Some((2, 1)));
    }

    #[test]
    fn inflation_grows_by_radius() {
        let g = Grid::from_rows(&[".....", "..#..", "....."], 0.1, (0.0, 0.0));
        let inflated = g.inflate(0.2);
        assert_eq!(inflated.get(2, 1), Cell::Occupied);
        assert_eq!(inflated.get(1, 1), Cell::Occupied);
        assert_eq!(inflated.get(0, 1), Cell::Occupied);
        assert_eq!(inflated.get(4, 1), Cell::Occupied);
        assert_eq!(
            inflated.get(0, 0),
            Cell::Free,
            "corner is outside the circle"
        );
    }

    #[test]
    fn nearest_free_finds_a_cell_inside_inflation() {
        let g = Grid::from_rows(&["###", "#.#", "###"], 0.1, (0.0, 0.0));
        let p = g.cell_to_world(1, 1);
        assert_eq!(g.nearest_free(p, 2), Some((1, 1)));
    }
}
