//! Engine state that does not depend on a GPU: the accepted snapshot (parsed once per
//! revision), verified font bytes, computed frame scenes (never serialized back to the
//! page) and their board placement. Hit testing and bounds read the same scenes.
use crate::frame::{Frame, NodeBounds, Placement, Summary};
use fagent_paint_vello::{Image, Resources};
use fagent_scene_wasm::{ResolvedNodes, SceneEngine, SceneOptions, Snapshot, StorageError};
use std::cell::OnceCell;
use std::collections::HashMap;

pub type Result<T> = std::result::Result<T, StorageError>;

const THEME: &str = "default";

struct Accepted {
    project_id: String,
    revision: u64,
    snapshot: Snapshot,
    /// Every node resolved for `THEME`, once per revision: resolving a large project takes
    /// far longer than laying out one of its frames (~0.4 s against ~25 ms natively).
    resolved: OnceCell<ResolvedNodes>,
}

impl Accepted {
    fn resolved(&self) -> Result<&ResolvedNodes> {
        if let Some(resolved) = self.resolved.get() {
            return Ok(resolved);
        }
        let resolved = ResolvedNodes::new(&self.snapshot, THEME)?;
        Ok(self.resolved.get_or_init(|| resolved))
    }
}

#[derive(Default)]
pub struct Model {
    scenes: SceneEngine,
    accepted: Option<Accepted>,
    /// Verified bytes by font id, bound to a paint face once a scene names its manifest.
    font_bytes: HashMap<String, (String, Vec<u8>)>,
    /// Manifest id → content hash currently bound in `paint`.
    painted_fonts: HashMap<String, String>,
    /// Fonts and images the encoder draws with; image sources belong to the live backend.
    pub paint: Resources,
    /// Computed but not yet shown: resources (images) may still be loading.
    staged: HashMap<String, Frame>,
    frames: HashMap<String, Frame>,
    layout: Vec<Placement>,
}

impl Model {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses `json` unless this project revision is already loaded. Another project
    /// drops every frame and staged scene of the previous one (frame ids, digests and
    /// revision numbers mean nothing across projects).
    ///
    /// The board layout is kept: it is the page's, sent before this sync (the page places
    /// the new project's skeletons before it asks for their scenes) and never resent while
    /// unchanged. Clearing it here left every frame of the new project computed but
    /// unplaced, so nothing was drawn once the skeletons were gone.
    pub fn load_snapshot(&mut self, json: &str) -> Result<()> {
        let snapshot: Snapshot = serde_json::from_str(json)?;
        if self
            .accepted
            .as_ref()
            .is_some_and(|a| a.project_id != snapshot.project_id)
        {
            self.frames.clear();
            self.staged.clear();
        }
        self.accepted = Some(Accepted {
            project_id: snapshot.project_id.clone(),
            revision: snapshot.revision,
            snapshot,
            resolved: OnceCell::new(),
        });
        Ok(())
    }

    pub fn snapshot_revision(&self) -> Option<u64> {
        self.accepted.as_ref().map(|a| a.revision)
    }

    fn accepted(&self, revision: u64) -> Result<&Accepted> {
        self.accepted
            .as_ref()
            .filter(|a| a.revision == revision)
            .ok_or_else(|| {
                StorageError::new(
                    "SNAPSHOT_MISSING",
                    "The engine has no accepted snapshot for this revision",
                )
            })
    }

    fn options(frame_id: &str, width: f64, height: f64) -> SceneOptions {
        SceneOptions {
            frame_id: frame_id.into(),
            width,
            height,
            theme: THEME.into(),
            state: "default".into(),
        }
    }

    /// `[{frameId, digest} | {frameId, error}]` for `[{frameId, width, height}]`.
    pub fn frame_keys(&self, revision: u64, frames_json: &str) -> Result<String> {
        let accepted = self.accepted(revision)?;
        let frames: Vec<fagent_scene_wasm::FrameKeyRequest> = serde_json::from_str(frames_json)?;
        let keys = fagent_scene_wasm::frame_keys_resolved(
            &accepted.snapshot,
            accepted.resolved()?,
            &frames,
            THEME,
        )?;
        Ok(serde_json::to_string(&keys)?)
    }

    /// Font manifests a frame needs before it can be measured (JSON array).
    pub fn required_fonts(
        &self,
        revision: u64,
        frame_id: &str,
        width: f64,
        height: f64,
    ) -> Result<String> {
        let accepted = self.accepted(revision)?;
        self.scenes.required_fonts_resolved(
            &accepted.snapshot,
            accepted.resolved()?,
            &Self::options(frame_id, width, height),
        )
    }

    /// Hash-checked by the scene engine before anything is replaced.
    pub fn load_font(&mut self, id: &str, hash: &str, bytes: &[u8]) -> Result<()> {
        if self.font_bytes.get(id).is_some_and(|(h, _)| h == hash) {
            return Ok(());
        }
        self.scenes.load_font(id, hash, bytes)?;
        self.font_bytes
            .insert(id.to_owned(), (hash.to_owned(), bytes.to_vec()));
        Ok(())
    }

    pub fn has_font(&self, id: &str, hash: &str) -> bool {
        self.font_bytes.get(id).is_some_and(|(h, _)| h == hash)
    }

