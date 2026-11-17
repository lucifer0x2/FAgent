//! One computed frame: the paint scene, its prepared tree for the encoder, a node index
//! for hit testing/bounds, its board placement and the small summary the page receives.
use fagent_layout::{Scene, SceneNode};
use fagent_paint_vello::{PaintScene, Prepared, Provenance, Viewport};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Board position (scene px) and the box the frame backdrop covers.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Placement {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssetRef {
    pub id: String,
    pub hash: String,
    pub media_type: String,
}

/// Everything the page needs about a computed frame (status, inspector source, notices);
/// the scene itself stays in the engine.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub project_id: String,
    pub frame_id: String,
    pub revision: u64,
    pub width: f64,
    pub height: f64,
    pub node_count: usize,
    pub scene_version: String,
    pub renderer_version: String,
    pub coordinate_space: String,
    pub source_scene_key: String,
    pub diagnostics: Vec<serde_json::Value>,
    pub fonts: Vec<String>,
    pub assets: Vec<AssetRef>,
    /// Union of node bounds `[x, y, width, height]` (frame-local): nothing outside it can
    /// be hit, so the page can treat such clicks as background without a round trip.
    pub hit_area: Option<[f32; 4]>,
}

/// Frame-local computed geometry of one node (Inspector, selection overlay).
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NodeBounds {
    pub frame_id: String,
    pub id: String,
    pub kind: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub lines: usize,
}

pub struct Frame {
    pub scene: PaintScene,
    pub prepared: Prepared,
    index: HashMap<String, usize>,
    pub placement: Placement,
    pub summary: Summary,
    pub revision: u64,
    /// Revision-free content digest; equal digests let a new revision reuse the frame.
    pub digest: String,
}

impl Frame {
    pub fn new(scene: Scene, diagnostics: Vec<serde_json::Value>, digest: String) -> Self {
        let p = &scene.provenance;
        let summary = Summary {
            project_id: p.project_id.clone(),
            frame_id: p.frame_id.clone(),
            revision: p.revision,
            width: p.viewport.width,
            height: p.viewport.height,
            node_count: scene.nodes.len(),
            scene_version: p.scene_version.clone(),
            renderer_version: p.renderer_version.clone(),
            coordinate_space: scene.coordinate_space.clone(),
            source_scene_key: p.source_scene_key.clone(),
            diagnostics,
            fonts: scene.fonts.iter().map(|f| f.id.clone()).collect(),
            assets: scene
                .assets
                .iter()
                .map(|a| AssetRef {
                    id: a.id.clone(),
                    hash: a.hash.clone(),
                    media_type: a.media_type.clone(),
                })
                .collect(),
            hit_area: scene
                .nodes
                .iter()
                .map(|n| {
                    let b = &n.bounds;
                    [b.x, b.y, b.x + b.width, b.y + b.height]
                })
                .reduce(|a, b| {
                    [
                        a[0].min(b[0]),
                        a[1].min(b[1]),
                        a[2].max(b[2]),
                        a[3].max(b[3]),
                    ]
                })
                .map(|[x0, y0, x1, y1]| [x0, y0, x1 - x0, y1 - y0]),
        };
        let paint = PaintScene {
            provenance: Some(Provenance {
                viewport: Viewport {
                    width: p.viewport.width,
                    height: p.viewport.height,
                },
            }),
            nodes: scene.nodes,
            fonts: scene.fonts,
            assets: scene.assets,
        };
        let index = paint
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.clone(), i))
            .collect();
        Self {
            prepared: Prepared::new(&paint),
            placement: Placement {
                id: summary.frame_id.clone(),
                width: summary.width,
                height: summary.height,
                ..Placement::default()
            },
            revision: summary.revision,
            summary,
            scene: paint,
            index,
            digest,
        }
    }

    pub fn id(&self) -> &str {
        &self.summary.frame_id
    }

    fn node(&self, id: &str) -> Option<&SceneNode> {
        self.index.get(id).map(|&i| &self.scene.nodes[i])
    }

    /// Topmost node containing frame-local (`x`, `y`) whose clipping ancestors also
    /// contain it; the same rule as `web/src/canvasHitTest.ts` (edges inclusive).
    pub fn hit_test(&self, x: f64, y: f64) -> Option<&str> {
        let inside = |n: &SceneNode| {
            let b = &n.bounds;
            let (x0, y0) = (f64::from(b.x), f64::from(b.y));
            x >= x0 && y >= y0 && x <= x0 + f64::from(b.width) && y <= y0 + f64::from(b.height)
        };
        let limit = self.scene.nodes.len();
        self.scene
            .nodes
            .iter()
            .rev()
            .find(|n| {
                if !inside(n) {
                    return false;
                }
                let mut parent = n.parent_id.as_deref().and_then(|id| self.node(id));
                let mut guard = 0;
                while let Some(p) = parent {
                    if guard > limit {
                        break;
                    }
                    guard += 1;
                    if p.clip && !inside(p) {
                        return false;
                    }
                    parent = p.parent_id.as_deref().and_then(|id| self.node(id));
                }
                true
            })
            .map(|n| n.id.as_str())
    }

    pub fn bounds(&self, ids: &[String]) -> Vec<NodeBounds> {
        ids.iter()
            .filter_map(|id| self.node(id))
            .map(|n| NodeBounds {
                frame_id: self.summary.frame_id.clone(),
                id: n.id.clone(),
                kind: n.kind.clone(),
                x: n.bounds.x,
                y: n.bounds.y,
                width: n.bounds.width,
                height: n.bounds.height,
                lines: n.lines.len(),
            })
            .collect()
    }
}
