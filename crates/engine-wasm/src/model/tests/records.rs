//! Persistent frame records (`record.rs`): export from a computed frame, import into a new
//! session, rejection of records that do not answer the request, exact floats.
use super::*;

/// Computes both frames at revision 3 with their real digests and returns their records.
fn recorded() -> (Model, Vec<(String, String, Vec<u8>)>) {
    let mut model = loaded(3);
    model.set_layout(vec![place("frame", 0.0), place("other", 300.0)]);
    let mut records = Vec::new();
    for id in ["frame", "other"] {
        let d = digest(&model, 3, id);
        model.compute(3, id, 200.0, 100.0, &d).unwrap();
        assert!(model.commit(id));
        let bytes = model.export_record(id, &d).unwrap();
        records.push((id.to_owned(), d, bytes));
    }
    (model, records)
}

fn board_pixels(model: &mut Model) -> Vec<u8> {
    use fagent_paint_vello::vello_common::pixmap::Pixmap;
    let mut ctx = vello_cpu::RenderContext::new(520, 140);
    let view = View {
        scale: 1.0,
        tx: 10.0,
        ty: 20.0,
        width: 520.0,
        height: 140.0,
        ..View::default()
    };
    let stats = draw(&mut ctx, model, &view);
    assert_eq!(stats.frames, 2);
    assert!(stats.glyphs > 0, "the text is drawn");
    let mut pixmap = Pixmap::new(520, 140);
    ctx.flush();
    ctx.render(&mut pixmap, &mut vello_cpu::Resources::new());
    pixmap.data_as_u8_slice().to_vec()
}

#[test]
fn an_imported_record_draws_hits_and_measures_like_the_computed_frame() {
    let (mut computed, records) = recorded();
    // A later session: same content at a new revision, nothing computed.
    let mut model = Model::new();
    model
        .load_snapshot(&snapshot(7, "Hello engine").to_string())
        .unwrap();
    model.load_font("font", HASH, FONT).unwrap();
    model.set_layout(vec![place("frame", 0.0), place("other", 300.0)]);
    for (id, d, bytes) in &records {
        assert_eq!(&digest(&model, 7, id), d, "digests are revision-free");
        let summary = model.import_record(bytes, 7, id, 200.0, 100.0, d).unwrap();
        let original = computed.frame(id).unwrap().summary.clone();
        assert_eq!(summary.revision, 7);
        assert_eq!(
            Summary {
                revision: 3,
                ..summary
            },
            original
        );
        assert!(model.commit(id));
        assert_eq!(
            model.export_record(id, d).unwrap(),
            *bytes,
            "re-export is identical"
        );
    }
    for (x, y) in [
        (20.0, 20.0),
        (115.0, 15.0),
        (150.0, 15.0),
        (310.0, 10.0),
        (250.0, 10.0),
    ] {
        assert_eq!(
            hit(&model, x, y, 7),
            hit(&computed, x, y, 3),
            "hit at ({x}, {y})"
        );
    }
    let ids = ["frame", "card", "inner", "text", "other"].map(String::from);
    let strip = |b: Vec<NodeBounds>| -> Vec<_> {
        b.into_iter()
            .map(|n| {
                (
                    n.frame_id, n.id, n.kind, n.x, n.y, n.width, n.height, n.lines,
                )
            })
            .collect()
    };
    assert_eq!(
        strip(model.bounds("", Some(7), &ids)),
        strip(computed.bounds("", Some(3), &ids))
    );
    assert_eq!(board_pixels(&mut model), board_pixels(&mut computed));
}

#[test]
fn a_record_holds_exactly_the_scene_the_compute_path_produces() {
    let (_, records) = recorded();
    let mut engine = SceneEngine::new();
    engine.load_font("font", HASH, FONT).unwrap();
    let json_scene = engine
        .compute_json(
            &snapshot(3, "Hello engine").to_string(),
            "frame",
            200.,
            100.,
            "default",
        )
        .unwrap();
    let record: Value = serde_json::from_slice(&records[0].2).unwrap();
    let mut json_scene: Value = serde_json::from_str(&json_scene).unwrap();
    // The JSON path carries the structured diagnostics the record keeps beside its scene.
    assert_eq!(record["diagnostics"], json_scene["diagnostics"]);
    assert!(record["diagnostics"]
        .as_array()
        .is_some_and(|d| !d.is_empty()));
    json_scene["diagnostics"] = record["scene"]["diagnostics"].clone();
    // Through `Scene` and the default serializer: equal text means bit-equal floats.
    let canonical = |scene: Value| {
        serde_json::to_string(&serde_json::from_value::<fagent_layout::Scene>(scene).unwrap())
            .unwrap()
    };
    assert_eq!(canonical(record["scene"].clone()), canonical(json_scene));
    assert_eq!(record["format"], crate::RECORD_FORMAT);
    assert_eq!(record["digest"], records[0].1.as_str());
    assert_eq!(
        (record["width"].as_f64(), record["height"].as_f64()),
        (Some(200.0), Some(100.0))
    );
}