    /// Computes a frame at `revision` into the staging area and returns its summary.
    pub fn compute(
        &mut self,
        revision: u64,
        frame_id: &str,
        width: f64,
        height: f64,
        digest: &str,
    ) -> Result<Summary> {
        let accepted = self.accepted(revision)?;
        let (scene, diagnostics) = self.scenes.compute_with_diagnostics_resolved(
            &accepted.snapshot,
            accepted.resolved()?,
            &Self::options(frame_id, width, height),
        )?;
        let p = &scene.provenance;
        if p.project_id != accepted.project_id || p.revision != revision || p.frame_id != frame_id {
            return Err(StorageError::new(
                "SCENE_IDENTITY",
                "Computed scene does not answer this request",
            ));
        }
        for manifest in &scene.fonts {
            self.bind_paint_font(manifest)?;
        }
        let frame = Frame::new(scene, diagnostics, digest.to_owned());
        let summary = frame.summary.clone();
        self.staged.insert(frame_id.to_owned(), frame);
        Ok(summary)
    }

    /// Stages an already computed scene-3 document (parity and benchmark harnesses that
    /// compare painters on fixed scenes); fonts must be loaded first.
    pub fn insert_scene(&mut self, scene_json: &str) -> Result<Summary> {
        let scene: fagent_layout::Scene = serde_json::from_str(scene_json)?;
        for manifest in &scene.fonts {
            self.bind_paint_font(manifest)?;
        }
        let diagnostics = scene
            .diagnostics
            .iter()
            .map(|d| serde_json::Value::String(d.clone()))
            .collect();
        let frame = Frame::new(scene, diagnostics, String::new());
        let summary = frame.summary.clone();
        self.staged.insert(summary.frame_id.clone(), frame);
        Ok(summary)
    }

    fn bind_paint_font(&mut self, manifest: &fagent_layout::Font) -> Result<()> {
        if self.painted_fonts.get(&manifest.id) == Some(&manifest.hash) {
            return Ok(());
        }
        let Some((hash, bytes)) = self.font_bytes.get(&manifest.id) else {
            return Ok(());
        };
        if *hash != manifest.hash {
            return Ok(());
        }
        self.paint
            .add_font(manifest, bytes)
            .map_err(|message| StorageError::new("FONT_INVALID", &message))?;
        self.painted_fonts
            .insert(manifest.id.clone(), manifest.hash.clone());
        Ok(())
    }

    /// Image ids the staged frame `frame_id` draws, with their content hashes.
    pub fn staged_assets(&self, frame_id: &str) -> Vec<(String, String)> {
        self.staged.get(frame_id).map_or_else(Vec::new, |f| {
            f.scene
                .assets
                .iter()
                .map(|a| (a.id.clone(), a.hash.clone()))
                .collect()
        })
    }

    /// Shows a staged frame in place of the frame's previous scene.
    pub fn commit(&mut self, frame_id: &str) -> bool {
        let Some(mut frame) = self.staged.remove(frame_id) else {
            return false;
        };
        if let Some(p) = self.layout.iter().find(|p| p.id == frame_id) {
            frame.placement = p.clone();
        }
        self.frames.insert(frame_id.to_owned(), frame);
        true
    }

    pub fn discard(&mut self, frame_id: &str) {
        self.staged.remove(frame_id);
    }

    /// A frame whose render input is unchanged answers `revision` without recomputing.
    pub fn reuse(&mut self, frame_id: &str, digest: &str, revision: u64) -> bool {
        match self.frames.get_mut(frame_id) {
            Some(f) if !digest.is_empty() && f.digest == digest => {
                f.revision = revision;
                f.summary.revision = revision;
                true
            }
            _ => false,
        }
    }

    /// Board order and placement; frames that are not placed are released.
    pub fn set_layout(&mut self, layout: Vec<Placement>) {
        self.frames
            .retain(|id, _| layout.iter().any(|p| &p.id == id));
        self.staged
            .retain(|id, _| layout.iter().any(|p| &p.id == id));
        for p in &layout {
            if let Some(f) = self.frames.get_mut(&p.id) {
                f.placement = p.clone();
            }
        }
        self.layout = layout;
    }

    /// Frames in board order with their placement (drawing order).
    pub fn placed(&self) -> impl Iterator<Item = &Frame> {
        self.layout.iter().filter_map(|p| self.frames.get(&p.id))
    }

    /// Placed frames in drawing order, with the paint resources the encoder mutates.
    pub(crate) fn frames_and_paint(&mut self) -> (Vec<&Frame>, &mut Resources) {
        let frames = self
            .layout
            .iter()
            .filter_map(|p| self.frames.get(&p.id))
            .collect();
        (frames, &mut self.paint)
    }

    /// Bounds of `ids` in `frame_id` (every placed frame when empty); with `revision`,
    /// only from frames current at it (Inspector), otherwise from whatever is shown.
    pub fn bounds(&self, frame_id: &str, revision: Option<u64>, ids: &[String]) -> Vec<NodeBounds> {
        self.placed()
            .filter(|f| frame_id.is_empty() || f.id() == frame_id)
            .filter(|f| revision.is_none_or(|r| f.revision == r))
            .flat_map(|f| f.bounds(ids))
            .collect()
    }

    pub fn frame(&self, frame_id: &str) -> Option<&Frame> {
        self.frames.get(frame_id)
    }

    /// Topmost node under board point (`x`, `y`), searching only frames current at
    /// `revision` (later frames first), as the Canvas2D board hit test does.
    pub fn hit_test(&self, x: f64, y: f64, revision: u64) -> Option<NodeBounds> {
        self.layout.iter().rev().find_map(|p| {
            let f = self.frames.get(&p.id).filter(|f| f.revision == revision)?;
            let node = f.hit_test(x - f.placement.x, y - f.placement.y)?;
            f.bounds(&[node.to_owned()]).pop()
        })
    }

    /// Records the decoded image (premultiplied RGBA8) the painter reads by asset id.
    pub fn add_image(&mut self, id: &str, image: Image) {
        self.paint.add_image(id, image);
    }

    pub fn has_image(&self, id: &str) -> bool {
        self.paint.has_image(id)
    }
}

#[cfg(test)]
mod tests;
