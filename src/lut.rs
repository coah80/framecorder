//! The lookup grid the shader uses to find, for each output pixel, where on
//! the panel to read from. Built once at startup, so the per-frame GPU work is
//! just a few texture reads per pixel.

use anyhow::{bail, Result};

use framecorder::openvr::OpenVr;

/// Output pixels between grid points. The mapping is smooth, 8 is plenty.
const STEP: u32 = 8;
/// Samples per side when reading SteamVR's distortion function.
const DISTORTION_GRID: usize = 97;
const INVALID: [f32; 2] = [-1.0, -1.0];
/// Color fringes narrower than this (in output pixels) are left alone.
const MIN_FRINGE_PX: f64 = 0.35;
/// Supersample once an output pixel covers more than this many panel pixels.
const SUPERSAMPLE_ABOVE: f64 = 1.5;
/// Panel pixels across one eye.
const EYE_PX: f64 = 2160.0;
/// The Frame's compositor scans each eye out turned 180° in its own half,
/// while SteamVR's distortion data describes the panels upright. Checked on
/// a headset: straight ahead lands where the in-place rotation puts it (not
/// where a whole-display rotation would), and recordings came out upside
/// down without this.
const PANELS_ROTATED: bool = true;

pub struct Lut {
    pub blended: bool,
    pub width: u32,
    pub height: u32,
    pub step: f32,
    /// Whether the red and blue channels have their own positions (they do
    /// when the compositor corrects chromatic aberration).
    pub per_channel: bool,
    /// Whether the output shrinks the panel enough that single taps would
    /// alias, so the shader should average a few.
    pub supersample: bool,
    /// Grid positions are in SteamVR's panel space; the scanout has each
    /// eye's half turned 180° from that. See `PANELS_ROTATED`.
    pub rotated_eyes: bool,
    /// Scanout coordinates, r/g/b per point. Blended views append the right
    /// eye grid, then one vec2 per point with its right-eye weight in x.
    pub points: Vec<[f32; 2]>,
}

impl Lut {
    fn new(out_w: u32, out_h: u32) -> Self {
        let width = out_w.div_ceil(STEP) + 1;
        let height = out_h.div_ceil(STEP) + 1;
        Self {
            blended: false,
            width,
            height,
            step: STEP as f32,
            per_channel: false,
            supersample: false,
            rotated_eyes: PANELS_ROTATED,
            points: vec![INVALID; (width * height * 3) as usize],
        }
    }

    fn grid(&self) -> impl Iterator<Item = (usize, f64, f64)> + '_ {
        (0..self.height).flat_map(move |j| {
            (0..self.width).map(move |i| (((j * self.width + i) * 3) as usize, (i * STEP) as f64, (j * STEP) as f64))
        })
    }
}

/// The whole panel, both eyes, lens distortion and all, fitted into the
/// output with black bars.
pub fn raw(out_w: u32, out_h: u32, src_w: u32, src_h: u32) -> Lut {
    let mut lut = Lut::new(out_w, out_h);
    let scale = (out_w as f64 / src_w as f64).min(out_h as f64 / src_h as f64);
    lut.supersample = 1.0 / scale > SUPERSAMPLE_ABOVE;
    let (fit_w, fit_h) = (src_w as f64 * scale, src_h as f64 * scale);
    let (ox, oy) = ((out_w as f64 - fit_w) / 2.0, (out_h as f64 - fit_h) / 2.0);

    let cells: Vec<_> = lut.grid().collect();
    for (idx, x, y) in cells {
        let u = (x - ox) / fit_w;
        let v = (y - oy) / fit_h;
        // Let the grid run a bit past the picture so the edges interpolate
        // cleanly, anything further out is a black bar.
        let margin = STEP as f64 / fit_w.min(fit_h);
        let p = if (-margin..=1.0 + margin).contains(&u) && (-margin..=1.0 + margin).contains(&v) {
            [u.clamp(0.0, 1.0) as f32, v.clamp(0.0, 1.0) as f32]
        } else {
            INVALID
        };
        lut.points[idx..idx + 3].fill(p);
    }
    lut
}

