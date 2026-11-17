//! The `vello_cpu` export keeps the shared `fagent-paint-core` strict contract (codes,
//! readiness, diagnostics, draw counters, sizes; checked equal to the former tiny-skia painter
//! until it was removed) and its PNG bytes are deterministic.
use crate::export;
use fagent_paint_core::{encode_png, PaintScene, RenderOptions, ResourceBytes, Resources};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::cell::Cell;

const FONT: &[u8] = include_bytes!("../../../renderer/fonts/NotoSans.ttf");

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn png_bytes() -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 3, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let data = [255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255].repeat(2);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&data)
        .unwrap();
    out
}

fn node(id: &str, parent: Option<&str>, kind: &str, b: [f32; 4], extra: Value) -> Value {
    let mut n = json!({"id":id,"parentId":parent,"kind":kind,
        "bounds":{"x":b[0],"y":b[1],"width":b[2],"height":b[3]},"lines":[],"fontId":null,
        "fontSize":16,"lineHeight":24,"fill":"#4682b4","assetId":null,"fit":"cover","opacity":1,
        "clip":false,"radius":6,"crop":null});
    for (k, v) in extra.as_object().unwrap() {
        n[k] = v.clone();
    }
    n
}

/// Text (with a glyph Noto lacks), image, gradient, shadow, stroke, clip and group opacity.
fn scene() -> Value {
    let text = node(
        "text",
        Some("card"),
        "text",
        [6.0, 4.0, 70.0, 30.0],
        json!({
        "fontId":"noto","fill":"#183860","fontSize":15,"lineHeight":20,
        "lines":[{"text":"Café ⌘ fj","x":0,"y":16,"width":66}],
        "decoration":{"line":"underline","offset":2,"thickness":1}}),
    );
    json!({"schemaVersion":1,"coordinateSpace":"frame-local",
        "provenance":{"projectId":"p","revision":1,"frameId":"f","viewport":{"width":96,"height":72},
            "theme":"default","state":"default","sceneVersion":"scene-3","rendererVersion":"web-4",
            "sourceSceneKey":"key","fontManifestHash":"fonts","assetManifestHash":"assets"},
        "nodes":[
            node("card", None, "frame", [4.0, 4.0, 86.0, 62.0], json!({"clip":true,"opacity":0.8,
                "fill":{"type":"linear","angle":30,"stops":[[0,"#ffeeddff"],[1,"#2255dd88"]]},
                "shadow":[{"x":2,"y":3,"blur":6,"spread":1,"color":"#15273899","inset":false}],
                "stroke":{"align":"inside","width":[2,2,2,2],"color":["#223344","#223344","#223344","#223344"]}})),
            text,
            node("image", Some("card"), "image", [40.0, 30.0, 40.0, 28.0], json!({"assetId":"img","fill":"transparent"})),
        ],
        "fonts":[{"id":"noto","family":"Noto","weight":400,"style":"normal","hash":sha(FONT),"url":"unused"}],
        "assets":[{"id":"img","mediaType":"image/png","hash":sha(&png_bytes()),"url":"unused"}],
        "diagnostics":[]})
}

fn parse(v: Value) -> PaintScene {
    serde_json::from_value(v).unwrap()
}

/// The observable export result: Ok(size, counters, readiness, diagnostics) or Err(code, readiness).
fn outcome(r: Result<fagent_paint_core::Raster, fagent_paint_core::RenderError>) -> Value {
    match r {
        Ok(r) => json!({"size":[r.width,r.height],"lines":r.draw.drawn_lines,
            "readiness":r.readiness,"diagnostics":r.diagnostics,"key":r.source_scene_key}),
        Err(e) => json!({"code":e.code,"readiness":e.readiness}),
    }
}

fn export_outcome(
    scene: &PaintScene,
    fonts: Option<&[u8]>,
    image: Option<&[u8]>,
    scale: f64,
) -> Value {
    let font_hash = fonts.map(sha).unwrap_or_default();
    let image_hash = image.map(sha).unwrap_or_default();
    let mut r = Resources::default();
    if let Some(bytes) = fonts {
        r.fonts.insert(
            "noto".into(),
            ResourceBytes {
                hash: &font_hash,
                bytes,
            },
        );
    }
    if let Some(bytes) = image {
        r.assets.insert(
            "img".into(),
            ResourceBytes {
                hash: &image_hash,
                bytes,
            },
        );
    }
    let o = RenderOptions {
        scale,
        ..Default::default()
    };
    outcome(export::render(scene, &r, &o))
}

/// Expected code, scene, font bytes, image bytes, scale.
type Case<'a> = (&'a str, PaintScene, Option<&'a [u8]>, Option<&'a [u8]>, f64);

