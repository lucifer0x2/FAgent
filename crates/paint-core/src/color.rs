//! The scene color grammar: `transparent`, `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`.
use crate::RenderError;

/// Straight sRGB RGBA8 of a scene color, or `SCENE_INVALID`.
pub fn rgba8(s: &str) -> Result<[u8; 4], RenderError> {
    if s == "transparent" {
        return Ok([0; 4]);
    }
    let invalid = || RenderError::scene("Invalid paint color");
    let h = s.strip_prefix('#').ok_or_else(invalid)?;
    if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    let h = if h.len() == 3 || h.len() == 4 {
        h.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        h.to_owned()
    };
    if h.len() != 6 && h.len() != 8 {
        return Err(invalid());
    }
    let byte = |i| u8::from_str_radix(&h[i..i + 2], 16).map_err(|_| invalid());
    Ok([
        byte(0)?,
        byte(2)?,
        byte(4)?,
        if h.len() == 8 { byte(6)? } else { 255 },
    ])
}
