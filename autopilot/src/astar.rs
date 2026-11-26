//! A* on the inflated occupancy grid, with line-of-sight smoothing.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::grid::{Cell, Grid};

#[derive(Debug, Clone, PartialEq)]
pub enum PlanError {
    StartNotFree,
    GoalNotFree,
    NoPath,
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanError::StartNotFree => write!(f, "start is not free"),
            PlanError::GoalNotFree => write!(f, "goal is not free"),
            PlanError::NoPath => write!(f, "no path"),
        }
    }
}

impl std::error::Error for PlanError {}

const SQRT2: f64 = std::f64::consts::SQRT_2;

/// Plan from one world point to another. The grid is expected to be already
/// inflated; this function only refuses occupied cells.
pub fn plan(
    grid: &Grid,
    start: (f64, f64),
    goal: (f64, f64),
) -> Result<Vec<(f64, f64)>, PlanError> {
    let (sx, sy) = grid.nearest_free(start, 5).ok_or(PlanError::StartNotFree)?;
    let (gx, gy) = grid.nearest_free(goal, 5).ok_or(PlanError::GoalNotFree)?;
    let start_idx = sy * grid.width + sx;
    let goal_idx = gy * grid.width + gx;

    let n = grid.width * grid.height;
    let mut g_score = vec![f64::INFINITY; n];
    let mut came = vec![usize::MAX; n];
    let mut closed = vec![false; n];
    g_score[start_idx] = 0.0;

    let mut open: BinaryHeap<(Reverse<u64>, usize)> = BinaryHeap::new();
    open.push((Reverse(0), start_idx));
    let mut expansions = 0usize;
    let cap = n.saturating_mul(4);

    while let Some((_, idx)) = open.pop() {
        if idx == goal_idx {
            break;
        }
        if closed[idx] {
            continue;
        }
        closed[idx] = true;
        expansions += 1;
        if expansions > cap {
            return Err(PlanError::NoPath);
        }
        let x = idx % grid.width;
        let y = idx / grid.width;
        for (dx, dy, cost) in neighbors() {
            let nx = x as i64 + dx;
            let ny = y as i64 + dy;
            if nx < 0 || ny < 0 || nx as usize >= grid.width || ny as usize >= grid.height {
                continue;
            }
            let (nx, ny) = (nx as usize, ny as usize);
            if grid.get(nx, ny) != Cell::Free {
                continue;
            }
            // No corner cutting: a diagonal move needs both orthogonal cells.
            if dx != 0
                && dy != 0
                && (grid.get((x as i64 + dx) as usize, y) != Cell::Free
                    || grid.get(x, (y as i64 + dy) as usize) != Cell::Free)
            {
                continue;
            }
            let ni = ny * grid.width + nx;
            let tentative = g_score[idx] + cost;
            if tentative + 1e-12 < g_score[ni] {
                g_score[ni] = tentative;
                came[ni] = idx;
                let f = tentative + heuristic(nx, ny, gx, gy);
                open.push((Reverse((f * 1000.0) as u64), ni));
            }
        }
    }

    if g_score[goal_idx].is_infinite() {
        return Err(PlanError::NoPath);
    }

    let mut cells = vec![goal_idx];
    let mut cur = goal_idx;
    while cur != start_idx {
        cur = came[cur];
        if cur == usize::MAX {
            return Err(PlanError::NoPath);
        }
        cells.push(cur);
    }
    cells.reverse();
    let points: Vec<(f64, f64)> = cells
        .into_iter()
        .map(|i| grid.cell_to_world(i % grid.width, i / grid.width))
        .collect();
    Ok(smooth(grid, &points))
}

fn neighbors() -> [(i64, i64, f64); 8] {
    [
        (1, 0, 1.0),
        (-1, 0, 1.0),
        (0, 1, 1.0),
        (0, -1, 1.0),
        (1, 1, SQRT2),
        (1, -1, SQRT2),
        (-1, 1, SQRT2),
        (-1, -1, SQRT2),
    ]
}

fn heuristic(x: usize, y: usize, gx: usize, gy: usize) -> f64 {
    let dx = (x as f64 - gx as f64).abs();
    let dy = (y as f64 - gy as f64).abs();
    let (lo, hi) = if dx < dy { (dx, dy) } else { (dy, dx) };
    hi + (SQRT2 - 1.0) * lo
}

/// Keep the farthest visible point, repeat. Turns the staircase into a path
/// the pursuit controller can actually follow.
pub fn smooth(grid: &Grid, points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut out = vec![points[0]];
    let mut anchor = 0usize;
    while anchor + 1 < points.len() {
        let mut best = anchor + 1;
        for probe in (anchor + 2)..points.len() {
            if line_of_sight(grid, points[anchor], points[probe]) {
                best = probe;
            } else {
                break;
            }
        }
        out.push(points[best]);
        anchor = best;
    }
    out
}

pub fn line_of_sight(grid: &Grid, a: (f64, f64), b: (f64, f64)) -> bool {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let dist = (dx * dx + dy * dy).sqrt();
    let steps = (dist / grid.resolution * 2.0).ceil().max(1.0) as usize;
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let p = (a.0 + dx * t, a.1 + dy * t);
        if !grid.is_free(p) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Grid {
        Grid::from_rows(
            &[
                ".........",
                ".........",
                ".........",
                "....#....",
                "....#....",
                "....#....",
                ".........",
                ".........",
            ],
            0.1,
            (0.0, 0.0),
        )
    }

    #[test]
    fn plans_around_a_wall() {
        let g = grid();
        let path = plan(&g, g.cell_to_world(1, 3), g.cell_to_world(7, 3)).unwrap();
        assert!(path.len() > 2);
        for p in &path {
            assert!(g.is_free(*p));
        }
        let last = *path.last().unwrap();
        assert!((last.0 - g.cell_to_world(7, 3).0).abs() < 0.2);
    }

    #[test]
    fn no_path_through_a_solid_wall() {
        let g = Grid::from_rows(
            &[
                "....#....",
                "....#....",
                "....#....",
                "....#....",
                "....#....",
            ],
            0.1,
            (0.0, 0.0),
        );
        assert_eq!(
            plan(&g, g.cell_to_world(1, 2), g.cell_to_world(7, 2)),
            Err(PlanError::NoPath)
        );
    }

    #[test]
    fn diagonal_cannot_squeeze_between_two_walls() {
        // (0,0) and (1,1) are free, (0,1) and (1,0) are walls. Without the
        // corner-cut guard the rover would slip diagonally between them.
        let g = Grid::from_rows(&["#.", ".#"], 0.1, (0.0, 0.0));
        assert_eq!(
            plan(&g, g.cell_to_world(0, 0), g.cell_to_world(1, 1)),
            Err(PlanError::NoPath)
        );
    }

    #[test]
    fn smoothing_removes_the_staircase() {
        let g = Grid::from_rows(
            &[
                ".........",
                ".........",
                ".........",
                ".........",
                ".........",
            ],
            0.1,
            (0.0, 0.0),
        );
        let raw = vec![(0.05, 0.05), (0.15, 0.15), (0.25, 0.25), (0.35, 0.35)];
        let out = smooth(&g, &raw);
        assert_eq!(out.len(), 2);
    }
}
