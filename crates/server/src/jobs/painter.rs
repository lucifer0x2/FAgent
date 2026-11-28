//! The only PNG dispatch seam. Native CPU paint/encode owns the compute lane until drained.
use super::{native, png::Painted, resource_bytes::ResourceBytes};
use crate::{
    error::{ApiError, Result},
    worker::Worker,
};
use fagent_paint_core::{encode_png, PaintScene, RenderOptions};
use serde_json::{json, Value};
use std::{sync::Arc, time::Instant};

/// Provenance `engineVersion` of native PNGs: `fagent-paint-vello` export on `vello_cpu`, the
/// viewer's encoder. Validation, strict resources, readiness and PNG encoding are the shared
/// `fagent-paint-core` contract.
pub(super) const NATIVE_ENGINE_VERSION: &str = fagent_paint_vello::export::ENGINE_VERSION;

pub(super) enum Painter<'a> {
    Chromium(&'a mut Worker),
    Native {
        bytes: Arc<ResourceBytes>,
        deadline: Instant,
        readiness: Vec<Value>,
    },
}

impl Painter<'_> {
    /// Paints `scene` at `scale`. A surface over the pixel budget (including a hug root that
    /// grew past the requested viewport) fails before dispatch with the limit, the requested
    /// pixels and the largest scale that fits, the same on every backend.
    pub async fn paint(&mut self, scene: &Value, scale: f64) -> Result<Painted> {
        let surface = super::png::surface(scene);
        if let Some((width, height)) = surface {
            super::png::check_pixels(width, height, scale)?;
        }
        self.paint_backend(scene, scale)
            .await
            .map_err(|error| super::png::explain(error, surface, scale))
    }

    async fn paint_backend(&mut self, scene: &Value, scale: f64) -> Result<Painted> {
        match self {
            Self::Chromium(worker) => {
                let value = worker
                    .call(json!({"op":"render","scene":scene,"scale":scale,"strict":true}))
                    .await?;
                let bytes = super::png::decode(&value)?;
                Ok(Painted { value, bytes })
            }
            Self::Native {
                bytes,
                deadline,
                readiness,
            } => {
                let (bytes, scene) = (bytes.clone(), scene.clone());
                let (result, observed) = native::blocking(*deadline, move |control| {
                    let scene: PaintScene = serde_json::from_value(scene)?;
                    let cancelled = || control.check().is_err();
                    let options = RenderOptions {
                        scale,
                        cancelled: Some(&cancelled),
                        ..Default::default()
                    };
                    let painted = match fagent_paint_vello::export::render(&scene, &bytes.paint(), &options) {
                        Ok(painted) => painted,
                        Err(error) => {
                            control.check()?; // CANCELLED distinguishes timeout from caller cancellation.
                            return Ok((
                                Err(ApiError::new(&error.code, &error.message)),
                                error.readiness,
                            ));
                        }
                    };
                    control.check()?;
                    let png =
                        encode_png(&painted).map_err(|e| ApiError::new(&e.code, &e.message))?;
                    control.check()?;
                    let mut provenance = serde_json::to_value(&scene)?["provenance"].clone();
                    provenance["scale"] = json!(scale);
                    let value = json!({"status":"ready","width":painted.width,"height":painted.height,
                        "draw":painted.draw,"provenance":provenance,
                        "readiness":painted.readiness,"diagnostics":painted.diagnostics});
                    control.check()?;
                    Ok((Ok(Painted { value, bytes: png }), painted.readiness))
                })
                .await?;
                // Missing upstream resources carry a more specific storage observation.
                merge(readiness, observed)?;
                result
            }
        }
    }

    /// SVG readiness: original resource integrity only, independent of the raster backend.
    pub async fn inspect_resources(&mut self, scene: &Value) -> Result<()> {
        let Self::Native {
            bytes,
            deadline,
            readiness,
            ..
        } = self
        else {
            return Ok(());
        };
        let (bytes, scene) = (bytes.clone(), scene.clone());
        let (result, observed) = native::blocking(*deadline, move |control| {
            let scene: PaintScene = serde_json::from_value(scene)?;
            let cancelled = || control.check().is_err();
            let options = RenderOptions {
                cancelled: Some(&cancelled),
                ..Default::default()
            };
            match fagent_paint_core::inspect_svg_resources(&scene, &bytes.paint(), &options) {
                Ok(observed) => Ok((Ok(()), observed)),
                Err(error) => {
                    control.check()?;
                    Ok((
                        Err(ApiError::new(&error.code, &error.message)),
                        error.readiness,
                    ))
                }
            }
        })
        .await?;
        merge(readiness, observed)?;
        result
    }

    pub fn readiness(&self) -> Option<&Vec<Value>> {
        match self {
            Self::Native { readiness, .. } => Some(readiness),
            _ => None,
        }
    }
}

fn merge(
    readiness: &mut Vec<Value>,
    observed: Vec<fagent_paint_core::ResourceReadiness>,
) -> Result<()> {
    for observation in observed {
        let value = serde_json::to_value(observation)?;
        if let Some(old) = readiness
            .iter_mut()
            .find(|r| r["id"] == value["id"] && r["kind"] == value["kind"])
        {
            if old["status"] != "failed" {
                *old = value;
            }
        } else {
            readiness.push(value);
        }
    }
    Ok(())
}
