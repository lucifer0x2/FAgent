#![allow(dead_code)] // Helpers are shared by independent integration-test binaries.
use fagent_text::{Font, FontSet, MeasureRequest};
use sha2::{Digest, Sha256};

pub const BYTES: &[u8] = include_bytes!("../../../../renderer/fonts/NotoSans.ttf");
pub const HASH: &str = "bfb7bb691513f12e734dc346c03a03f784912432d7e3fa8e56efcf906fe86b3d";

pub fn font() -> Font {
    Font::from_bytes(BYTES, HASH).unwrap()
}

pub fn set() -> FontSet {
    let mut set = FontSet::new();
    set.insert(
        "font_bundled",
        font().with_face("FAgent Noto Sans", 400, "normal").unwrap(),
    )
    .unwrap();
    set
}

pub fn request(text: &str) -> MeasureRequest {
    MeasureRequest {
        key: "test".into(),
        text: text.into(),
        font_id: "font_bundled".into(),
        font_hash: HASH.into(),
        family: "FAgent Noto Sans".into(),
        weight: 400,
        style: "normal".into(),
        font_size: 16.0,
        line_height: 24.0,
        width: None,
        letter_spacing: None,
        text_transform: None,
    }
}

pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