/// Output edge points checked per side when looking for the widest clean view.
const EDGE_SAMPLES: usize = 64;
/// Pulls the automatic view in a little from the very edge of what's drawn,
/// where the lens is at its blurriest.
const AUTO_FOV_MARGIN: f64 = 0.97;
/// Resolution of the hidden area mask, per side.
const MASK_SIZE: usize = 512;
/// How far off the middle of the view (see `EyeModel::middle`) the automatic
/// view may center itself. Zero: a few extra degrees aren't worth the
/// middle of the video not being where you looked.
const MAX_CENTER_SHIFT_DEG: f64 = 0.0;
/// Candidate centers per direction on each side of the middle.
const CENTER_STEPS: i32 = 5;

/// Everything we know about how one eye maps to the panel.
struct EyeModel {
    eye: usize,
    l: f64,
    r: f64,
    t: f64,
    b: f64,
    roll: f64,
    /// Level view direction -> the compositor's canted eye (see `cant`).
    cant: [[f64; 3]; 3],
    /// Panel -> render target, sampled on a grid, per channel.
    forward: Vec<[[f64; 2]; 3]>,
    /// Render target spots the compositor never draws.
    hidden: Vec<bool>,
    /// Render target spots that land somewhere on the panel.
    covered: Vec<bool>,
}

impl EyeModel {
    fn new(vr: &OpenVr, eye: usize) -> Result<Self> {
        let proj = vr.projection(eye);
        let roll = vr.eye_roll(eye);
        let cant = cant(eye);
        log::info!(
            "eye {eye} projection: {proj:?}, render target {:?}, roll {:.2}°",
            vr.render_target_size(),
            roll.to_degrees()
        );
        if proj.left >= 0.0 || proj.right <= 0.0 || proj.top >= 0.0 || proj.bottom <= 0.0 {
            bail!("SteamVR reported a weird projection: {proj:?}");
        }

        let n = DISTORTION_GRID;
        let mut forward = vec![[[f64::NAN; 2]; 3]; n * n];
        for j in 0..n {
            for i in 0..n {
                let (u, v) = (i as f32 / (n - 1) as f32, j as f32 / (n - 1) as f32);
                if let Some(d) = vr.distortion(eye, u, v) {
                    forward[j * n + i] = d.map(|c| [c[0] as f64, c[1] as f64]);
                }
            }
        }

        let triangles = vr.hidden_area(eye);
        let hidden = rasterize(&triangles);
        if log::log_enabled!(log::Level::Debug) {
            let rows: Vec<String> = (0..32)
                .map(|y| (0..64).map(|x| if hidden[(y * 16) * MASK_SIZE + x * 8] { '#' } else { '.' }).collect())
                .collect();
            log::debug!("hidden area of eye {eye}:\n{}", rows.join("\n"));
        }
        log::info!(
            "hidden area: {} triangles, {:.1}% of the render target",
            triangles.len(),
            hidden.iter().filter(|h| **h).count() as f64 * 100.0 / hidden.len() as f64
        );

        let covered = rasterize(&panel_coverage(&forward, n));

        Ok(Self {
            eye,
            l: proj.left as f64,
            r: proj.right as f64,
            t: proj.top as f64,
            b: proj.bottom as f64,
            roll,
            cant,
            forward,
            hidden,
            covered,
        })
    }

    /// Render target UV for a view direction given as tangents, with the
    /// eye's roll undone so the video stays level with the head.
    fn target(&self, tx: f64, ty: f64) -> [f64; 2] {
        let (s, c) = (-self.roll).sin_cos();
        let (rx, ry) = (tx * c - ty * s, tx * s + ty * c);
        let m = &self.cant;
        let d = [0, 1, 2].map(|i| m[i][0] * rx + m[i][1] * ry + m[i][2]);
        if d[2] <= 0.0 {
            return [f64::NAN; 2];
        }
        let (rx, ry) = (d[0] / d[2], d[1] / d[2]);
        [(rx - self.l) / (self.r - self.l), (ry - self.t) / (self.b - self.t)]
    }

    /// Whether a render target spot is on the panel and actually drawn.
    /// Whether a view direction (tangents, level) ends up in the video: the
    /// game drew it (its render target and hidden area are level, they're
    /// the game's) and it lands on the panel (the distortion is the
    /// compositor's, so canted).
    fn shown(&self, tx: f64, ty: f64) -> bool {
        let mask = |uv: [f64; 2], m: &[bool]| {
            if !(0.0..1.0).contains(&uv[0]) || !(0.0..1.0).contains(&uv[1]) {
                return false;
            }
            m[(uv[1] * MASK_SIZE as f64) as usize * MASK_SIZE + (uv[0] * MASK_SIZE as f64) as usize]
        };
        let level = [(tx - self.l) / (self.r - self.l), (ty - self.t) / (self.b - self.t)];
        let drawn = (0.0..1.0).contains(&level[0]) && (0.0..1.0).contains(&level[1]) && !mask(level, &self.hidden);
        drawn && mask(self.target(tx, ty), &self.covered)
    }

