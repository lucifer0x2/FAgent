//! Renders a scene with vello_cpu to a straight-alpha PNG. Speaks the adapter
//! protocol of `spikes/native-paint-parity`: `OUT_ROOT REQUEST_JSON PNG_OUTPUT`,
//! where the request is `{scene, scale, shadows?, size?, translate?, cull?,
//! repeat?, greekBelowPx?}`
//! (see `run`) and resources are flat
//! `/resources/<file>` URLs under `OUT_ROOT/resources`. Prints one JSON line.
use fagent_paint_core::{decode_image, RenderOptions};
use fagent_paint_vello::vello_common::kurbo::{Affine, Rect};
use fagent_paint_vello::vello_common::peniko::ImageAlphaType;
use fagent_paint_vello::{encode, pixmap_source, EncodeOptions, PaintScene, Resources, Shadows};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::{env, error::Error, fs};
use vello_cpu::{Pixmap, RenderContext};

fn resource(root: &Path, url: &str) -> Result<Option<PathBuf>, Box<dyn Error>> {
    let name = url
        .strip_prefix("/resources/")
        .ok_or("fixture URL required")?;
    if name.is_empty() || name.contains(['/', '\\']) || name == ".." {
        return Err("invalid fixture resource name".into());
    }
    let path = root.join("resources").join(name);
    Ok(path.exists().then_some(path))
}

fn run() -> Result<Value, Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        return Err("usage: paint-vello-png OUT_ROOT REQUEST_JSON PNG_OUTPUT".into());
    }
    let root = fs::canonicalize(&args[1])?;
    let request: Value = serde_json::from_slice(&fs::read(&args[2])?)?;
    let scene: PaintScene = serde_json::from_value(request["scene"].clone())?;
    let scale = request["scale"].as_f64().ok_or("scale required")?;
    // Optional camera: `size: [w, h]` device px and `translate: [tx, ty]` (default: whole
    // viewport × scale). `cull` (default true) skips off-screen subtrees; `repeat` times
    // that many encode+render passes and reports each.
    let pair = |key: &str| -> Option<[f64; 2]> {
        let v = request[key].as_array()?;
        Some([v.first()?.as_f64()?, v.get(1)?.as_f64()?])
    };
    let (width, height) = match pair("size") {
        Some([w, h]) if w >= 1.0 && h >= 1.0 && w <= 65535.0 && h <= 65535.0 => {
            (w as u32, h as u32)
        }
        Some(_) => return Err("invalid size".into()),
        None => scene.device_size(scale).ok_or("invalid viewport")?,
    };
    let [tx, ty] = pair("translate").unwrap_or([0.0, 0.0]);
    let repeat = request["repeat"].as_u64().unwrap_or(1).clamp(1, 1000);

    let mut resources = Resources::new();
    let mut missing = Vec::new();
    for f in &scene.fonts {
        match resource(&root, &f.url)? {
            Some(path) => resources.add_font(f, &fs::read(path)?)?,
            None => missing.push(f.id.clone()),
        }
    }
    for a in &scene.assets {
        let decoded = resource(&root, &a.url)?
            .ok_or("missing")
            .map_err(String::from)
            .and_then(|p| fs::read(p).map_err(|e| e.to_string()))
            .and_then(|bytes| {
                decode_image(&bytes, &a.media_type, &RenderOptions::default())
                    .map_err(|e| e.to_string())
            });
        match decoded.and_then(|i| pixmap_source(i.rgba, i.width, i.height)) {
            Ok(image) => resources.add_image(a.id.clone(), image),
            Err(_) => missing.push(a.id.clone()),
        }
    }
    if !missing.is_empty() {
        return Ok(json!({"ok": false, "code": "RESOURCE_NOT_READY", "missing": missing}));
    }

    let (w, h) = (u16::try_from(width)?, u16::try_from(height)?);
    let options = EncodeOptions {
        shadows: match request["shadows"].as_str() {
            Some("analytic") => Shadows::Analytic,
            _ => Shadows::Filter,
        },
        viewport: (request["cull"].as_bool() != Some(false))
            .then(|| Rect::new(0.0, 0.0, f64::from(w), f64::from(h))),
        greek_below_px: request["greekBelowPx"].as_f64().unwrap_or(0.0),
    };
    let camera = Affine::translate((tx, ty)) * Affine::scale(scale);
    let mut pixmap = Pixmap::new(w, h);
    let mut cpu = vello_cpu::Resources::new();
    let (mut stats, mut runs) = (Default::default(), Vec::new());
    for _ in 0..repeat {
        let start = Instant::now();
        let mut ctx = RenderContext::new(w, h);
        stats = encode(&mut ctx, &scene, &mut resources, camera, options);
        let encoded = start.elapsed();
        ctx.flush();
        ctx.render(&mut pixmap, &mut cpu);
        runs.push(json!({"encode": encoded.as_secs_f64() * 1e3,
            "total": start.elapsed().as_secs_f64() * 1e3}));
    }
    let stats: fagent_paint_vello::EncodeStats = stats;

    let output = Path::new(&args[3]);
    let mut writer = png::Encoder::new(fs::File::create(output)?, width, height);
    writer.set_color(png::ColorType::Rgba);
    writer.set_depth(png::BitDepth::Eight);
    writer
        .write_header()?
        .write_image_data(&pixmap.take_rgba8(ImageAlphaType::Alpha))?;
    Ok(json!({
        "ok": true, "width": width, "height": height,
        "stats": {"nodes": stats.nodes, "glyphs": stats.glyphs, "layers": stats.layers,
                  "placeholders": stats.placeholders, "culled": stats.culled,
                  "greeked": stats.greeked},
        "ms": runs[0], "runs": runs,
    }))
}

fn main() {
    let value = run().unwrap_or_else(
        |e| json!({"ok": false, "code": "HARNESS_ERROR", "message": e.to_string()}),
    );
    println!("{value}");
}
