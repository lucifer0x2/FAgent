//! Persistent frame records (Task 42): what `compute` stages for one frame, written so the
//! viewer Worker can keep it in its own store (IndexedDB) and stage it again on a later load
//! without recomputing. A record never goes to the page.
//!
//! JSON through `serde_json` with exact floats: every `f32` reads back bit for bit (see
//! [`ExactFloats`]) and `f64` uses the crate's `float_roundtrip` parsing, so an imported
//! frame paints, hit-tests and measures exactly like the computed one.
use crate::frame::Frame;
use fagent_layout::{Asset, Font, Provenance, Scene, SceneNode};
use fagent_scene_wasm::StorageError;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

/// Bumped whenever the layout of a record changes; the Worker also keys its store by it.
pub const RECORD_FORMAT: u32 = 1;

/// Written from a committed frame, borrowing its paint scene (no second copy in memory).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordOut<'a> {
    format: u32,
    project_id: &'a str,
    frame_id: &'a str,
    /// The requested viewport, which the digest covers (a hug frame's scene is taller).
    width: f64,
    height: f64,
    digest: &'a str,
    diagnostics: &'a [serde_json::Value],
    scene: SceneOut<'a>,
}

/// Serializes exactly like `fagent_layout::Scene` (same field names and order).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SceneOut<'a> {
    schema_version: u32,
    coordinate_space: &'a str,
    provenance: &'a Provenance,
    nodes: &'a [SceneNode],
    fonts: &'a [Font],
    assets: &'a [Asset],
    diagnostics: &'a [String],
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    pub format: u32,
    pub project_id: String,
    pub frame_id: String,
    pub width: f64,
    pub height: f64,
    pub digest: String,
    pub diagnostics: Vec<serde_json::Value>,
    pub scene: Scene,
}

/// What a request expects a record to answer.
pub struct Expected<'a> {
    pub project_id: &'a str,
    pub frame_id: &'a str,
    pub width: f64,
    pub height: f64,
    pub digest: &'a str,
}

fn error(code: &str, message: &str) -> StorageError {
    StorageError::new(code, message)
}

pub fn encode(frame: &Frame) -> Result<Vec<u8>, StorageError> {
    if frame.digest.is_empty() {
        return Err(error(
            "RECORD_UNAVAILABLE",
            "A frame without a content digest has no record",
        ));
    }
    let source = &frame.source;
    let record = RecordOut {
        format: RECORD_FORMAT,
        project_id: &frame.summary.project_id,
        frame_id: &frame.summary.frame_id,
        width: source.request[0],
        height: source.request[1],
        digest: &frame.digest,
        diagnostics: &frame.summary.diagnostics,
        scene: SceneOut {
            schema_version: source.schema_version,
            coordinate_space: &frame.summary.coordinate_space,
            provenance: &source.provenance,
            nodes: &frame.scene.nodes,
            fonts: &frame.scene.fonts,
            assets: &frame.scene.assets,
            diagnostics: &source.diagnostics,
        },
    };
    let mut bytes = Vec::with_capacity(64 * 1024);
    let mut writer = serde_json::Serializer::with_formatter(&mut bytes, ExactFloats);
    record.serialize(&mut writer)?;
    Ok(bytes)
}

/// Parses `bytes` and checks they answer `expected`; anything else is rejected, never staged.
pub fn decode(bytes: &[u8], expected: &Expected) -> Result<Record, StorageError> {
    let record: Record = serde_json::from_slice(bytes)
        .map_err(|e| error("RECORD_INVALID", &format!("Unreadable frame record: {e}")))?;
    if record.format != RECORD_FORMAT {
        return Err(error(
            "RECORD_FORMAT",
            &format!(
                "Frame record format {} is not {RECORD_FORMAT}",
                record.format
            ),
        ));
    }
    let p = &record.scene.provenance;
    let answers = record.project_id == expected.project_id
        && p.project_id == expected.project_id
        && record.frame_id == expected.frame_id
        && p.frame_id == expected.frame_id
        && record.width == expected.width
        && record.height == expected.height
        && !expected.digest.is_empty()
        && record.digest == expected.digest;
    if !answers {
        return Err(error(
            "RECORD_MISMATCH",
            "Frame record does not answer this request",
        ));
    }
    Ok(record)
}

/// `serde_json` reads every number as `f64` and narrows it, so the shortest `f32` digits can
/// in rare cases land on a neighbouring `f32` (double rounding). Such values are written
/// with the digits of their exact `f64` widening instead; everything else stays shortest.
struct ExactFloats;

impl serde_json::ser::Formatter for ExactFloats {
    fn write_f32<W: ?Sized + Write>(&mut self, writer: &mut W, value: f32) -> io::Result<()> {
        // Only finite values reach this (serde_json writes `null` for the others).
        let mut buffer = [0u8; 64];
        let mut cursor = io::Cursor::new(&mut buffer[..]);
        write!(cursor, "{value:?}")?;
        let len = cursor.position() as usize;
        let digits = &buffer[..len];
        let exact = std::str::from_utf8(digits)
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .is_some_and(|wide| (wide as f32).to_bits() == value.to_bits());
        if exact {
            writer.write_all(digits)
        } else {
            write!(writer, "{:?}", f64::from(value))
        }
    }
}

#[cfg(test)]
pub(crate) fn exact_json<T: Serialize>(value: &T) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut writer = serde_json::Serializer::with_formatter(&mut bytes, ExactFloats);
    value.serialize(&mut writer).unwrap();
    bytes
}