    /// Panel position (eye local) that shows this render target spot in channel `c`.
    fn solve(&self, c: usize, target: [f64; 2], guess: [f64; 2]) -> Option<[f64; 2]> {
        let n = DISTORTION_GRID;
        invert(&self.forward, n, c, target, guess).or_else(|| {
            let start = nearest(&self.forward, n, c, target)?;
            invert(&self.forward, n, c, target, start)
        })
    }

    /// Widest view at this aspect ratio that's entirely drawn, as the
    /// horizontal half-angle tangent plus a center offset (also tangents).
    /// The center may drift a few degrees off straight ahead when that buys
    /// a noticeably wider view, since the lens isn't symmetric.
    /// Where the middle of the video goes: the middle of what the eye sees
    /// vertically, which on the Frame is about 16° above its straight ahead
    /// (the lens shows ~60° up, ~49° down). It's also where SteamVR's own
    /// headset view is centered. Straight ahead sideways: the eyes' views
    /// are lopsided sideways too, but toward the nose and out, which is
    /// fine.
    fn middle(&self) -> (f64, f64) {
        (0.0, (self.t + self.b) / 2.0)
    }

    fn widest(&self, aspect: f64) -> (f64, f64, f64) {
        let fits = |tan_h: f64, cx: f64, cy: f64| {
            let tan_v = tan_h * aspect;
            (0..=EDGE_SAMPLES).all(|k| {
                let f = k as f64 / EDGE_SAMPLES as f64 * 2.0 - 1.0;
                [
                    (cx + f * tan_h, cy - tan_v),
                    (cx + f * tan_h, cy + tan_v),
                    (cx - tan_h, cy + f * tan_v),
                    (cx + tan_h, cy + f * tan_v),
                ]
                .iter()
                .all(|&(x, y)| self.shown(x, y))
            })
        };
        let limit = (-self.l).max(self.r).max((-self.t).max(self.b) / aspect);
        let search = |cx: f64, cy: f64| {
            let (mut lo, mut hi) = (0.0, limit);
            if !fits(1e-3, cx, cy) {
                return 0.0;
            }
            for _ in 0..30 {
                let mid = (lo + hi) / 2.0;
                if fits(mid, cx, cy) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            lo
        };

        let (bx, by) = self.middle();
        let centered = search(bx, by);
        let max_shift = MAX_CENTER_SHIFT_DEG.to_radians().tan();
        let mut best = (centered, bx, by);
        for i in -CENTER_STEPS..=CENTER_STEPS {
            for j in -CENTER_STEPS..=CENTER_STEPS {
                let cx = bx + max_shift * i as f64 / CENTER_STEPS as f64;
                let cy = by + max_shift * j as f64 / CENTER_STEPS as f64;
                let w = search(cx, cy);
                if w > best.0 {
                    best = (w, cx, cy);
                }
            }
        }
        // Only move off center when it's worth it.
        if best.0 < centered * 1.02 {
            best = (centered, bx, by);
        }
        best
    }
}

/// The panel's outline in render target space, as triangles.
fn panel_coverage(forward: &[[[f64; 2]; 3]], n: usize) -> Vec<[[f32; 2]; 3]> {
    let mut triangles = Vec::new();
    let at = |i: usize, j: usize| forward[j * n + i][1];
    for j in 0..n - 1 {
        for i in 0..n - 1 {
            let q = [at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)];
            if q.iter().any(|p| !p[0].is_finite() || !p[1].is_finite()) {
                continue;
            }
            let q = q.map(|p| [p[0] as f32, p[1] as f32]);
            triangles.push([q[0], q[1], q[2]]);
            triangles.push([q[0], q[2], q[3]]);
        }
    }
    triangles
}

