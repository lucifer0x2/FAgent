//! Unit tests: color/geometry helpers and small vello_cpu renders checked pixel by pixel.
use crate::color;
use crate::geometry;
use crate::{encode, EncodeOptions, EncodeStats, PaintScene, Resources, Shadows};
use serde_json::{json, Value};
use vello_common::kurbo::{Affine, Rect};
use vello_common::peniko::ImageAlphaType;
use vello_cpu::{Pixmap, RenderContext};

fn node(id: &str, parent: Option<&str>, kind: &str, b: [f32; 4], extra: Value) -> Value {
    let mut n = json!({
        "id": id, "parentId": parent, "kind": kind,
        "bounds": {"x": b[0], "y": b[1], "width": b[2], "height": b[3]},
        "lines": [], "fontId": null, "fontSize": 16, "lineHeight": 24, "assetId": null,
        "fit": "contain", "opacity": 1, "clip": false, "radius": 0, "crop": null, "fill": "transparent"
    });
    for (k, v) in extra.as_object().unwrap() {
        n[k] = v.clone();
    }
    n
}

fn scene(nodes: Vec<Value>, w: f64, h: f64) -> PaintScene {
    serde_json::from_value(
        json!({"provenance": {"viewport": {"width": w, "height": h}}, "nodes": nodes}),
    )
    .unwrap()
}

/// Straight RGBA8 pixels and encode stats.
fn render(s: &PaintScene, options: EncodeOptions) -> (Vec<u8>, u16, EncodeStats) {
    let (w, h) = s.device_size(1.0).unwrap();
    let (w, h) = (w as u16, h as u16);
    let mut ctx = RenderContext::new(w, h);
    let stats = encode(
        &mut ctx,
        s,
        &mut Resources::new(),
        Affine::IDENTITY,
        options,
    );
    ctx.flush();
    let mut pixmap = Pixmap::new(w, h);
    ctx.render(&mut pixmap, &mut vello_cpu::Resources::new());
    (pixmap.take_rgba8(ImageAlphaType::Alpha), w, stats)
}

fn px(data: &[u8], w: u16, x: usize, y: usize) -> [u8; 4] {
    let i = (y * usize::from(w) + x) * 4;
    data[i..i + 4].try_into().unwrap()
}

#[test]
fn parses_css_hex_colors_and_rejects_others() {
    let rgba = |s: &str| color::parse(s).map(|c| c.to_rgba8().to_u8_array());
    assert_eq!(rgba("#abc"), Some([0xaa, 0xbb, 0xcc, 255]));
    assert_eq!(rgba("#abcd"), Some([0xaa, 0xbb, 0xcc, 0xdd]));
    assert_eq!(rgba("#102030"), Some([0x10, 0x20, 0x30, 255]));
    assert_eq!(rgba("#10203040"), Some([0x10, 0x20, 0x30, 0x40]));
    assert_eq!(rgba("transparent"), Some([0, 0, 0, 0]));
    for bad in ["red", "#ggg", "#12345", "", "#"] {
        assert_eq!(rgba(bad), None, "{bad}");
    }
    assert!(color::solid("#ff000000", 1.0).is_none());
    assert!(color::opaque(&"#fff".into()) && !color::opaque(&"#fff8".into()));
}

#[test]
fn shadow_spread_changes_radii_like_chrome() {
    let r = [[10.0, 10.0]; 4];
    assert_eq!(color::spread_radii(r, 4.0)[0], [14.0, 14.0]);
    assert_eq!(color::spread_radii(r, -15.0)[0], [0.0, 0.0]);
    // Radius below the spread grows by the cubic falloff, never past r + s.
    let small = color::spread_radii([[2.0, 2.0]; 4], 8.0)[0][0];
    assert!(small > 2.0 && small < 10.0, "{small}");
    assert_eq!(color::spread_radii([[0.0, 0.0]; 4], 8.0)[0], [0.0, 0.0]);
}

#[test]
fn corner_radii_shrink_by_one_css_factor() {
    let s = scene(
        vec![node(
            "a",
            None,
            "rectangle",
            [0.0, 0.0, 50.0, 20.0],
            json!({"radius": 100}),
        )],
        50.0,
        20.0,
    );
    // Heights limit: 20 / (100 + 100) = 0.1 for every corner.
    assert_eq!(geometry::corners(&s.nodes[0]), [[10.0, 10.0]; 4]);
    let r = geometry::reduced(Rect::new(0.0, 0.0, 10.0, 40.0), [[10.0, 5.0]; 4]);
    assert_eq!(r, [[5.0, 2.5]; 4]);
    assert!(
        (geometry::single_radius([[2.0, 2.0], [4.0, 4.0], [2.0, 2.0], [4.0, 4.0]]) - 3.0).abs()
            < 1e-6
    );
}

