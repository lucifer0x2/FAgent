//! An image asset to embed: its original bytes and natural size, read from the PNG, JPEG or
//! WebP header by the storage parser (`fagent_storage::image_size`), so a stored record without
//! a size still embeds. A crop needs the natural size for its `viewBox`; a header the parser
//! cannot read fails rather than guessing.
use base64::{engine::general_purpose::STANDARD, Engine};

pub struct Image {
    pub media_type: String,
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl Image {
    /// `None` when the header gives no positive size.
    pub fn new(media_type: &str, bytes: Vec<u8>) -> Option<Self> {
        let (width, height) = fagent_storage::image_size(media_type, &bytes)?;
        Some(Self {
            media_type: media_type.into(),
            bytes,
            width,
            height,
        })
    }

    pub fn data_uri(&self) -> String {
        format!(
            "data:{};base64,{}",
            self.media_type,
            STANDARD.encode(&self.bytes)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Image;

    fn size(mime: &str, bytes: &[u8]) -> Option<(u32, u32)> {
        Image::new(mime, bytes.to_vec()).map(|i| (i.width, i.height))
    }

    /// The header formats themselves are tested with the parser in storage.
    #[test]
    fn uses_the_storage_header_parser() {
        let jpeg = b"\xff\xd8\xff\xe0\x00\x04ab\xff\xff\xc2\x00\x11\x08\x01\xe0\x02\x80\x03";
        assert_eq!(size("image/jpeg", jpeg), Some((640, 480)));
        assert_eq!(size("image/png", b"\x89PNG"), None);
        assert_eq!(size("image/gif", b"GIF89a"), None);
    }

    #[test]
    fn data_uri_embeds_original_bytes() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend([0, 0, 0, 1, 0, 0, 0, 1]);
        let image = Image::new("image/png", png).unwrap();
        assert!(image
            .data_uri()
            .starts_with("data:image/png;base64,iVBORw0KGgo"));
    }
}