#[test]
fn strict_contract_codes_readiness_and_diagnostics() {
    let png = png_bytes();
    let ok = parse(scene());
    let vello = export_outcome(&ok, Some(FONT), Some(&png), 2.0);
    assert_eq!(vello["size"], json!([192, 144]));
    assert_eq!(vello["lines"], 1);
    assert_eq!(vello["key"], "key");
    assert_eq!(
        vello["readiness"],
        json!([{"id":"img","kind":"asset","status":"ready"},
            {"id":"noto","kind":"font","status":"ready"}])
    );
    let diagnostics = vello["diagnostics"].to_string();
    assert!(diagnostics.contains("MISSING_GLYPH"), "{diagnostics}");

    let mut cases: Vec<Case> = vec![
        ("RESOURCE_NOT_READY", ok.clone(), None, Some(&png), 1.0),
        ("PIXEL_BUDGET", ok.clone(), Some(FONT), Some(&png), 0.0),
        ("PIXEL_BUDGET", ok.clone(), Some(FONT), Some(&png), 4.5),
    ];
    let mut corrupt = scene();
    corrupt["assets"][0]["hash"] = json!(sha(b"not an image"));
    cases.push((
        "RESOURCE_NOT_READY",
        parse(corrupt),
        Some(FONT),
        Some(b"not an image"),
        1.0,
    ));
    let mut version = scene();
    version["provenance"]["rendererVersion"] = json!("web-3");
    cases.push((
        "PIPELINE_MISMATCH",
        parse(version),
        Some(FONT),
        Some(&png),
        1.0,
    ));
    let mut hash = scene();
    hash["fonts"][0]["hash"] = json!("0".repeat(64));
    cases.push((
        "PIPELINE_MISMATCH",
        parse(hash),
        Some(FONT),
        Some(&png),
        1.0,
    ));
    let mut duplicate = scene();
    duplicate["nodes"][2]["id"] = json!("text");
    cases.push((
        "SCENE_INVALID",
        parse(duplicate),
        Some(FONT),
        Some(&png),
        1.0,
    ));
    let mut cycle = scene();
    cycle["nodes"][0]["parentId"] = json!("text");
    cases.push(("SCENE_INVALID", parse(cycle), Some(FONT), Some(&png), 1.0));
    let mut color = scene();
    color["nodes"][0]["shadow"][0]["color"] = json!("red");
    cases.push(("SCENE_INVALID", parse(color), Some(FONT), Some(&png), 1.0));
    for (code, s, font, image, scale) in cases {
        let vello = export_outcome(&s, font, image, scale);
        assert_eq!(vello["code"], code, "{vello}");
        assert!(vello["readiness"].is_array(), "{vello}");
    }
}

#[test]
fn cancellation_is_cooperative_and_reported() {
    let png = png_bytes();
    let (font_hash, image_hash) = (sha(FONT), sha(&png));
    let mut r = Resources::default();
    r.fonts.insert(
        "noto".into(),
        ResourceBytes {
            hash: &font_hash,
            bytes: FONT,
        },
    );
    r.assets.insert(
        "img".into(),
        ResourceBytes {
            hash: &image_hash,
            bytes: &png,
        },
    );
    let s = parse(scene());
    // Cancel at every check in turn: never a raster, always CANCELLED.
    for after in 0..40 {
        let calls = Cell::new(0);
        let cancelled = || {
            calls.set(calls.get() + 1);
            calls.get() > after
        };
        let o = RenderOptions {
            cancelled: Some(&cancelled),
            ..Default::default()
        };
        match export::render(&s, &r, &o) {
            Err(e) => assert_eq!(e.code, "CANCELLED"),
            Ok(_) => return assert!(after > 3, "finished after {after} checks"),
        }
    }
    panic!("export never completed");
}

#[test]
fn png_bytes_are_deterministic_across_runs() {
    let png = png_bytes();
    let (font_hash, image_hash) = (sha(FONT), sha(&png));
    let render = || {
        let mut r = Resources::default();
        r.fonts.insert(
            "noto".into(),
            ResourceBytes {
                hash: &font_hash,
                bytes: FONT,
            },
        );
        r.assets.insert(
            "img".into(),
            ResourceBytes {
                hash: &image_hash,
                bytes: &png,
            },
        );
        let o = RenderOptions {
            scale: 2.4,
            ..Default::default()
        };
        encode_png(&export::render(&parse(scene()), &r, &o).unwrap()).unwrap()
    };
    let first = render();
    for _ in 0..3 {
        assert_eq!(sha(&render()), sha(&first));
    }
}
