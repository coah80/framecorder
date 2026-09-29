//! A tiny software painter: anti-aliased rounded rects, circles and text on
//! an RGBA buffer. The UI redraws a handful of times per interaction, so this
//! never needs the GPU.

use super::text::{Fonts, Face};

pub type Rgb = [u8; 3];

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height, pixels: vec![0; (width * height * 4) as usize] }
    }

    pub fn clear(&mut self, color: Rgb, alpha: f32) {
        let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        for px in self.pixels.as_chunks_mut::<4>().0 {
            *px = [color[0], color[1], color[2], a];
        }
    }

    /// Source-over blend of one pixel with straight alpha.
    fn blend(&mut self, x: i32, y: i32, color: Rgb, alpha: f32) {
        if alpha <= 0.0 || x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let i = ((y as u32 * self.width + x as u32) * 4) as usize;
        let px = &mut self.pixels[i..i + 4];
        let a = alpha.min(1.0);
        let da = px[3] as f32 / 255.0;
        let out_a = a + da * (1.0 - a);
        if out_a <= 0.0 {
            return;
        }
        for c in 0..3 {
            let v = (color[c] as f32 * a + px[c] as f32 * da * (1.0 - a)) / out_a;
            px[c] = v.round().clamp(0.0, 255.0) as u8;
        }
        px[3] = (out_a * 255.0).round() as u8;
    }

    /// Runs `coverage` over every pixel in the box and blends the result.
    fn shade(&mut self, bounds: Rect, color: Rgb, alpha: f32, coverage: impl Fn(f32, f32) -> f32) {
        let x0 = bounds.x.floor().max(0.0) as i32;
        let y0 = bounds.y.floor().max(0.0) as i32;
        let x1 = (bounds.x + bounds.w).ceil().min(self.width as f32) as i32;
        let y1 = (bounds.y + bounds.h).ceil().min(self.height as f32) as i32;
        for y in y0..y1 {
            for x in x0..x1 {
                let c = coverage(x as f32 + 0.5, y as f32 + 0.5);
                if c > 0.0 {
                    self.blend(x, y, color, alpha * c);
                }
            }
        }
    }

    pub fn fill_rrect(&mut self, r: Rect, radius: f32, color: Rgb, alpha: f32) {
        self.shade(r, color, alpha, |x, y| (0.5 - rrect_distance(r, radius, x, y)).clamp(0.0, 1.0));
    }

    pub fn stroke_rrect(&mut self, r: Rect, radius: f32, width: f32, color: Rgb, alpha: f32) {
        self.shade(r, color, alpha, |x, y| {
            let d = rrect_distance(r, radius, x, y);
            let outer = (0.5 - d).clamp(0.0, 1.0);
            let inner = (0.5 - (d + width)).clamp(0.0, 1.0);
            outer - inner
        });
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, radius: f32, color: Rgb, alpha: f32) {
        let b = Rect::new(cx - radius - 1.0, cy - radius - 1.0, radius * 2.0 + 2.0, radius * 2.0 + 2.0);
        self.shade(b, color, alpha, |x, y| {
            let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - radius;
            (0.5 - d).clamp(0.0, 1.0)
        });
    }

    pub fn ring(&mut self, cx: f32, cy: f32, radius: f32, width: f32, color: Rgb, alpha: f32) {
        let b = Rect::new(cx - radius - 1.0, cy - radius - 1.0, radius * 2.0 + 2.0, radius * 2.0 + 2.0);
        self.shade(b, color, alpha, |x, y| {
            let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
            let outer = (0.5 - (d - radius)).clamp(0.0, 1.0);
            let inner = (0.5 - (d - (radius - width))).clamp(0.0, 1.0);
            outer - inner
        });
    }

    /// A hard edged rectangle snapped to whole pixels, for things like QR
    /// codes where anti-aliased seams would get in the way.
    pub fn fill_rect(&mut self, r: Rect, color: Rgb, alpha: f32) {
        let snapped = Rect::new(r.x.round(), r.y.round(), r.w.round(), r.h.round());
        self.shade(snapped, color, alpha, |_, _| 1.0);
    }

    pub fn hline(&mut self, x: f32, y: f32, w: f32, color: Rgb, alpha: f32) {
        self.shade(Rect::new(x, y, w, 1.0), color, alpha, |_, _| 1.0);
    }

    /// Draws text with its baseline at `y`. Returns the advance width.
    #[allow(clippy::too_many_arguments)]
    pub fn text(&mut self, fonts: &mut Fonts, face: Face, size: f32, x: f32, y: f32, s: &str, color: Rgb, alpha: f32) -> f32 {
        let tracking = fonts.tracking(face);
        let mut pen = x;
        for ch in s.chars() {
            let glyph = fonts.glyph(face, size, ch);
            let gx = (pen + glyph.xmin as f32).round() as i32;
            let gy = (y - glyph.ymin as f32 - glyph.height as f32).round() as i32;
            for row in 0..glyph.height {
                for col in 0..glyph.width {
                    let c = glyph.bitmap[row * glyph.width + col] as f32 / 255.0;
                    if c > 0.0 {
                        self.blend(gx + col as i32, gy + row as i32, color, alpha * c);
                    }
                }
            }
            pen += glyph.advance + tracking * size;
        }
        pen - x
    }

    #[allow(clippy::too_many_arguments)]
    pub fn text_centered(&mut self, fonts: &mut Fonts, face: Face, size: f32, cx: f32, y: f32, s: &str, color: Rgb, alpha: f32) {
        let w = fonts.measure(face, size, s);
        self.text(fonts, face, size, cx - w / 2.0, y, s, color, alpha);
    }
}

/// Signed distance from a point to a rounded rectangle's edge (negative inside).
fn rrect_distance(r: Rect, radius: f32, x: f32, y: f32) -> f32 {
    let radius = radius.min(r.w / 2.0).min(r.h / 2.0);
    let (cx, cy) = r.center();
    let qx = (x - cx).abs() - (r.w / 2.0 - radius);
    let qy = (y - cy).abs() - (r.h / 2.0 - radius);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - radius
}
