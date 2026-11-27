use crate::error::{ApiError, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};

/// Pixels one PNG may hold: the render budget shared with `fagent_layout::MAX_PIXELS`,
/// `fagent_scene_input::check_viewport` and the paint backends.
pub const MAX_PIXELS: f64 = 16_000_000.0;
/// Largest render and export scale.
const MAX_SCALE: f64 = 4.0;

/// Device pixels of a `width`×`height` CSS-pixel surface at `scale`, rounded up per side as
/// the paint backends allocate it.
fn pixels(width: f64, height: f64, scale: f64) -> f64 {
    (width * scale).ceil() * (height * scale).ceil()
}

/// The largest scale, in hundredths and at most 4, at which `width`×`height` fits the budget;
/// 0 when not even 0.01 fits.
pub fn max_scale(width: f64, height: f64) -> f64 {
    let mut hundredths = ((MAX_PIXELS / (width * height)).sqrt() * 100.0)
        .floor()
        .min(MAX_SCALE * 100.0);
    while hundredths > 0.0 && pixels(width, height, hundredths / 100.0) > MAX_PIXELS {
        hundredths -= 1.0;
    }
    hundredths.max(0.0) / 100.0
}

/// `PIXEL_BUDGET` for a `width`×`height` surface at `scale`, with the numbers an agent needs to
/// retry: `maxPixels`, `requestedPixels` and `maxScale` (largest scale that fits, two decimals).
pub fn pixel_budget(width: f64, height: f64, scale: f64) -> ApiError {
    let requested = pixels(width, height, scale);
    let max = max_scale(width, height);
    let message = format!(
        "{width}×{height} at scale {scale} needs {} pixels, above the {} pixel budget; use scale {max} or less, or a smaller viewport or node",
        group(requested),
        group(MAX_PIXELS)
    );
    ApiError::new("PIXEL_BUDGET", &message)
        .with_detail("maxPixels", json!(MAX_PIXELS as u64))
        .with_detail("requestedPixels", json!(requested as u64))
        .with_detail("maxScale", json!(max))
}

/// `Err(PIXEL_BUDGET)` when the surface area exceeds the budget (`check_viewport`'s rule,
/// unrounded, so nothing a request check accepts fails here).
pub fn check_pixels(width: f64, height: f64, scale: f64) -> Result<()> {
    if width * height * scale * scale > MAX_PIXELS {
        return Err(pixel_budget(width, height, scale));
    }
    Ok(())
}

/// A paint backend's `PIXEL_BUDGET` for a surface that is over budget once rounded up to whole
/// pixels gains the numbers; any other error, or a budget failure for another reason (such as
/// one layer), passes through unchanged.
pub fn explain(error: ApiError, surface: Option<(f64, f64)>, scale: f64) -> ApiError {
    match surface {
        Some((width, height))
            if error.0.code == "PIXEL_BUDGET" && pixels(width, height, scale) > MAX_PIXELS =>
        {
            pixel_budget(width, height, scale)
        }
        _ => error,
    }
}

/// CSS-pixel size of the surface a scene paints, from its provenance viewport.
pub fn surface(scene: &Value) -> Option<(f64, f64)> {
    let viewport = &scene["provenance"]["viewport"];
    Some((viewport["width"].as_f64()?, viewport["height"].as_f64()?))
}

/// The layout's hug-root budget failure (`LAYOUT_INVALID`, the root grew past the budget at
/// scale 1), restated as `PIXEL_BUDGET` with the limit and the tallest height that fits the
/// viewport width. The grown height itself is not reported by the layout.
pub fn hug_budget(error: ApiError, viewport_width: Option<f64>) -> ApiError {
    if error.0.code != "LAYOUT_INVALID"
        || error.0.message != "Hug root height exceeds the viewport budget"
    {
        return error;
    }
    let Some(width) = viewport_width.filter(|w| *w > 0.0) else {
        return error;
    };
    let max_height = (MAX_PIXELS / width).floor();
    let message = format!(
        "The hug root grew taller than {} px, the most the {} pixel budget allows at width {width}; give the root a fixed height or render a narrower viewport",
        group(max_height),
        group(MAX_PIXELS)
    );
    ApiError::new("PIXEL_BUDGET", &message)
        .with_detail("maxPixels", json!(MAX_PIXELS as u64))
        .with_detail("maxHeight", json!(max_height as u64))
}

/// `16000000` as `16,000,000`.
fn group(n: f64) -> String {
    let digits = format!("{}", n.max(0.0).round() as u64);
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}
/// Validated bytes travel with metadata; record publication never decodes again.
pub(super) struct Painted {
    pub value: Value,
    pub bytes: Vec<u8>,
}
pub(super) fn decode(value: &Value) -> Result<Vec<u8>> {
    let invalid = || {
        ApiError::new(
            "WORKER_FAILED",
            "Worker returned invalid PNG metadata or bytes",
        )
    };
    let bytes = STANDARD
        .decode(value["base64"].as_str().ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR" {
        return Err(invalid());
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if width == 0
        || height == 0
        || u64::from(width) * u64::from(height) > 16_000_000
        || value["width"] != width
        || value["height"] != height
    {
        return Err(invalid());
    }
    Ok(bytes)
}
