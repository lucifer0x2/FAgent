use fagent_layout::{LayoutInput, Scene};
use fagent_scene_input::{
    check_viewport, frame_input, scene_diagnostics, Node, ResolvedNodes, SceneOptions, Snapshot,
    StorageError,
};
use fagent_text::{Font, FontSet, FontSetMeasurer, TextError};
use serde::Deserialize;
use std::collections::BTreeMap;

type Result<T> = std::result::Result<T, StorageError>;

/// Opaque transport labels supplied by the caller; never constructed or fetched here.
/// Replacing the maps prevents stale URLs leaking across snapshot/project changes.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceUrls {
    #[serde(default)]
    pub fonts: BTreeMap<String, String>,
    #[serde(default)]
    pub assets: BTreeMap<String, String>,
}

/// Caller-owned font cache; immutable verified font data is reused between computes.
#[derive(Default)]
pub struct SceneEngine {
    fonts: BTreeMap<String, Font>,
    urls: ResourceUrls,
}

impl SceneEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate original upload hash and decode before replacing any existing font.
    /// Face descriptors are bound to the exact snapshot manifest during compute.
    pub fn load_font(&mut self, id: &str, hash: &str, bytes: &[u8]) -> Result<()> {
        if id.is_empty() || id.len() > 1024 || id.chars().any(char::is_control) {
            return Err(StorageError::validation("fontId", "Invalid font identity"));
        }
        let font = Font::from_bytes(bytes, hash).map_err(text_error)?;
        self.fonts.insert(id.to_owned(), font);
        Ok(())
    }

    pub fn set_resource_urls(&mut self, urls: ResourceUrls) {
        self.urls = urls;
    }

    /// Resolve required faces before fetching bytes, so unused resources never block.
    pub fn required_fonts(&self, snapshot: &Snapshot, options: &SceneOptions) -> Result<String> {
        self.required_fonts_resolved(snapshot, &resolve(snapshot, options)?, options)
    }

    /// `required_fonts` with the snapshot already resolved for `options.theme`.
    pub fn required_fonts_resolved(
        &self,
        snapshot: &Snapshot,
        resolved: &ResolvedNodes,
        options: &SceneOptions,
    ) -> Result<String> {
        let input: LayoutInput = serde_json::from_value(input(snapshot, resolved, options)?)?;
        Ok(serde_json::to_string(&input.fonts)?)
    }

    pub fn compute(&self, snapshot: &Snapshot, options: &SceneOptions) -> Result<Scene> {
        let value = input(snapshot, &resolve(snapshot, options)?, options)?;
        self.compute_input(value)
    }

    fn compute_input(&self, value: serde_json::Value) -> Result<Scene> {
        let mut input: LayoutInput = serde_json::from_value(value)?;
        for font in &mut input.fonts {
            font.url = self.urls.fonts.get(&font.id).cloned().unwrap_or_default();
        }
        for asset in &mut input.assets {
            asset.url = self.urls.assets.get(&asset.id).cloned().unwrap_or_default();
        }
        let mut fonts = FontSet::new();
        for manifest in &input.fonts {
            let font = self.fonts.get(&manifest.id).ok_or_else(|| {
                StorageError::new("FONT_MISSING", "Exact project font bytes are unavailable")
            })?;
            if font.hash() != manifest.hash {
                return Err(StorageError::new(
                    "RESOURCE_HASH_MISMATCH",
                    "Loaded font no longer matches the requested manifest",
                ));
            }
            let font = font
                .clone()
                .with_face(&manifest.family, manifest.weight, &manifest.style)
                .map_err(text_error)?;
            fonts.insert(&manifest.id, font).map_err(text_error)?;
        }
        fagent_layout::compute_with_measurer(&input, &mut FontSetMeasurer::new(&fonts))
            .map_err(|error| StorageError::new(&error.code, &error.message))
    }

    /// Typed scene plus the advisory diagnostics `compute_json` appends (text overflow),
    /// for callers that keep the scene in memory instead of serializing it.
    pub fn compute_with_diagnostics(
        &self,
        snapshot: &Snapshot,
        options: &SceneOptions,
    ) -> Result<(Scene, Vec<serde_json::Value>)> {
        self.compute_with_diagnostics_resolved(snapshot, &resolve(snapshot, options)?, options)
    }

    /// `compute_with_diagnostics` with the snapshot already resolved for `options.theme`:
    /// resolving every node of a large project takes far longer than laying out one frame,
    /// so callers computing several frames of one revision resolve it once.
    pub fn compute_with_diagnostics_resolved(
        &self,
        snapshot: &Snapshot,
        resolved: &ResolvedNodes,
        options: &SceneOptions,
    ) -> Result<(Scene, Vec<serde_json::Value>)> {
        let input = input(snapshot, resolved, options)?;
        let intentional = if snapshot.notes.is_empty() {
            Vec::new()
        } else {
            let nodes: Vec<Node> = Deserialize::deserialize(&input["nodes"])?;
            scene_diagnostics::overflow_ids(snapshot, &nodes)
        };
        let scene = self.compute_input(input)?;
        let mut light = serde_json::json!({
            "nodes": serde_json::to_value(&scene.nodes)?,
            "provenance": serde_json::to_value(&scene.provenance)?,
            "diagnostics": serde_json::to_value(&scene.diagnostics)?,
        });
        scene_diagnostics::append(&mut light, &intentional);
        let diagnostics = match light["diagnostics"].take() {
            serde_json::Value::Array(items) => items,
            _ => Vec::new(),
        };
        Ok((scene, diagnostics))
    }

    pub fn compute_json(
        &self,
        snapshot_json: &str,
        frame_id: &str,
        width: f64,
        height: f64,
        theme: &str,
    ) -> Result<String> {
        let snapshot: Snapshot = serde_json::from_str(snapshot_json)?;
        let options = SceneOptions {
            frame_id: frame_id.into(),
            width,
            height,
            theme: theme.into(),
            state: "default".into(),
        };
        let input = input(&snapshot, &resolve(&snapshot, &options)?, &options)?;
        let intentional = if snapshot.notes.is_empty() {
            Vec::new()
        } else {
            let nodes: Vec<Node> = Deserialize::deserialize(&input["nodes"])?;
            scene_diagnostics::overflow_ids(&snapshot, &nodes)
        };
        let mut scene = serde_json::to_value(self.compute_input(input)?)?;
        scene_diagnostics::append(&mut scene, &intentional);
        Ok(serde_json::to_string(&scene)?)
    }
}

/// The viewport budget check of `make_input`, then the snapshot resolved for its theme.
fn resolve(snapshot: &Snapshot, options: &SceneOptions) -> Result<ResolvedNodes> {
    check_viewport(options.width, options.height, 1.0)?;
    ResolvedNodes::new(snapshot, &options.theme)
}

/// `make_input` from nodes already resolved for `options.theme`.
fn input(
    snapshot: &Snapshot,
    resolved: &ResolvedNodes,
    options: &SceneOptions,
) -> Result<serde_json::Value> {
    check_viewport(options.width, options.height, 1.0)?;
    frame_input(snapshot, resolved, options)
}

fn text_error(error: TextError) -> StorageError {
    StorageError::new(error.code(), "Text resources or measurement are invalid")
}