#[test]
fn records_that_do_not_answer_the_request_are_rejected() {
    let (_, records) = recorded();
    let (id, d, bytes) = &records[0];
    let mut model = loaded(4);
    let code = |model: &mut Model, bytes: &[u8], frame: &str, w: f64, d: &str| {
        model
            .import_record(bytes, 4, frame, w, 100.0, d)
            .unwrap_err()
            .code
    };
    let mut edited: Value = serde_json::from_slice(bytes).unwrap();
    edited["format"] = json!(crate::RECORD_FORMAT + 1);
    let other_format = serde_json::to_vec(&edited).unwrap();
    assert_eq!(
        code(&mut model, &other_format, id, 200.0, d),
        "RECORD_FORMAT"
    );
    assert_eq!(
        code(&mut model, bytes, "other", 200.0, d),
        "RECORD_MISMATCH"
    );
    assert_eq!(code(&mut model, bytes, id, 201.0, d), "RECORD_MISMATCH");
    assert_eq!(
        code(&mut model, bytes, id, 200.0, "different"),
        "RECORD_MISMATCH"
    );
    assert_eq!(code(&mut model, bytes, id, 200.0, ""), "RECORD_MISMATCH");
    assert_eq!(
        code(&mut model, &bytes[..bytes.len() / 2], id, 200.0, d),
        "RECORD_INVALID"
    );
    // Another project with the same frame ids and content.
    let mut next = snapshot(4, "Hello engine");
    next["projectId"] = json!("engine-next");
    model.load_snapshot(&next.to_string()).unwrap();
    assert_eq!(code(&mut model, bytes, id, 200.0, d), "RECORD_MISMATCH");
    // No accepted snapshot at this revision.
    assert!(model.import_record(bytes, 9, id, 200.0, 100.0, d).is_err());
    assert!(model.frame(id).is_none() && model.staged_assets(id).is_empty());
}

#[test]
fn a_record_whose_font_is_not_loaded_is_an_error_not_a_frame_without_text() {
    let (_, records) = recorded();
    let (id, d, bytes) = &records[0];
    let mut model = Model::new();
    model
        .load_snapshot(&snapshot(4, "Hello engine").to_string())
        .unwrap();
    let error = model
        .import_record(bytes, 4, id, 200.0, 100.0, d)
        .unwrap_err();
    assert_eq!(error.code, "FONT_MISSING");
    assert!(!model.commit(id), "nothing was staged");
    model.load_font("font", HASH, FONT).unwrap();
    model.import_record(bytes, 4, id, 200.0, 100.0, d).unwrap();
    assert!(model.commit(id));
}

#[test]
fn shown_frames_export_only_with_their_digest_and_list_their_fonts() {
    let (model, records) = recorded();
    let (id, d, _) = &records[0];
    assert!(model.shows(id, d));
    assert!(!model.shows(id, "other") && !model.shows(id, "") && !model.shows("nope", d));
    assert_eq!(
        model.export_record(id, "stale").unwrap_err().code,
        "RECORD_UNAVAILABLE"
    );
    assert_eq!(
        model.record_fonts(id),
        vec![("font".to_owned(), HASH.to_owned())]
    );
    // Harness scenes carry no digest, so they have no record.
    let mut harness = loaded(3);
    harness.set_layout(vec![place("frame", 0.0)]);
    harness.compute(3, "frame", 200.0, 100.0, "").unwrap();
    harness.commit("frame");
    assert!(harness.export_record("frame", "").is_err());
}

#[test]
fn record_floats_read_back_bit_for_bit() {
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Floats(Vec<f32>, Vec<f64>);
    let mut singles = vec![
        0.0,
        -0.0,
        0.1,
        1.0 / 3.0,
        1.5258789e-5,
        f32::MIN_POSITIVE,
        f32::from_bits(1),
        f32::MAX,
        -f32::MAX,
        16_777_215.0,
        123_456.79,
    ];
    // A deterministic sample over every finite bit pattern class (xorshift).
    let mut state = 0x2545_f491_u32;
    while singles.len() < 200_000 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let value = f32::from_bits(state);
        if value.is_finite() {
            singles.push(value);
        }
    }
    let doubles = vec![0.1, 1440.0, 1.0 / 3.0, f64::MIN_POSITIVE, 5e-324, f64::MAX];
    let bytes = crate::record::exact_json(&Floats(singles.clone(), doubles.clone()));
    let Floats(back, wide) = serde_json::from_slice(&bytes).unwrap();
    let bits = |v: &[f32]| v.iter().map(|f| f.to_bits()).collect::<Vec<_>>();
    assert_eq!(bits(&back), bits(&singles));
    assert_eq!(
        wide.iter().map(|f| f.to_bits()).collect::<Vec<_>>(),
        doubles.iter().map(|f| f.to_bits()).collect::<Vec<_>>()
    );
}
