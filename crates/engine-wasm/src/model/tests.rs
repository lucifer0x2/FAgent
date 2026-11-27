use super::*;
use crate::draw::{draw, View};
use crate::frame::Placement;
use fagent_scene_wasm::SceneEngine;
use serde_json::{json, Value};

const FONT: &[u8] = include_bytes!("../../../../renderer/fonts/NotoSans.ttf");
const HASH: &str = "bfb7bb691513f12e734dc346c03a03f784912432d7e3fa8e56efcf906fe86b3d";

fn snapshot(revision: u64, title: &str) -> Value {
    json!({
        "schemaVersion":4,"projectId":"engine","slug":"test","name":"Test","revision":revision,
        "nodes":[
            {"id":"page","parentId":null,"sortKey":0,"kind":"page","props":{}},
            {"id":"frame","parentId":"page","sortKey":0,"kind":"frame",
                "props":{"width":200,"height":100,"clip":true,"fill":"#ffffff"}},
            {"id":"card","parentId":"frame","sortKey":0,"kind":"frame",
                "props":{"x":10,"y":10,"width":120,"height":60,"fill":"#2255ff","clip":true}},
            {"id":"inner","parentId":"card","sortKey":0,"kind":"frame",
                "props":{"x":100,"y":0,"width":80,"height":30,"fill":"#ff0000"}},
            {"id":"text","parentId":"frame","sortKey":1,"kind":"text",
                "props":{"text":title,"fontId":"font","fontSize":16,"lineHeight":24,
                    "width":160,"height":"hug","x":180,"y":70}},
            {"id":"other","parentId":"page","sortKey":1,"kind":"frame",
                "props":{"width":100,"height":100,"fill":"#00ff00"}}
        ],
        "tokens":[],"styles":[],"assets":[],
        "fonts":[{"id":"font","family":"Noto Sans","weight":400,"style":"normal",
            "assetId":null,"bundledId":"noto","contentHash":HASH,"license":"OFL","status":"ready"}],
        "retention":{"oldestCommitRevision":0,"idempotencyPolicy":"bounded"}
    })
}

fn loaded(revision: u64) -> Model {
    let mut model = Model::new();
    model
        .load_snapshot(&snapshot(revision, "Hello engine").to_string())
        .unwrap();
    model.load_font("font", HASH, FONT).unwrap();
    model
}

fn place(id: &str, x: f64) -> Placement {
    Placement {
        id: id.into(),
        x,
        y: 0.0,
        width: 200.0,
        height: 100.0,
    }
}

fn digest(model: &Model, revision: u64, frame: &str) -> String {
    let keys: Value = serde_json::from_str(
        &model
            .frame_keys(
                revision,
                &json!([{"frameId":frame,"width":200,"height":100}]).to_string(),
            )
            .unwrap(),
    )
    .unwrap();
    keys[0]["digest"].as_str().unwrap().to_owned()
}

