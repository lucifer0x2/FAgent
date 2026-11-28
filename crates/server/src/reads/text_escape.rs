//! `textEscaped`: a node's text with characters that clients tend to drop or hide written as
//! JSON `\uXXXX` escapes. The server always sends text as raw UTF-8 (FB-14 confirmed a
//! private-use icon glyph such as U+E40D leaves the server intact); some agent clients still
//! show such text as `""`. The escaped copy is ASCII-visible for those characters and is a valid
//! JSON string body, so it can be written back as is.
use serde_json::{json, Value};
use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};

/// Private-use, format and unassigned characters.
fn hidden(c: char) -> bool {
    matches!(
        c.general_category(),
        GeneralCategory::PrivateUse | GeneralCategory::Format | GeneralCategory::Unassigned
    )
}

/// `text` as a JSON string body with every hidden character as `\uXXXX` (a surrogate pair
/// above U+FFFF), or `None` when it has none, so plain text gets no extra field.
pub(super) fn escaped(text: &str) -> Option<String> {
    if !text.chars().any(hidden) {
        return None;
    }
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
            out.push(c);
        } else if hidden(c) || c.is_control() {
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{unit:04X}"));
            }
        } else {
            out.push(c);
        }
    }
    Some(out)
}

/// Adds `textEscaped` next to the text a read entry shows: `props.text`, else `textPreview`.
pub(super) fn annotate(value: &mut Value) {
    let shown = value["props"]["text"]
        .as_str()
        .or_else(|| value["textPreview"].as_str());
    if let Some(escaped) = shown.and_then(escaped) {
        value["textEscaped"] = json!(escaped);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_characters_are_escaped_and_plain_text_is_untouched() {
        assert_eq!(escaped("\u{E40D}").as_deref(), Some("\\uE40D"));
        assert_eq!(
            escaped("Go \u{E5C8}\u{200B}!").as_deref(),
            Some("Go \\uE5C8\\u200B!")
        );
        assert_eq!(escaped("\u{F0000}").as_deref(), Some("\\uDB80\\uDC00"));
        assert_eq!(escaped("Café 🦄 \u{4E2D}"), None);
        // The escaped form is a JSON string body that decodes to the original text.
        let text = "a\u{E40D}b\u{100001} \"q\" \\ \n";
        let decoded: String =
            serde_json::from_str(&format!("\"{}\"", escaped(text).unwrap())).unwrap();
        assert_eq!(decoded, text);
    }

    #[test]
    fn annotate_uses_props_text_then_the_preview() {
        let mut node = json!({"props":{"text":"\u{E40D}"}});
        annotate(&mut node);
        assert_eq!(node["textEscaped"], "\\uE40D");
        let mut matched = json!({"textPreview":"x\u{E000}"});
        annotate(&mut matched);
        assert_eq!(matched["textEscaped"], "x\\uE000");
        let mut plain = json!({"props":{"text":"Hello"}});
        annotate(&mut plain);
        assert!(plain.get("textEscaped").is_none());
    }
}