/// Turns the hidden area triangles into a lookup mask.
fn rasterize(triangles: &[[[f32; 2]; 3]]) -> Vec<bool> {
    let mut mask = vec![false; MASK_SIZE * MASK_SIZE];
    let size = MASK_SIZE as f64;
    for tri in triangles {
        let p = tri.map(|v| [v[0] as f64 * size, v[1] as f64 * size]);
        let min_x = p.iter().map(|v| v[0]).fold(f64::INFINITY, f64::min).floor().max(0.0) as usize;
        let max_x = p.iter().map(|v| v[0]).fold(f64::NEG_INFINITY, f64::max).ceil().min(size) as usize;
        let min_y = p.iter().map(|v| v[1]).fold(f64::INFINITY, f64::min).floor().max(0.0) as usize;
        let max_y = p.iter().map(|v| v[1]).fold(f64::NEG_INFINITY, f64::max).ceil().min(size) as usize;
        let edge = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        let area = edge(p[0], p[1], p[2]);
        if area.abs() < 1e-12 {
            continue;
        }
        for y in min_y..max_y {
            for x in min_x..max_x {
                let c = [x as f64 + 0.5, y as f64 + 0.5];
                let w = [edge(p[1], p[2], c), edge(p[2], p[0], c), edge(p[0], p[1], c)];
                if w.iter().all(|w| w * area.signum() >= 0.0) {
                    mask[y * MASK_SIZE + x] = true;
                }
            }
        }
    }
    mask
}

/// Fits an upright flat source (SteamVR's headset view) into the output,
/// letterboxed if the shapes differ.
pub fn flat(out_w: u32, out_h: u32, src_w: u32, src_h: u32) -> Lut {
    let mut lut = raw(out_w, out_h, src_w, src_h);
    lut.rotated_eyes = false;
    lut
}

/// One eye as a flat 2D picture: the lens distortion SteamVR applied gets
/// undone, cropped to the output's aspect ratio around straight ahead. With
/// no field of view given, it picks the widest one that has no black edges.
/// How the compositor's eye is turned from the one apps render, which is
/// level: the Frame's displays are canted, about 10° of roll each way plus a
/// few degrees of yaw and pitch. SteamVR doesn't tell apps (it's not in the
/// eye to head transform), it's in the headset's factory calibration, which
/// is also what `vrcmd --info` shows as the "compositor residual". Without
/// it the video comes out tilted. Identity when there's no calibration.
fn cant(eye: usize) -> [[f64; 3]; 3] {
    const LEVEL: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let Some(m) = calibrated_eye_to_head(eye) else {
        log::info!("eye {eye}: no headset calibration found, assuming the display isn't canted");
        return LEVEL;
    };
    // The calibration's x and y point the other way from ours (it's a 180°
    // turn about the view axis, the head's plus_x and plus_z are -1 in it).
    // Checked against SteamVR's own level headset view and between the eyes:
    // level within about a degree, and the eyes line up but for their
    // spacing.
    let q: [[f64; 3]; 3] = std::array::from_fn(|i| {
        let sign = if i < 2 { -1.0 } else { 1.0 };
        std::array::from_fn(|j| sign * m[i][j])
    });
    log::info!(
        "eye {eye}: display canted {:.1}° roll, {:.1}° yaw, {:.1}° pitch (undoing it)",
        q[1][0].atan2(q[0][0]).to_degrees(),
        q[0][2].atan2(q[2][2]).to_degrees(),
        q[1][2].atan2(q[2][2]).to_degrees()
    );
    q
}

/// The headset's factory calibration of where each eye looks, from the
/// tracking config SteamVR keeps per headset.
fn calibrated_eye_to_head(eye: usize) -> Option<[[f64; 3]; 3]> {
    let home = std::env::var_os("HOME")?;
    let dir = std::path::Path::new(&home).join(".config/openvr/config/cv");
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let Ok(text) = std::fs::read_to_string(entry.path().join("config.json")) else { continue };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        let rows = &json["tracking_to_eye_transform"][eye]["eye_to_head"];
        let m: Option<Vec<Vec<f64>>> = rows.as_array().and_then(|r| r.iter().map(|row| row.as_array()?.iter().map(|v| v.as_f64()).collect()).collect());
        if let Some(m) = m.filter(|m| m.len() == 3 && m.iter().all(|r| r.len() == 3)) {
            return Some(std::array::from_fn(|i| std::array::from_fn(|j| m[i][j])));
        }
    }
    None
}