#[test]
fn summary_matches_the_json_scene_the_canvas2d_path_receives() {
    let mut model = loaded(3);
    let summary = model.compute(3, "frame", 200.0, 100.0, "d").unwrap();
    let mut engine = SceneEngine::new();
    engine.load_font("font", HASH, FONT).unwrap();
    let scene: Value = serde_json::from_str(
        &engine
            .compute_json(
                &snapshot(3, "Hello engine").to_string(),
                "frame",
                200.,
                100.,
                "default",
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(json!(summary.diagnostics), scene["diagnostics"]);
    assert_eq!(summary.node_count, scene["nodes"].as_array().unwrap().len());
    assert_eq!(
        summary.source_scene_key,
        scene["provenance"]["sourceSceneKey"]
    );
    assert_eq!(summary.fonts, vec!["font".to_owned()]);
    assert!(
        summary
            .diagnostics
            .iter()
            .any(|d| d["code"] == "TEXT_CONTENT_OUTSIDE_BOX"),
        "the text overflows the clipping frame"
    );
}

#[test]
fn staged_frames_are_invisible_until_committed() {
    let mut model = loaded(3);
    model.set_layout(vec![place("frame", 0.0)]);
    model.compute(3, "frame", 200.0, 100.0, "d").unwrap();
    assert!(hit(&model, 20.0, 20.0, 3).is_none());
    assert!(model.commit("frame"));
    assert_eq!(hit(&model, 20.0, 20.0, 3), pair("frame", "card"));
}

#[test]
fn hit_test_follows_clip_ancestors_revision_and_board_offsets() {
    let mut model = loaded(3);
    model.set_layout(vec![place("frame", 0.0), place("other", 300.0)]);
    for id in ["frame", "other"] {
        model.compute(3, id, 200.0, 100.0, "d").unwrap();
        model.commit(id);
    }
    // `inner` sticks out of the clipping `card`: only its visible part hits.
    assert_eq!(hit(&model, 115.0, 15.0, 3), pair("frame", "inner"));
    assert_eq!(hit(&model, 150.0, 15.0, 3), pair("frame", "frame"));
    assert_eq!(hit(&model, 310.0, 10.0, 3), pair("other", "other"));
    assert_eq!(hit(&model, 250.0, 10.0, 3), None);
    // A frame answering an older revision is not hit (stale until revalidated).
    assert_eq!(hit(&model, 20.0, 20.0, 4), None);
    let bounds = model.bounds("", Some(3), &["text".into(), "nope".into()]);
    assert_eq!(bounds[0].frame_id, "frame");
    assert!(model.bounds("other", Some(3), &["text".into()]).is_empty());
    assert!(model.bounds("frame", Some(4), &["text".into()]).is_empty());
    assert_eq!(bounds.len(), 1);
    assert_eq!(
        (bounds[0].x, bounds[0].y, bounds[0].kind.as_str()),
        (180.0, 70.0, "text")
    );
    assert!(bounds[0].lines >= 1);
}

#[test]
fn unchanged_content_moves_to_the_new_revision_without_recomputing() {
    let mut model = loaded(3);
    model.set_layout(vec![place("frame", 0.0)]);
    let d3 = digest(&model, 3, "frame");
    model.compute(3, "frame", 200.0, 100.0, &d3).unwrap();
    model.commit("frame");
    model
        .load_snapshot(&snapshot(4, "Hello engine").to_string())
        .unwrap();
    assert_eq!(digest(&model, 4, "frame"), d3);
    assert!(model.reuse("frame", &d3, 4));
    assert_eq!(hit(&model, 20.0, 20.0, 4), pair("frame", "card"));
    model
        .load_snapshot(&snapshot(5, "Changed").to_string())
        .unwrap();
    assert!(!model.reuse("frame", &digest(&model, 5, "frame"), 5));
}

#[test]
fn layout_releases_frames_that_are_no_longer_placed() {
    let mut model = loaded(3);
    model.set_layout(vec![place("frame", 0.0), place("other", 300.0)]);
    for id in ["frame", "other"] {
        model.compute(3, id, 200.0, 100.0, "d").unwrap();
        model.commit(id);
    }
    model.set_layout(vec![place("other", 0.0)]);
    assert!(model.frame("frame").is_none());
    assert_eq!(hit(&model, 10.0, 10.0, 3), pair("other", "other"));
}

#[test]
fn another_project_keeps_the_layout_the_page_sent_before_its_snapshot() {
    let mut model = loaded(3);
    model.set_layout(vec![place("frame", 0.0)]);
    let d3 = digest(&model, 3, "frame");
    model.compute(3, "frame", 200.0, 100.0, &d3).unwrap();
    model.commit("frame");
    // Project switch: the page places the new project's frames, then syncs its snapshot.
    // Same revision and frame ids on purpose: nothing of the previous project carries over.
    model.set_layout(vec![place("frame", 0.0), place("other", 300.0)]);
    let mut next = snapshot(3, "Hello engine");
    next["projectId"] = json!("engine-next");
    model.load_snapshot(&next.to_string()).unwrap();
    assert!(
        model.frame("frame").is_none(),
        "the previous project's scene is dropped"
    );
    assert!(!model.reuse("frame", &d3, 3));
    assert_eq!(hit(&model, 20.0, 20.0, 3), None);
    for id in ["frame", "other"] {
        model.compute(3, id, 200.0, 100.0, "d").unwrap();
        assert!(model.commit(id));
    }
    let placed: Vec<_> = model.placed().map(|f| f.id().to_owned()).collect();
    assert_eq!(placed, ["frame", "other"], "committed frames are drawn");
    assert_eq!(hit(&model, 310.0, 10.0, 3), pair("other", "other"));
}

#[test]
fn compute_needs_the_accepted_revision_and_verified_fonts() {
    let mut model = Model::new();
    assert!(model.compute(3, "frame", 200.0, 100.0, "").is_err());
    model.load_snapshot(&snapshot(3, "x").to_string()).unwrap();
    assert_eq!(
        model
            .compute(3, "frame", 200.0, 100.0, "")
            .unwrap_err()
            .code,
        "FONT_MISSING"
    );
    assert!(model.load_font("font", &"0".repeat(64), FONT).is_err());
    assert!(!model.has_font("font", HASH));
}

#[test]
fn draws_placed_frames_with_backdrop_and_culls_offscreen_ones() {
    use fagent_paint_vello::vello_common::pixmap::Pixmap;
    let mut model = loaded(3);
    model.set_layout(vec![place("frame", 0.0), place("other", 5000.0)]);
    for id in ["frame", "other"] {
        model.compute(3, id, 200.0, 100.0, "d").unwrap();
        model.commit(id);
    }
    let mut ctx = vello_cpu::RenderContext::new(240, 140);
    let view = View {
        scale: 1.0,
        tx: 20.0,
        ty: 20.0,
        width: 240.0,
        height: 140.0,
        ..View::default()
    };
    let stats = draw(&mut ctx, &mut model, &view);
    assert_eq!((stats.frames, stats.culled_frames), (1, 1));
    assert!(stats.glyphs > 0);
    assert!(!stats.analytic_shadows);
    let mut pixmap = Pixmap::new(240, 140);
    ctx.flush();
    ctx.render(&mut pixmap, &mut vello_cpu::Resources::new());
    let px = |x: u16, y: u16| pixmap.sample(x, y);
    // Card fill (#2255ff) at frame-local (20, 20); backdrop shadow below the frame.
    let card = px(40, 40);
    assert_eq!((card.r, card.g, card.b, card.a), (0x22, 0x55, 0xff, 255));
    assert!(px(120, 126).a > 0, "drop shadow under the frame");
    assert_eq!(px(2, 2).a, 0);
    let small = View { scale: 0.1, ..view };
    let stats = draw(
        &mut vello_cpu::RenderContext::new(240, 140),
        &mut model,
        &small,
    );
    assert!(stats.analytic_shadows);
    assert_eq!(stats.glyphs, 0, "1.6 px text is drawn as LOD bars");
    assert!(stats.greeked > 0);
}

fn hit(model: &Model, x: f64, y: f64, revision: u64) -> Option<(String, String)> {
    model.hit_test(x, y, revision).map(|b| (b.frame_id, b.id))
}

fn pair(frame: &str, node: &str) -> Option<(String, String)> {
    Some((frame.into(), node.into()))
}

#[test]
fn hit_results_carry_bounds_and_overlay_bounds_ignore_revision() {
    let mut model = loaded(3);
    model.set_layout(vec![place("frame", 0.0)]);
    model.compute(3, "frame", 200.0, 100.0, "d").unwrap();
    model.commit("frame");
    let hit = model.hit_test(20.0, 20.0, 3).unwrap();
    assert_eq!(
        (hit.x, hit.y, hit.width, hit.height),
        (10.0, 10.0, 120.0, 60.0)
    );
    assert_eq!(model.bounds("", None, &["card".into()]).len(), 1);
    assert!(model.bounds("", Some(4), &["card".into()]).is_empty());
}

#[test]
fn each_revision_is_resolved_once_and_never_reused_for_the_next() {
    let mut model = loaded(3);
    let first = digest(&model, 3, "frame");
    let a = model.compute(3, "frame", 200.0, 100.0, "d").unwrap();
    let again = model.compute(3, "frame", 200.0, 100.0, "d").unwrap();
    assert_eq!(a, again, "frames of one revision share its resolved nodes");
    model
        .load_snapshot(&snapshot(4, "A much longer title for the next revision").to_string())
        .unwrap();
    assert_ne!(
        digest(&model, 4, "frame"),
        first,
        "the new text is resolved"
    );
    let next = model.compute(4, "frame", 200.0, 100.0, "d").unwrap();
    assert_eq!(next.revision, 4);
    assert_ne!(next.source_scene_key, a.source_scene_key);
}

mod records;
