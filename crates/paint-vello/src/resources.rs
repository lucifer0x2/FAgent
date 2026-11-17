//! Fonts and images the encoder can draw, plus caches that persist across
//! encodes: shaped lines and glyph outlines (shaping is the costly part of text).
use fagent_text::{DrawMemory, PathSink, ScalerFont};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use vello_common::kurbo::BezPath;
use vello_common::paint::ImageSource;
use vello_common::peniko::{Blob, FontData};

pub struct Image {
    pub source: ImageSource,
    pub width: u32,
    pub height: u32,
}

pub(crate) use crate::painter::Glyph;

pub(crate) struct Font {
    font: fagent_text::Font,
    /// Font-unit outlines (y up) by (glyph, size bits): optical size varies outlines.
    outlines: HashMap<(u32, u32), Option<Arc<BezPath>>>,
    runs: HashMap<(String, u32, u32), Arc<[Glyph]>>,
    /// Decoded SFNT for backend glyph pipelines, and instance coordinates by size bits.
    pub(crate) data: FontData,
    coords: HashMap<u32, Arc<[i16]>>,
}

#[derive(Default)]
pub struct Resources {
    pub(crate) fonts: BTreeMap<String, Font>,
    pub(crate) images: BTreeMap<String, Image>,
}

impl Resources {
    pub fn new() -> Self {
        Self::default()
    }

    /// Verifies the bytes against the manifest hash and binds the manifest face.
    pub fn add_font(&mut self, manifest: &fagent_layout::Font, bytes: &[u8]) -> Result<(), String> {
        let font = fagent_text::Font::from_bytes(bytes, &manifest.hash)
            .and_then(|f| f.with_face(&manifest.family, manifest.weight, &manifest.style))
            .map_err(|e| format!("font {}: {e}", manifest.id))?;
        self.insert_font(manifest.id.clone(), font);
        Ok(())
    }

    /// Binds an already verified face (e.g. from `fagent-paint-core` preparation).
    pub fn insert_font(&mut self, id: impl Into<String>, font: fagent_text::Font) {
        self.fonts.insert(
            id.into(),
            Font {
                data: FontData::new(Blob::from(font.sfnt_bytes().to_vec()), 0),
                font,
                outlines: HashMap::new(),
                runs: HashMap::new(),
                coords: HashMap::new(),
            },
        );
    }

    pub fn add_image(&mut self, id: impl Into<String>, image: Image) {
        self.images.insert(id.into(), image);
    }

    pub fn has_font(&self, id: &str) -> bool {
        self.fonts.contains_key(id)
    }

    pub fn has_image(&self, id: &str) -> bool {
        self.images.contains_key(id)
    }
}

impl Font {
    pub(crate) fn run(&mut self, text: &str, size: f32, spacing: f32) -> Option<Arc<[Glyph]>> {
        let key = (text.to_owned(), size.to_bits(), spacing.to_bits());
        if let Some(run) = self.runs.get(&key) {
            return Some(run.clone());
        }
        let run = self.font.positioned_run(text, size, spacing).ok()?;
        let glyphs: Arc<[Glyph]> = run
            .glyphs
            .iter()
            .map(|g| Glyph {
                id: g.id.to_u32(),
                x: g.x as f32,
                y: g.y as f32,
            })
            .collect();
        self.runs.insert(key, glyphs.clone());
        Some(glyphs)
    }

    /// Normalized variation coordinates of the instance used at `size` (optical size).
    pub(crate) fn coords(&mut self, size: f32) -> Option<Arc<[i16]>> {
        if let Some(c) = self.coords.get(&size.to_bits()) {
            return Some(c.clone());
        }
        let instance = self.font.shaping_font_at_size(size).ok()?;
        let coords: Arc<[i16]> = instance
            .normalized_coords()
            .iter()
            .map(|c| c.to_bits())
            .collect();
        self.coords.insert(size.to_bits(), coords.clone());
        Some(coords)
    }

    /// Font units per CSS px at `size`, or `None` when the size is invalid.
    pub(crate) fn scale(&self, size: f32) -> Option<f64> {
        let instance = self.font.shaping_font_at_size(size).ok()?;
        Some(f64::from(size) / f64::from(instance.units_per_em()))
    }

    /// Export strictness: the font must instance at `size`, the line
    /// must position, and every glyph with ink must have an outline. Warms the run cache.
    #[cfg(feature = "export")]
    pub(crate) fn check_line(
        &mut self,
        text: &str,
        size: f32,
        spacing: f32,
    ) -> Result<(), (&'static str, &'static str)> {
        let instance = self
            .font
            .shaping_font_at_size(size)
            .map_err(|_| ("TEXT_SHAPING_FAILED", "Could not instance scene font"))?;
        let run = self
            .run(text, size, spacing)
            .ok_or(("TEXT_SHAPING_FAILED", "Could not position scene line"))?;
        for g in run.iter() {
            let ink = instance
                .glyph_metrics()
                .extents(fagent_text::GlyphId::new(g.id))
                .is_some_and(|e| e.width != 0.0 && e.height != 0.0);
            if ink && self.outline(g.id, size).is_none() {
                return Err((
                    "UNSUPPORTED_FEATURE",
                    "Non-outline glyph rasterization is not supported",
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn outline(&mut self, glyph: u32, size: f32) -> Option<Arc<BezPath>> {
        let key = (glyph, size.to_bits());
        if let Some(path) = self.outlines.get(&key) {
            return path.clone();
        }
        let instance = self.font.shaping_font_at_size(size).ok()?;
        let scaler = ScalerFont::unhinted(instance, None);
        let path = scaler
            .outline(fagent_text::GlyphId::new(glyph))
            .and_then(|o| {
                let mut sink = Sink(BezPath::new());
                o.draw(DrawMemory::Scratch, &mut sink).ok()?;
                (!sink.0.elements().is_empty()).then(|| Arc::new(sink.0))
            });
        self.outlines.insert(key, path.clone());
        path
    }
}

struct Sink(BezPath);
impl PathSink for Sink {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to((f64::from(x), f64::from(y)));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to((f64::from(x), f64::from(y)));
    }
    fn quad_to(&mut self, a: f32, b: f32, x: f32, y: f32) {
        self.0
            .quad_to((f64::from(a), f64::from(b)), (f64::from(x), f64::from(y)));
    }
    fn curve_to(&mut self, a: f32, b: f32, c: f32, d: f32, x: f32, y: f32) {
        self.0.curve_to(
            (f64::from(a), f64::from(b)),
            (f64::from(c), f64::from(d)),
            (f64::from(x), f64::from(y)),
        );
    }
    fn close(&mut self) {
        self.0.close_path();
    }
}

/// A CPU image source from premultiplied RGBA8.
#[cfg(feature = "cpu")]
pub fn pixmap_source(rgba: Vec<u8>, width: u32, height: u32) -> Result<Image, String> {
    use vello_common::peniko::ImageAlphaType;
    use vello_common::pixmap::{PixelMetadata, Pixmap};
    let (w, h) = (
        u16::try_from(width).map_err(|_| "image too wide")?,
        u16::try_from(height).map_err(|_| "image too tall")?,
    );
    let pixmap = Pixmap::from_parts(
        rgba,
        w,
        h,
        PixelMetadata::new(ImageAlphaType::AlphaPremultiplied, true),
    );
    Ok(Image {
        source: ImageSource::Pixmap(Arc::new(pixmap)),
        width,
        height,
    })
}