pub fn undistorted(vr: &OpenVr, eye: usize, fov_deg: Option<f64>, out_w: u32, out_h: u32) -> Result<Lut> {
    let model = EyeModel::new(vr, eye)?;
    let aspect = out_h as f64 / out_w as f64;

    let (widest, wide_cx, wide_cy) = model.widest(aspect);
    if widest <= 0.0 {
        bail!("couldn't find any clean part of the view, SteamVR's lens data looks off");
    }
    log::info!(
        "widest clean view at this aspect ratio: {:.1}° x {:.1}°, centered {:.1}° right and {:.1}° down from straight ahead",
        2.0 * widest.atan().to_degrees(),
        2.0 * (widest * aspect).atan().to_degrees(),
        wide_cx.atan().to_degrees(),
        wide_cy.atan().to_degrees()
    );
    let (tan_h, cx, cy) = match fov_deg {
        Some(fov) => {
            let wanted = (fov.to_radians() / 2.0).tan();
            if wanted > widest {
                log::warn!("a {fov}° view reaches past what the lens shows, the edges will be black");
            }
            let (mx, my) = model.middle();
            (wanted, mx, my)
        }
        None => (widest * AUTO_FOV_MARGIN, wide_cx, wide_cy),
    };
    let tan_v = tan_h * aspect;
    log::info!(
        "recording {:.1}° x {:.1}° of the eye's view",
        2.0 * tan_h.atan().to_degrees(),
        2.0 * tan_v.atan().to_degrees()
    );

    let mut lut = Lut::new(out_w, out_h);
    let eye_offset = model.eye as f64 * 0.5;
    let mut guess = [[0.5f64, 0.5]; 3];
    // Widest color fringe, measured in output pixels.
    let mut max_split = 0.0f64;
    let mut max_minify = 0.0f64;
    let mut prev_green: Option<[f64; 2]> = None;

    let cells: Vec<_> = lut.grid().collect();
    for (idx, x, y) in cells {
        if x == 0.0 {
            prev_green = None;
        }
        let tx = cx + (x / out_w as f64 * 2.0 - 1.0) * tan_h;
        let ty = cy + (y / out_h as f64 * 2.0 - 1.0) * tan_v;
        let target = model.target(tx, ty);

        let mut solved = [None; 3];
        for c in 0..3 {
            if let Some(d) = model.solve(c, target, guess[c]) {
                guess[c] = d;
                solved[c] = Some(d);
                lut.points[idx + c] = [(eye_offset + d[0] * 0.5) as f32, d[1] as f32];
            }
        }
        if let (Some(rd), Some(gd), Some(bd), Some(prev)) = (solved[0], solved[1], solved[2], prev_green) {
            // How far the panel moves per output pixel, to express the split in output pixels.
            let panel_per_out = dist(gd, prev) / STEP as f64;
            if panel_per_out > 0.0 {
                max_split = max_split.max(dist(rd, gd).max(dist(bd, gd)) / panel_per_out);
                max_minify = max_minify.max(panel_per_out * EYE_PX);
            }
        }
        prev_green = solved[1];
    }

    // A fringe well under a pixel in the video isn't worth tripling the
    // texture reads for.
    lut.per_channel = max_split > MIN_FRINGE_PX;
    lut.supersample = max_minify > SUPERSAMPLE_ABOVE;
    log::info!(
        "distortion grid ready: color fringe up to {:.2} px (correcting: {}), up to {:.2} panel px per output px (supersampling: {})",
        max_split,
        lut.per_channel,
        max_minify,
        lut.supersample
    );
    Ok(lut)
}

/// Where in the overlap the left eye fades into the right, as SteamVR's own
/// headset view does in its default "both eyes, right dominant" mode
/// (`SetHeadsetViewBlendRange(0.0, 0.1)` from vrmonitor): the right eye
/// wherever it sees, and the left eye only past the right eye's inner edge,
/// fading out linearly over the tenth of the overlap next to that edge. The
/// middle of the picture comes from one eye, so nothing ghosts there.
const BLEND_RANGE: [f64; 2] = [0.0, 0.1];

fn blend_weight(tx: f64, overlap: [f64; 2]) -> f64 {
    let start = overlap[0] + (overlap[1] - overlap[0]) * BLEND_RANGE[0];
    let end = overlap[0] + (overlap[1] - overlap[0]) * BLEND_RANGE[1];
    ((tx - start) / (end - start)).clamp(0.0, 1.0)
}

/// How far either eye's view reaches sideways.
fn stereo_limit(models: &[EyeModel; 2]) -> f64 {
    models.iter().map(|m| (-m.l).max(m.r)).fold(0.0, f64::max)
}