#[test]
fn solid_rounded_rect_covers_its_box_but_not_the_corner() {
    let s = scene(
        vec![node(
            "a",
            None,
            "rectangle",
            [2.0, 2.0, 12.0, 12.0],
            json!({"fill": "#ff0000", "radius": 6}),
        )],
        16.0,
        16.0,
    );
    let (data, w, stats) = render(&s, EncodeOptions::default());
    assert_eq!(stats.nodes, 1);
    assert_eq!(px(&data, w, 8, 8), [255, 0, 0, 255]);
    assert_eq!(px(&data, w, 2, 2)[3], 0, "rounded corner");
    assert_eq!(px(&data, w, 15, 15)[3], 0, "outside");
}

#[test]
fn linear_gradient_runs_left_to_right_at_90_degrees() {
    let fill = json!({"type": "linear", "angle": 90, "stops": [[0, "#000000"], [1, "#ffffff"]]});
    let s = scene(
        vec![node(
            "a",
            None,
            "rectangle",
            [0.0, 0.0, 64.0, 4.0],
            json!({"fill": fill}),
        )],
        64.0,
        4.0,
    );
    let (data, w, _) = render(&s, EncodeOptions::default());
    let red: Vec<u8> = (0..64).map(|x| px(&data, w, x, 2)[0]).collect();
    assert!(red[0] < 8 && red[63] > 247, "{red:?}");
    assert!(red.windows(2).all(|p| p[0] <= p[1]), "monotonic {red:?}");
    assert!((i32::from(red[32]) - 128).abs() < 8);
}

#[test]
fn outer_shadow_falls_off_outside_the_box_with_both_techniques() {
    let shadow =
        json!([{"x": 0, "y": 0, "blur": 8, "spread": 0, "color": "#000000", "inset": false}]);
    let s = scene(
        vec![node(
            "a",
            None,
            "rectangle",
            [16.0, 16.0, 16.0, 16.0],
            json!({"fill": "#ffffff", "radius": 4, "shadow": shadow}),
        )],
        48.0,
        48.0,
    );
    for shadows in [Shadows::Filter, Shadows::Analytic] {
        let options = EncodeOptions {
            shadows,
            ..Default::default()
        };
        let (data, w, _) = render(&s, options);
        let near = px(&data, w, 14, 24)[3];
        let far = px(&data, w, 40, 24)[3];
        assert!(near > 40 && near < 200, "{shadows:?} near {near}");
        assert!(far < near && far < 30, "{shadows:?} far {far}");
        assert_eq!(
            px(&data, w, 24, 24),
            [255, 255, 255, 255],
            "{shadows:?} fill on top"
        );
        assert_eq!(px(&data, w, 0, 0)[3], 0, "{shadows:?} corner");
    }
}

#[test]
fn inset_shadow_darkens_the_inner_edge_only() {
    let shadow =
        json!([{"x": 0, "y": 0, "blur": 6, "spread": 0, "color": "#000000", "inset": true}]);
    let s = scene(
        vec![node(
            "a",
            None,
            "rectangle",
            [0.0, 0.0, 40.0, 40.0],
            json!({"fill": "#ffffff", "shadow": shadow}),
        )],
        40.0,
        40.0,
    );
    for shadows in [Shadows::Filter, Shadows::Analytic] {
        let (data, w, _) = render(
            &s,
            EncodeOptions {
                shadows,
                ..Default::default()
            },
        );
        let edge = px(&data, w, 0, 20)[0];
        let center = px(&data, w, 20, 20)[0];
        assert!(edge < 200, "{shadows:?} edge {edge}");
        assert!(center > 250, "{shadows:?} center {center}");
    }
}

#[test]
fn group_opacity_is_isolated_and_clip_limits_children() {
    let s = scene(
        vec![
            node(
                "g",
                None,
                "frame",
                [0.0, 0.0, 20.0, 10.0],
                json!({"opacity": 0.5, "clip": true}),
            ),
            node(
                "a",
                Some("g"),
                "rectangle",
                [0.0, 0.0, 12.0, 10.0],
                json!({"fill": "#ff0000"}),
            ),
            node(
                "b",
                Some("g"),
                "rectangle",
                [6.0, 0.0, 30.0, 10.0],
                json!({"fill": "#ff0000"}),
            ),
        ],
        40.0,
        10.0,
    );
    let (data, w, stats) = render(&s, EncodeOptions::default());
    assert_eq!(stats.layers, 2, "opacity + clip");
    let single = px(&data, w, 2, 5)[3];
    assert!((i32::from(single) - 128).abs() <= 1, "{single}");
    assert_eq!(px(&data, w, 8, 5)[3], single, "overlap composites once");
    assert_eq!(px(&data, w, 30, 5)[3], 0, "clipped by the parent");
}

