pub const CELL_PX: u32 = 8;
pub const TILE_W: u32 = 32;
pub const TILE_H: u32 = 8;
const TILE_CELLS: u32 = TILE_W / CELL_PX;

const FRINGE_WORTH_OWN_READS_PX: f32 = 0.3;

pub const SHOWN: u32 = 1;
pub const OWN_RED_BLUE: u32 = 2;
pub const NO_RED: u32 = 4;
pub const NO_BLUE: u32 = 8;
pub const RIGHT_EYE: u32 = 16;
pub const BLENDED: u32 = 32;
const OTHER_EYE_SHIFT: u32 = 5;
const CHANNEL_NEEDS: u32 = OWN_RED_BLUE | NO_RED | NO_BLUE;

const TILE_Y_SHIFT: u32 = 12;
const TILE_EYE_SHIFT: u32 = 24;

pub struct Grid<'a> {
    pub points: &'a [[f32; 2]],
    pub width: u32,
    pub height: u32,
    pub per_channel: bool,
    pub blended: bool,
}

pub struct Plan {
    pub cell_needs: Vec<[f32; 2]>,
    pub one_eye_tiles: Vec<u32>,
    pub both_eye_tiles: Vec<u32>,
}

#[derive(Clone, Copy)]
struct Cell {
    origin: [f32; 2],
    dx: [f32; 2],
    dy: [f32; 2],
    dxy: [f32; 2],
    valid: bool,
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn add(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

fn scale(a: [f32; 2], s: f32) -> [f32; 2] {
    [a[0] * s, a[1] * s]
}

fn length(a: [f32; 2]) -> f32 {
    (a[0] * a[0] + a[1] * a[1]).sqrt()
}

impl Grid<'_> {
    fn count(&self) -> usize {
        (self.width * self.height) as usize
    }

    fn cells(&self) -> (u32, u32) {
        (self.width - 1, self.height - 1)
    }

    fn corners<T>(&self, i: u32, j: u32, at: impl Fn(usize) -> T) -> [T; 4] {
        let point = |i: u32, j: u32| (j * self.width + i) as usize;
        [at(point(i, j)), at(point(i + 1, j)), at(point(i, j + 1)), at(point(i + 1, j + 1))]
    }

    fn cell(&self, eye: usize, channel: usize, i: u32, j: u32) -> Cell {
        let [a, b, d, e] = self.corners(i, j, |p| self.points[eye * self.count() * 3 + p * 3 + channel]);
        let inv = 1.0 / CELL_PX as f32;
        Cell {
            origin: a,
            dx: scale(sub(b, a), inv),
            dy: scale(sub(d, a), inv),
            dxy: scale(add(sub(sub(e, d), b), a), inv * inv),
            valid: a[0].min(b[0]).min(d[0].min(e[0])) >= 0.0,
        }
    }

    fn eye_needs(&self, eye: usize, i: u32, j: u32) -> u32 {
        let green = self.cell(eye, 1, i, j);
        let mut needs = if green.valid { SHOWN } else { 0 };
        if self.per_channel {
            let red = self.cell(eye, 0, i, j);
            let blue = self.cell(eye, 2, i, j);
            let limit = FRINGE_WORTH_OWN_READS_PX * length(green.dx);
            if fringe(&red, &green) > limit || fringe(&blue, &green) > limit {
                needs |= OWN_RED_BLUE;
                if !red.valid {
                    needs |= NO_RED;
                }
                if !blue.valid {
                    needs |= NO_BLUE;
                }
            }
        }
        needs
    }

    fn cell_needs(&self, i: u32, j: u32) -> u32 {
        let left = self.eye_needs(0, i, j);
        if !self.blended {
            return left;
        }
        let right = self.eye_needs(1, i, j);
        let weights = self.corners(i, j, |p| self.points[self.count() * 6 + p][0]);
        let (left_shown, right_shown) = (left & SHOWN != 0, right & SHOWN != 0);
        if !right_shown || (left_shown && weights.iter().all(|&w| w <= 0.0)) {
            left
        } else if !left_shown || weights.iter().all(|&w| w >= 1.0) {
            right | RIGHT_EYE
        } else {
            left | BLENDED | (right & CHANNEL_NEEDS) << OTHER_EYE_SHIFT
        }
    }
}

fn fringe(channel: &Cell, green: &Cell) -> f32 {
    let s = CELL_PX as f32;
    let o = sub(channel.origin, green.origin);
    let x = scale(sub(channel.dx, green.dx), s);
    let y = scale(sub(channel.dy, green.dy), s);
    let xy = scale(sub(channel.dxy, green.dxy), s * s);
    length(o).max(length(add(o, x))).max(length(add(o, y)).max(length(add(add(add(o, x), y), xy))))
}

pub fn plan(grid: &Grid, out_w: u32, out_h: u32) -> Plan {
    let (cells_x, cells_y) = grid.cells();
    let needs: Vec<u32> = (0..cells_y).flat_map(|j| (0..cells_x).map(move |i| (i, j))).map(|(i, j)| grid.cell_needs(i, j)).collect();
    let mut plan = Plan {
        cell_needs: needs.iter().map(|&n| [n as f32, 0.0]).collect(),
        one_eye_tiles: Vec::new(),
        both_eye_tiles: Vec::new(),
    };
    for ty in 0..out_h.div_ceil(TILE_H).min(cells_y) {
        for tx in 0..out_w.div_ceil(TILE_W) {
            let tile = (0..TILE_CELLS).map(|c| needs[(ty * cells_x + (tx * TILE_CELLS + c).min(cells_x - 1)) as usize]);
            let mut eyes = tile.clone().filter(|n| n & SHOWN != 0).map(|n| n & RIGHT_EYE != 0);
            let eye = eyes.next().unwrap_or(false);
            let place = tx | ty << TILE_Y_SHIFT;
            if tile.clone().any(|n| n & BLENDED != 0) || eyes.any(|e| e != eye) {
                plan.both_eye_tiles.push(place);
            } else {
                plan.one_eye_tiles.push(place | (eye as u32) << TILE_EYE_SHIFT);
            }
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: u32 = 9;
    const H: u32 = 3;
    const OUT: (u32, u32) = (64, 16);

    fn flat(eyes: usize) -> Vec<[f32; 2]> {
        let count = (W * H) as usize;
        let mut points = vec![[0.0; 2]; count * if eyes == 2 { 7 } else { 3 }];
        for eye in 0..eyes {
            for j in 0..H {
                for i in 0..W {
                    let at = eye * count * 3 + ((j * W + i) * 3) as usize;
                    points[at..at + 3].fill([0.1 + i as f32 * 0.01, 0.1 + j as f32 * 0.01]);
                }
            }
        }
        points
    }

    fn needs(plan: &Plan, i: u32, j: u32) -> u32 {
        plan.cell_needs[(j * (W - 1) + i) as usize][0] as u32
    }

    #[test]
    fn channels_that_line_up_share_a_read_and_a_fringe_gets_its_own() {
        let mut points = flat(1);
        let per_output_px = 0.01 / CELL_PX as f32;
        points[((W + 2) * 3) as usize][0] += per_output_px * 0.2;
        points[((W + 6) * 3 + 2) as usize][0] += per_output_px * 0.4;
        let grid = Grid { points: &points, width: W, height: H, per_channel: true, blended: false };
        let plan = plan(&grid, OUT.0, OUT.1);
        assert_eq!(needs(&plan, 1, 0), SHOWN);
        assert_eq!(needs(&plan, 5, 0), SHOWN | OWN_RED_BLUE);
        assert_eq!(needs(&plan, 6, 1), SHOWN | OWN_RED_BLUE);
        assert_eq!(needs(&plan, 3, 1), SHOWN);
        assert_eq!(plan.one_eye_tiles, [0, 1, 1 << TILE_Y_SHIFT, 1 | 1 << TILE_Y_SHIFT]);
        assert!(plan.both_eye_tiles.is_empty());
    }

    #[test]
    fn a_cell_past_the_lens_is_not_shown_and_a_lost_channel_reads_black() {
        let mut points = flat(1);
        points[1] = [-1.0, -1.0];
        points[(4 * 3) as usize] = [-1.0, -1.0];
        let grid = Grid { points: &points, width: W, height: H, per_channel: true, blended: false };
        let plan = plan(&grid, OUT.0, OUT.1);
        assert_eq!(needs(&plan, 0, 0) & SHOWN, 0);
        assert_eq!(needs(&plan, 3, 0), SHOWN | OWN_RED_BLUE | NO_RED);
        assert_eq!(needs(&plan, 1, 1), SHOWN);
    }

    #[test]
    fn each_cell_takes_the_eye_its_weights_ask_for_and_only_the_crossfade_reads_both() {
        let mut points = flat(2);
        let count = (W * H) as usize;
        for j in 0..H {
            for i in 0..W {
                let weight = ((i as f32 - 5.0) / 2.0).clamp(0.0, 1.0);
                points[count * 6 + (j * W + i) as usize] = [weight, 0.0];
            }
        }
        points[count * 3 + 1] = [-1.0, -1.0];
        let grid = Grid { points: &points, width: W, height: H, per_channel: false, blended: true };
        let plan = plan(&grid, OUT.0, OUT.1);
        assert_eq!(needs(&plan, 0, 1), SHOWN);
        assert_eq!(needs(&plan, 4, 1), SHOWN);
        assert_eq!(needs(&plan, 5, 1), SHOWN | BLENDED);
        assert_eq!(needs(&plan, 6, 1), SHOWN | BLENDED);
        assert_eq!(needs(&plan, 7, 1), SHOWN | RIGHT_EYE);
        assert_eq!(needs(&plan, 0, 0), SHOWN);
        assert_eq!(plan.one_eye_tiles, [0, 1 << TILE_Y_SHIFT]);
        assert_eq!(plan.both_eye_tiles, [1, 1 | 1 << TILE_Y_SHIFT]);

        for p in &mut points[count * 6..] {
            p[0] = 1.0;
        }
        let grid = Grid { points: &points, width: W, height: H, per_channel: false, blended: true };
        let plan = super::plan(&grid, OUT.0, OUT.1);
        assert_eq!(needs(&plan, 0, 0), SHOWN, "the right eye can't show this cell, so the left one does");
        assert_eq!(plan.one_eye_tiles, [1 | 1 << TILE_EYE_SHIFT, 1 << TILE_Y_SHIFT | 1 << TILE_EYE_SHIFT, 1 | 1 << TILE_Y_SHIFT | 1 << TILE_EYE_SHIFT]);
        assert_eq!(plan.both_eye_tiles, [0], "a tile whose cells take different eyes reads both");
    }
}