/// What both eyes see across every one of `rows`. SteamVR measures it at the
/// top and bottom of its picture, where the round lens edge pulls it in most,
/// so the fade never lands where the right eye has already run out.
fn stereo_overlap(models: &[EyeModel; 2], rows: &[f64], limit: f64) -> Result<[f64; 2]> {
    let mut overlap = [f64::NEG_INFINITY, f64::INFINITY];
    for &y in rows {
        let mut row = [f64::INFINITY, f64::NEG_INFINITY];
        for k in 0..=MASK_SIZE {
            let x = (k as f64 / MASK_SIZE as f64 * 2.0 - 1.0) * limit;
            if models.iter().all(|m| m.shown(x, y)) {
                row[0] = row[0].min(x);
                row[1] = row[1].max(x);
            }
        }
        overlap = [overlap[0].max(row[0]), overlap[1].min(row[1])];
    }
    if overlap[0] >= overlap[1] {
        bail!("the eyes have no stereo overlap at the center of the view");
    }
    Ok(overlap)
}

fn stereo_fit(models: &[EyeModel; 2], aspect: f64, cy: f64, limit: f64) -> f64 {
    let fits = |h: f64| {
        (0..=EDGE_SAMPLES).all(|j| (0..=EDGE_SAMPLES).all(|i| {
            let x = (i as f64 / EDGE_SAMPLES as f64 * 2.0 - 1.0) * h;
            let y = cy + (j as f64 / EDGE_SAMPLES as f64 * 2.0 - 1.0) * h * aspect;
            models.iter().any(|m| m.shown(x, y))
        }))
    };
    let (mut lo, mut hi) = (0.0, limit);
    for _ in 0..30 {
        let mid = (lo + hi) / 2.0;
        if fits(mid) { lo = mid; } else { hi = mid; }
    }
    lo
}

pub fn blended(vr: &OpenVr, fov_deg: Option<f64>, out_w: u32, out_h: u32) -> Result<Lut> {
    let models = [EyeModel::new(vr, 0)?, EyeModel::new(vr, 1)?];
    stereo_lut(&models, fov_deg, out_w, out_h)
}

