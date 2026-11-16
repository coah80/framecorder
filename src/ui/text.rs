//! The three typefaces of the UI, rasterized on demand and cached.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use fontdue::{Font, FontSettings};

const HEADING: &[u8] = include_bytes!("../../assets/fonts/Montserrat-Bold.ttf");
const BODY: &[u8] = include_bytes!("../../assets/fonts/Poppins-Regular.ttf");
const BODY_MEDIUM: &[u8] = include_bytes!("../../assets/fonts/Poppins-Medium.ttf");
const DATA: &[u8] = include_bytes!("../../assets/fonts/SpaceGrotesk-Medium.ttf");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    /// Montserrat Bold: titles and button labels.
    Heading,
    /// Montserrat Bold with wide tracking, for small uppercase labels.
    Label,
    /// Poppins: descriptions and captions.
    Body,
    /// Poppins Medium: control text.
    BodyMedium,
    /// Space Grotesk: the timer and file sizes.
    Data,
}

pub struct Glyph {
    pub width: usize,
    pub height: usize,
    pub xmin: i32,
    pub ymin: i32,
    pub advance: f32,
    pub bitmap: Vec<u8>,
}

pub struct Fonts {
    heading: Font,
    body: Font,
    body_medium: Font,
    data: Font,
    cache: HashMap<(Face, u32, char), Glyph>,
}

impl Fonts {
    pub fn load() -> Result<Self> {
        let load = |bytes: &[u8]| Font::from_bytes(bytes, FontSettings::default()).map_err(|e| anyhow!("loading a font: {e}"));
        Ok(Self {
            heading: load(HEADING)?,
            body: load(BODY)?,
            body_medium: load(BODY_MEDIUM)?,
            data: load(DATA)?,
            cache: HashMap::new(),
        })
    }

    fn font(&self, face: Face) -> &Font {
        match face {
            Face::Heading | Face::Label => &self.heading,
            Face::Body => &self.body,
            Face::BodyMedium => &self.body_medium,
            Face::Data => &self.data,
        }
    }

    /// Extra space between letters, as a fraction of the size.
    pub fn tracking(&self, face: Face) -> f32 {
        match face {
            Face::Label => 0.12,
            Face::Heading => -0.01,
            _ => 0.0,
        }
    }

    pub fn glyph(&mut self, face: Face, size: f32, ch: char) -> &Glyph {
        let key = (face, size.to_bits(), ch);
        if !self.cache.contains_key(&key) {
            let (m, bitmap) = self.font(face).rasterize(ch, size);
            let glyph = Glyph {
                width: m.width,
                height: m.height,
                xmin: m.xmin,
                ymin: m.ymin,
                advance: m.advance_width,
                bitmap,
            };
            self.cache.insert(key, glyph);
        }
        &self.cache[&key]
    }

    pub fn measure(&mut self, face: Face, size: f32, s: &str) -> f32 {
        let tracking = self.tracking(face) * size;
        s.chars().map(|c| self.glyph(face, size, c).advance + tracking).sum()
    }

    /// `s` as is when it fits in `max` px, otherwise as much of it as does
    /// with an ellipsis on the end.
    pub fn fit(&mut self, face: Face, size: f32, s: &str, max: f32) -> String {
        if self.measure(face, size, s) <= max {
            return s.to_string();
        }
        let tracking = self.tracking(face) * size;
        let room = max - self.measure(face, size, "…");
        let mut out = String::new();
        let mut width = 0.0;
        for ch in s.chars() {
            width += self.glyph(face, size, ch).advance + tracking;
            if width > room {
                break;
            }
            out.push(ch);
        }
        format!("{}…", out.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_keeps_short_names_and_cuts_long_ones_to_the_width() {
        let mut fonts = Fonts::load().unwrap();
        assert_eq!(fonts.fit(Face::BodyMedium, 18.0, "Pixel 9", 200.0), "Pixel 9");
        let long = "W".repeat(64);
        let cut = fonts.fit(Face::BodyMedium, 18.0, &long, 150.0);
        assert!(cut.ends_with('…') && cut.len() < long.len());
        assert!(fonts.measure(Face::BodyMedium, 18.0, &cut) <= 150.0);
        assert!(fonts.measure(Face::BodyMedium, 18.0, &format!("{cut}W")) > 150.0, "cuts no more than it has to");
    }
}
