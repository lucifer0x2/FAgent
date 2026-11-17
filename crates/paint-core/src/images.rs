//! Bounded in-memory PNG/JPEG/WebP decode, EXIF orientation and ICC to sRGB, shared by
//! every painter so color management and orientation never differ between backends.
use crate::{RenderError, RenderOptions};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use moxcms::DataColorSpace;
use std::io::Cursor;

/// Oriented sRGB image, row-major PREMULTIPLIED RGBA8, stride = width * 4.
#[derive(Clone, Debug)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// Whether each output axis runs against the encoded image's axis (EXIF orientation);
    /// painters that quantize sampling in source axes need it.
    pub reversed: [bool; 2],
}

const MAX_BYTES: usize = 64 * 1024 * 1024;
fn error() -> RenderError {
    RenderError::new("RESOURCE_NOT_READY", "Image decode or color profile failed")
}

pub fn decode_image(
    bytes: &[u8],
    media: &str,
    options: &RenderOptions<'_>,
) -> Result<DecodedImage, RenderError> {
    if bytes.len() > MAX_BYTES {
        return Err(error());
    }
    let format = match media {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        "image/webp" => ImageFormat::WebP,
        _ => return Err(error()),
    };
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    limits.max_image_width = Some(16_000_000);
    limits.max_image_height = Some(16_000_000);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|_| error())?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 16_000_000 {
        return Err(error());
    }
    let profile = crate::icc::profile(decoder.icc_profile().map_err(|_| error())?)?;
    let orientation = decoder
        .exif_metadata()
        .map_err(|_| error())?
        .and_then(|e| image::metadata::Orientation::from_exif_chunk(&e))
        .unwrap_or(decoder.orientation().map_err(|_| error())?);
    options.check()?;
    let mut image = if let Some(src) = profile
        .as_ref()
        .filter(|p| p.color_space == DataColorSpace::Cmyk)
    {
        if format != ImageFormat::Jpeg {
            return Err(RenderError::new(
                "UNSUPPORTED_FEATURE",
                "CMYK ICC requires CMYK JPEG source channels",
            ));
        }
        drop(decoder);
        DynamicImage::ImageRgba8(crate::icc::cmyk(bytes, src, w, h, options)?)
    } else {
        let mut image = DynamicImage::from_decoder(decoder)
            .map_err(|_| error())?
            .to_rgba8();
        if let Some(src) = &profile {
            crate::icc::apply(&mut image, src, options)?;
        }
        DynamicImage::ImageRgba8(image)
    };
    options.check()?;
    image.apply_orientation(orientation);
    let rgba = image.into_rgba8();
    let (width, height) = rgba.dimensions();
    let mut rgba = rgba.into_raw();
    for (i, px) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        if i % 4096 == 0 {
            options.check()?;
        }
        for k in 0..3 {
            px[k] = ((u16::from(px[k]) * u16::from(px[3]) + 127) / 255) as u8;
        }
    }
    use image::metadata::Orientation::*;
    let reversed = match orientation {
        NoTransforms | Rotate90FlipH => [false, false],
        FlipHorizontal | Rotate90 => [true, false],
        FlipVertical | Rotate270 => [false, true],
        Rotate180 | Rotate270FlipH => [true, true],
    };
    Ok(DecodedImage {
        width,
        height,
        rgba,
        reversed,
    })
}