fn stereo_lut(models: &[EyeModel; 2], fov_deg: Option<f64>, out_w: u32, out_h: u32) -> Result<Lut> {
    let aspect = out_h as f64 / out_w as f64;
    let cy = (models[0].middle().1 + models[1].middle().1) / 2.0;
    let limit = stereo_limit(models);
    let widest = stereo_fit(models, aspect, cy, limit);
    if widest <= 0.0 {
        bail!("couldn't find a clean combined view in SteamVR's lens data");
    }
    let h = fov_deg.map_or(widest * AUTO_FOV_MARGIN, |f| (f.to_radians() / 2.0).tan());
    if h > widest {
        log::warn!("the requested combined view reaches beyond the panels, the edges will be black");
    }
    // our picture's corners may be one eye only, then the middle row has to do
    let edges = [cy - h * aspect, cy + h * aspect];
    let overlap = stereo_overlap(models, &edges, limit).or_else(|_| stereo_overlap(models, &[cy], limit))?;
    let mut lut = Lut::new(out_w, out_h);
    lut.blended = true;
    let count = (lut.width * lut.height) as usize;
    lut.points.resize(count * 7, INVALID);
    let mut guesses = [[[0.5; 2]; 3]; 2];
    let mut previous = [None; 2];
    let mut max_split = 0.0f64;
    let mut max_minify = 0.0f64;
    let cells: Vec<_> = lut.grid().collect();
    for (idx, x, y) in cells {
        if x == 0.0 { previous = [None; 2]; }
        let tx = (x / out_w as f64 * 2.0 - 1.0) * h;
        let ty = cy + (y / out_h as f64 * 2.0 - 1.0) * h * aspect;
        for (eye, model) in models.iter().enumerate() {
            let mut solved = [None; 3];
            if model.shown(tx, ty) {
                for c in 0..3 {
                    if let Some(d) = model.solve(c, model.target(tx, ty), guesses[eye][c]) {
                        guesses[eye][c] = d;
                        solved[c] = Some(d);
                        lut.points[eye * count * 3 + idx + c] = [(eye as f64 * 0.5 + d[0] * 0.5) as f32, d[1] as f32];
                    }
                }
            }
            if let (Some(r), Some(g), Some(b), Some(prev)) = (solved[0], solved[1], solved[2], previous[eye]) {
                let scale = dist(g, prev) / STEP as f64;
                if scale > 0.0 {
                    max_split = max_split.max(dist(r, g).max(dist(b, g)) / scale);
                    max_minify = max_minify.max(scale * EYE_PX);
                }
            }
            previous[eye] = solved[1];
        }
        let weight = match (lut.points[idx + 1] != INVALID, lut.points[count * 3 + idx + 1] != INVALID) {
            (true, false) => 0.0,
            (false, true) => 1.0,
            _ => blend_weight(tx, overlap),
        };
        lut.points[count * 6 + idx / 3] = [weight as f32, 0.0];
    }
    lut.per_channel = max_split > MIN_FRINGE_PX;
    lut.supersample = max_minify > SUPERSAMPLE_ABOVE;
    log::info!("combined view: {:.1}° x {:.1}°, overlap {:.1}° to {:.1}°", 2.0 * h.atan().to_degrees(),
        2.0 * (h * aspect).atan().to_degrees(), overlap[0].atan().to_degrees(), overlap[1].atan().to_degrees());
    Ok(lut)
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// Bilinear read of the sampled forward mapping. None outside the panel.
fn eval(forward: &[[[f64; 2]; 3]], n: usize, c: usize, d: [f64; 2]) -> Option<[f64; 2]> {
    if !(0.0..=1.0).contains(&d[0]) || !(0.0..=1.0).contains(&d[1]) {
        return None;
    }
    let gx = d[0] * (n - 1) as f64;
    let gy = d[1] * (n - 1) as f64;
    let (i, j) = ((gx.floor() as usize).min(n - 2), (gy.floor() as usize).min(n - 2));
    let (fx, fy) = (gx - i as f64, gy - j as f64);
    let p = |ii: usize, jj: usize| forward[jj * n + ii][c];
    let (a, b, e, f) = (p(i, j), p(i + 1, j), p(i, j + 1), p(i + 1, j + 1));
    let out = [0, 1].map(|k| {
        let top = a[k] + (b[k] - a[k]) * fx;
        let bottom = e[k] + (f[k] - e[k]) * fx;
        top + (bottom - top) * fy
    });
    out.iter().all(|v| v.is_finite()).then_some(out)
}

/// Newton's method on the forward mapping.
fn invert(forward: &[[[f64; 2]; 3]], n: usize, c: usize, target: [f64; 2], start: [f64; 2]) -> Option<[f64; 2]> {
    const H: f64 = 1e-4;
    let mut d = start;
    for _ in 0..24 {
        let f = eval(forward, n, c, d)?;
        let err = [f[0] - target[0], f[1] - target[1]];
        if err[0].abs() < 1e-7 && err[1].abs() < 1e-7 {
            return Some(d);
        }
        let dx0 = eval(forward, n, c, [(d[0] - H).max(0.0), d[1]])?;
        let dx1 = eval(forward, n, c, [(d[0] + H).min(1.0), d[1]])?;
        let dy0 = eval(forward, n, c, [d[0], (d[1] - H).max(0.0)])?;
        let dy1 = eval(forward, n, c, [d[0], (d[1] + H).min(1.0)])?;
        let hx = (d[0] + H).min(1.0) - (d[0] - H).max(0.0);
        let hy = (d[1] + H).min(1.0) - (d[1] - H).max(0.0);
        let j = [
            [(dx1[0] - dx0[0]) / hx, (dy1[0] - dy0[0]) / hy],
            [(dx1[1] - dx0[1]) / hx, (dy1[1] - dy0[1]) / hy],
        ];
        let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
        if det.abs() < 1e-12 {
            return None;
        }
        let step = [
            (j[1][1] * err[0] - j[0][1] * err[1]) / det,
            (-j[1][0] * err[0] + j[0][0] * err[1]) / det,
        ];
        d = [(d[0] - step[0]).clamp(0.0, 1.0), (d[1] - step[1]).clamp(0.0, 1.0)];
    }
    let f = eval(forward, n, c, d)?;
    (dist(f, target) < 1e-5).then_some(d)
}

fn nearest(forward: &[[[f64; 2]; 3]], n: usize, c: usize, target: [f64; 2]) -> Option<[f64; 2]> {
    let (best, _) = forward
        .iter()
        .enumerate()
        .filter(|(_, p)| p[c][0].is_finite())
        .map(|(k, p)| (k, dist(p[c], target)))
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    Some([(best % n) as f64 / (n - 1) as f64, (best / n) as f64 / (n - 1) as f64])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eyes() -> [EyeModel; 2] {
        std::array::from_fn(|eye| EyeModel {
            eye,
            l: if eye == 0 { -2.0 } else { -0.7 },
            r: if eye == 0 { 0.7 } else { 2.0 },
            t: -1.0,
            b: 1.0,
            roll: 0.0,
            cant: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            forward: (0..DISTORTION_GRID * DISTORTION_GRID).map(|k| {
                [[(k % DISTORTION_GRID) as f64 / (DISTORTION_GRID - 1) as f64,
                  (k / DISTORTION_GRID) as f64 / (DISTORTION_GRID - 1) as f64]; 3]
            }).collect(),
            hidden: vec![false; MASK_SIZE * MASK_SIZE],
            covered: vec![true; MASK_SIZE * MASK_SIZE],
        })
    }

    #[test]
    fn combined_view_fits_each_shape_and_uses_the_stereo_overlap() {
        let models = eyes();
        let limit = stereo_limit(&models);
        let overlap = stereo_overlap(&models, &[-0.5, 0.0, 0.5], limit).unwrap();
        assert!((overlap[0] + 0.7).abs() < 0.01 && (overlap[1] - 0.7).abs() < 0.01);
        // the left eye only out to the right eye's inner edge, then a linear
        // fade over a tenth of the overlap, then the right eye
        let width = overlap[1] - overlap[0];
        for (tx, expected) in [(-0.8, 0.0), (overlap[0], 0.0), (overlap[0] + width * 0.05, 0.5), (overlap[0] + width * 0.1, 1.0), (0.0, 1.0), (0.5, 1.0)] {
            assert!((blend_weight(tx, overlap) - expected).abs() < 1e-9, "{tx}");
        }
        for (w, h, expected) in [(256, 144, 16.0 / 9.0), (96, 96, 1.0), (144, 256, 9.0 / 16.0)] {
            let fitted = stereo_fit(&models, h as f64 / w as f64, 0.0, limit);
            assert!((fitted - expected).abs() < 1e-6);
            let lut = stereo_lut(&models, None, w, h).unwrap();
            let count = (lut.width * lut.height) as usize;
            assert!(lut.blended);
            if w == h {
                let center = (lut.height / 2 * lut.width + lut.width / 2) as usize;
                assert!((lut.points[center * 3 + 1][0] - 10.0 / 27.0).abs() < 1e-6);
                assert!((lut.points[count * 3 + center * 3 + 1][0] - 17.0 / 27.0).abs() < 1e-6);
                assert_eq!(lut.points[count * 6 + center][0], 1.0);
            }
            for j in 0..lut.height - 1 {
                for i in 0..lut.width - 1 {
                    let valid_cell = |eye: usize| [0, 1, lut.width, lut.width + 1].iter().all(|off| {
                        lut.points[eye * count * 3 + ((j * lut.width + i + off) * 3 + 1) as usize] != INVALID
                    });
                    assert!(valid_cell(0) || valid_cell(1), "black cell at {i},{j} in {w}x{h}");
                }
            }
        }
        assert!(stereo_fit(&models, 9.0 / 16.0, 0.0, limit) > models[0].widest(9.0 / 16.0).0);
    }

    #[test]
    fn hidden_area_uses_the_other_eye_and_is_checked_before_cant() {
        let mut models = eyes();
        let x = ((0.0 - models[0].l) / (models[0].r - models[0].l) * MASK_SIZE as f64) as usize;
        models[0].hidden[MASK_SIZE / 2 * MASK_SIZE + x] = true;
        models[0].cant[0][2] = -0.2;
        assert!(!models[0].shown(0.0, 0.0));
        assert!(models[1].shown(0.0, 0.0));
        let lut = stereo_lut(&models, Some(60.0), 96, 96).unwrap();
        let count = (lut.width * lut.height) as usize;
        let center = (lut.height / 2 * lut.width + lut.width / 2) as usize;
        assert_eq!(lut.points[center * 3 + 1], INVALID);
        assert_ne!(lut.points[count * 3 + center * 3 + 1], INVALID);
        assert_eq!(lut.points[count * 6 + center][0], 1.0);
        models[1].hidden.fill(true);
        assert!(stereo_lut(&models, None, 96, 96).is_err());
    }
}
