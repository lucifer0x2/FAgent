//! Natural image size from a PNG, JPEG or WebP header. Imports record it; assets imported
//! before JPEG and WebP sizes were recorded get it on read (no migration). A header this
//! parser cannot read gives `None` rather than a guess.
use crate::Asset;
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

/// Largest image a new import accepts, in pixels.
pub(crate) const MAX_IMAGE_PIXELS: u64 = 40_000_000;

/// Width and height from the header of `bytes` as `media_type`; `None` when the type is not
/// an image this parser reads or the header gives no positive size.
pub fn image_size(media_type: &str, bytes: &[u8]) -> Option<(u32, u32)> {
    match media_type {
        "image/png" => png(bytes),
        "image/jpeg" => jpeg(bytes),
        "image/webp" => webp(bytes),
        _ => None,
    }
    .filter(|(w, h)| *w > 0 && *h > 0)
}

/// Fills the size of each ready image asset stored without one. `read` returns the asset's
/// bytes; sizes are cached by content hash, which names immutable bytes.
pub(crate) fn fill_missing(assets: &mut [Asset], mut read: impl FnMut(&Asset) -> Option<Vec<u8>>) {
    static SIZES: OnceLock<Mutex<HashMap<String, Option<(u32, u32)>>>> = OnceLock::new();
    for asset in assets.iter_mut().filter(|a| needs_size(a)) {
        let sizes = SIZES.get_or_init(Default::default);
        let cached = sizes
            .lock()
            .ok()
            .and_then(|map| map.get(&asset.content_hash).copied());
        let size = match cached {
            Some(size) => size,
            None => {
                let Some(bytes) = read(asset) else { continue };
                let size = image_size(&asset.media_type, &bytes);
                if let Ok(mut map) = sizes.lock() {
                    // Bounded: only assets imported before sizes were recorded land here.
                    if map.len() >= 4096 {
                        map.clear();
                    }
                    map.insert(asset.content_hash.clone(), size);
                }
                size
            }
        };
        if let Some((width, height)) = size {
            (asset.width, asset.height) = (Some(width), Some(height));
        }
    }
}

/// A ready image asset whose stored record has no size.
pub(crate) fn needs_size(asset: &Asset) -> bool {
    asset.status == "ready"
        && asset.width.is_none()
        && asset.height.is_none()
        && matches!(
            asset.media_type.as_str(),
            "image/png" | "image/jpeg" | "image/webp"
        )
}

fn be16(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from(u16::from_be_bytes(
        b.get(at..at + 2)?.try_into().ok()?,
    )))
}

fn le(b: &[u8], at: usize, len: usize) -> Option<u32> {
    let bytes = b.get(at..at + len)?;
    Some(
        bytes
            .iter()
            .rev()
            .fold(0, |acc, &x| (acc << 8) | u32::from(x)),
    )
}

fn png(b: &[u8]) -> Option<(u32, u32)> {
    if !b.starts_with(b"\x89PNG\r\n\x1a\n") || b.get(12..16)? != b"IHDR" {
        return None;
    }
    let read = |at: usize| Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?));
    Some((read(16)?, read(20)?))
}

/// Walks the marker segments to the first start-of-frame (SOF0–15 except DHT, JPG, DAC).
fn jpeg(b: &[u8]) -> Option<(u32, u32)> {
    if !b.starts_with(b"\xff\xd8") {
        return None;
    }
    let mut at = 2;
    loop {
        while *b.get(at)? == 0xff && *b.get(at + 1)? == 0xff {
            at += 1;
        }
        if *b.get(at)? != 0xff {
            return None;
        }
        let marker = *b.get(at + 1)?;
        match marker {
            0x01 | 0xd0..=0xd7 => at += 2,
            0xd9 | 0xda => return None,
            0xc0..=0xcf if ![0xc4, 0xc8, 0xcc].contains(&marker) => {
                return Some((be16(b, at + 7)?, be16(b, at + 5)?));
            }
            _ => at += 2 + usize::try_from(be16(b, at + 2)?).ok()?,
        }
    }
}

/// Lossy `VP8 `, lossless `VP8L` or extended `VP8X` first chunk.
fn webp(b: &[u8]) -> Option<(u32, u32)> {
    if !b.starts_with(b"RIFF") || b.get(8..12)? != b"WEBP" {
        return None;
    }
    match b.get(12..16)? {
        b"VP8 " if b.get(23..26)? == b"\x9d\x01\x2a" => {
            Some((le(b, 26, 2)? & 0x3fff, le(b, 28, 2)? & 0x3fff))
        }
        b"VP8L" if *b.get(20)? == 0x2f => {
            let bits = le(b, 21, 4)?;
            Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        b"VP8X" => Some((le(b, 24, 3)? + 1, le(b, 27, 3)? + 1)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::image_size;

    #[test]
    fn reads_png_jpeg_and_webp_headers() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend([0, 0, 1, 0x2c, 0, 0, 0, 0xc8]);
        assert_eq!(image_size("image/png", &png), Some((300, 200)));
        // SOI, an APP0 segment of 4 bytes, fill byte, then SOF2 (progressive) 640×480.
        let jpeg = b"\xff\xd8\xff\xe0\x00\x04ab\xff\xff\xc2\x00\x11\x08\x01\xe0\x02\x80\x03";
        assert_eq!(image_size("image/jpeg", jpeg), Some((640, 480)));
        let dht_only = b"\xff\xd8\xff\xc4\x00\x02\xff\xda\x00\x02";
        assert_eq!(image_size("image/jpeg", dht_only), None);
        let mut lossy = b"RIFF\0\0\0\0WEBPVP8 \0\0\0\0\0\0\0\x9d\x01\x2a".to_vec();
        lossy.extend([0x20, 0x03, 0x58, 0x02]);
        assert_eq!(image_size("image/webp", &lossy), Some((800, 600)));
        // 14-bit width-1 = 99 and height-1 = 49 packed little-endian after the 0x2f signature.
        let bits: u32 = 99 | (49 << 14);
        let mut lossless = b"RIFF\0\0\0\0WEBPVP8L\0\0\0\0\x2f".to_vec();
        lossless.extend(bits.to_le_bytes());
        assert_eq!(image_size("image/webp", &lossless), Some((100, 50)));
        let mut extended = b"RIFF\0\0\0\0WEBPVP8X\0\0\0\0\0\0\0\0".to_vec();
        extended.extend([0x0f, 0, 0, 0x07, 0, 0]);
        assert_eq!(image_size("image/webp", &extended), Some((16, 8)));
        assert_eq!(image_size("image/png", b"\x89PNG"), None);
        assert_eq!(image_size("image/gif", b"GIF89a"), None);
    }

    /// Bundles written before JPEG and WebP sizes were recorded keep validating.
    #[test]
    fn sizeless_jpeg_and_webp_records_stay_valid() {
        use crate::assets::stored_dimensions_match as matches;
        let jpeg = b"\xff\xd8\xff\xe0\x00\x04ab\xff\xff\xc2\x00\x11\x08\x01\xe0\x02\x80\x03";
        assert!(matches("image/jpeg", jpeg, (None, None)).unwrap());
        assert!(matches("image/jpeg", jpeg, (Some(640), Some(480))).unwrap());
        assert!(!matches("image/jpeg", jpeg, (Some(1), Some(1))).unwrap());
        assert!(matches("image/jpeg", b"GIF89a", (None, None)).is_err());
    }
}