#[test]
fn culling_skips_off_screen_subtrees_without_changing_pixels() {
    let s = scene(
        vec![
            node(
                "on",
                None,
                "rectangle",
                [0.0, 0.0, 8.0, 8.0],
                json!({"fill": "#00ff00"}),
            ),
            node(
                "off",
                None,
                "frame",
                [100.0, 0.0, 8.0, 8.0],
                json!({"fill": "#ff0000"}),
            ),
            node(
                "child",
                Some("off"),
                "rectangle",
                [100.0, 0.0, 4.0, 4.0],
                json!({"fill": "#ff0000"}),
            ),
        ],
        16.0,
        16.0,
    );
    let all = render(&s, EncodeOptions::default());
    let culled = render(
        &s,
        EncodeOptions {
            viewport: Some(Rect::new(0.0, 0.0, 16.0, 16.0)),
            ..Default::default()
        },
    );
    assert_eq!(all.0, culled.0);
    assert_eq!((all.2.nodes, culled.2.nodes, culled.2.culled), (3, 1, 1));
}

#[test]
fn missing_resources_draw_placeholders() {
    let s = scene(
        vec![
            node(
                "t",
                None,
                "text",
                [0.0, 0.0, 120.0, 20.0],
                json!({"fontId": "nope", "fill": "#000000",
                "lines": [{"text": "hi", "x": 0, "y": 14, "width": 12}]}),
            ),
            node(
                "i",
                None,
                "image",
                [0.0, 20.0, 120.0, 20.0],
                json!({"assetId": "nope"}),
            ),
        ],
        120.0,
        40.0,
    );
    let (data, w, stats) = render(
        &s,
        EncodeOptions {
            greek_below_px: 100.0,
            ..Default::default()
        },
    );
    assert_eq!(
        (stats.placeholders, stats.greeked),
        (2, 0),
        "no font: placeholder, not LOD"
    );
    assert_eq!(
        px(&data, w, 1, 21),
        [0xff, 0xe4, 0xe6, 255],
        "image placeholder background"
    );
}

#[test]
fn text_draws_glyph_outlines_or_lod_bars() {
    let manifest: fagent_layout::Font = serde_json::from_value(json!({
        "id": "noto", "family": "FAgent Noto Sans", "weight": 400, "style": "normal",
        "hash": "bfb7bb691513f12e734dc346c03a03f784912432d7e3fa8e56efcf906fe86b3d", "url": "/resources/noto.ttf"
    }))
    .unwrap();
    let mut resources = Resources::new();
    resources
        .add_font(
            &manifest,
            include_bytes!("../../../renderer/fonts/NotoSans.ttf"),
        )
        .unwrap();
    let s = scene(
        vec![node(
            "t",
            None,
            "text",
            [0.0, 0.0, 60.0, 24.0],
            json!({"fontId": "noto", "fill": "#000000",
            "lines": [{"text": "HH", "x": 0, "y": 18, "width": 24}]}),
        )],
        60.0,
        24.0,
    );
    for (greek, glyphs, greeked) in [(0.0, 2, 0), (100.0, 0, 1)] {
        let mut ctx = RenderContext::new(60, 24);
        let options = EncodeOptions {
            greek_below_px: greek,
            ..Default::default()
        };
        let stats = encode(&mut ctx, &s, &mut resources, Affine::IDENTITY, options);
        assert_eq!((stats.glyphs, stats.greeked), (glyphs, greeked));
        ctx.flush();
        let mut pixmap = Pixmap::new(60, 24);
        ctx.render(&mut pixmap, &mut vello_cpu::Resources::new());
        let data = pixmap.take_rgba8(ImageAlphaType::Alpha);
        let ink = (0..60 * 24).filter(|i| data[i * 4 + 3] > 64).count();
        assert!(ink > 20, "greek {greek}: {ink} inked pixels");
        assert_eq!(px(&data, 60, 50, 4)[3], 0, "right of the line");
    }
}

#[test]
fn zero_width_border_sides_stay_unpainted_on_fractional_edges() {
    // Bottom-only border at a half-pixel offset: the outer and inner outlines share
    // the top, left and right edges, which used to leave even-odd hairlines.
    let s = scene(
        vec![node(
            "bar",
            None,
            "frame",
            [10.5, 10.5, 40.0, 20.0],
            json!({"stroke": {"color": ["#000000", "#000000", "#000000", "#000000"],
                              "width": [0, 0, 1, 0], "align": "inside"}}),
        )],
        64.0,
        40.0,
    );
    let (data, w, _) = render(&s, EncodeOptions::default());
    for x in 10..=50 {
        assert_eq!(px(&data, w, x, 10)[3], 0, "top edge at x={x}");
    }
    for y in 10..=28 {
        assert_eq!(px(&data, w, 10, y)[3], 0, "left edge at y={y}");
        assert_eq!(px(&data, w, 50, y)[3], 0, "right edge at y={y}");
    }
    assert!(px(&data, w, 30, 29)[3] > 100, "bottom border is painted");
}
