use crate::asset_import::{self, Fields, Prepared};
use crate::*;
impl Project {
    pub(crate) fn asset_path(&self, key: &str) -> Result<PathBuf> {
        if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(StorageError::new(
                "PATH_REJECTED",
                "Invalid asset storage key",
            ));
        }
        let assets = guard(&self.path.join("assets"), &self.path)?;
        let path = guard(&assets.join(key), &assets)?;
        if !path.starts_with(&self.path) || !path.is_file() {
            return Err(StorageError::new("PATH_REJECTED", "Invalid asset location"));
        }
        Ok(path)
    }
    pub fn read_asset(&self, id: &str) -> Result<(Asset, Vec<u8>)> {
        self.check_paths()?;
        let conn = self.reader()?;
        let data: Option<String> = conn
            .query_row("SELECT data_json FROM assets WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        let asset: Asset = serde_json::from_str(
            &data.ok_or_else(|| StorageError::new("ASSET_NOT_FOUND", "Asset does not exist"))?,
        )?;
        let path = self
            .asset_path(&asset.relative_key)
            .map_err(|_| StorageError::new("ASSET_MISSING", "Asset binary is unavailable"))?;
        let metadata = fs::metadata(&path)?;
        if metadata.len() > MAX_ASSET_BYTES as u64 {
            return Err(StorageError::new(
                "ASSET_INVALID",
                "Asset exceeds the supported size",
            ));
        }
        let bytes = fs::read(path)?;
        if hash(&bytes) != asset.content_hash || bytes.len() as u64 != asset.byte_size {
            return Err(StorageError::new(
                "ASSET_INVALID",
                "Asset content does not match its manifest hash",
            ));
        }
        let mut asset = [asset];
        image_size::fill_missing(&mut asset, |_| Some(bytes.clone()));
        let [asset] = asset;
        Ok((asset, bytes))
    }
    /// Sizes of ready image assets stored without one, read from their files: assets
    /// imported before JPEG and WebP sizes were recorded. Nothing is written back.
    pub(crate) fn fill_image_sizes(&self, s: &mut Snapshot) {
        image_size::fill_missing(&mut s.assets, |asset| {
            let path = self.asset_path(&asset.relative_key).ok()?;
            let small = fs::metadata(&path).ok()?.len() <= MAX_ASSET_BYTES as u64;
            small.then(|| fs::read(path).ok()).flatten()
        });
    }
    pub fn import_asset(&self, request: ImportAssetRequest) -> Result<ImportAssetResult> {
        let mut imported = self.import_items(request.into(), Fields::Single)?;
        Ok(ImportAssetResult {
            asset: imported.assets.remove(0),
            result: imported.result,
        })
    }
    /// Imports several assets atomically as one revision; item errors name `items[i]`.
    pub fn import_assets(&self, request: ImportAssetsRequest) -> Result<ImportAssetsResult> {
        self.import_items(request, Fields::Indexed)
    }
    fn import_items(
        &self,
        request: ImportAssetsRequest,
        fields: Fields,
    ) -> Result<ImportAssetsResult> {
        self.check_paths()?;
        self.request_ids(&request.project_id, &request.idempotency_key)?;
        let Prepared {
            assets,
            canonical,
            request_hash,
        } = asset_import::prepare(&request, fields)?;
        let mut conn = self.writer.lock().map_err(|_| poisoned())?;
        if let Some(mut result) = replay(&conn, &request.idempotency_key, &request_hash)? {
            result.replayed = true;
            let mut stored = Vec::with_capacity(assets.len());
            for asset in &assets {
                let data: String = conn.query_row(
                    "SELECT data_json FROM assets WHERE id=?1",
                    [&asset.id],
                    |r| r.get(0),
                )?;
                stored.push(serde_json::from_str(&data)?);
            }
            return Ok(ImportAssetsResult {
                assets: stored,
                result,
            });
        }
        let before = read_snapshot(&mut conn)?;
        if before.revision != request.expected_revision {
            return Err(StorageError::conflict(before.revision));
        }
        let mut candidate = Candidate::from_snapshot(&before);
        let changed = asset_import::apply(&mut candidate, &request, &assets, fields)?;
        candidate.validate()?;
        // Immutable content is durable before the DB can refer to it. A crash here
        // leaves only orphans, which are deliberately not garbage collected.
        for (asset, bytes) in asset_import::unique_binaries(&request, &assets) {
            self.publish_binary(&asset.content_hash, bytes)?;
        }
        failpoint("after_asset_publish");
        let result = BatchResult {
            project_id: self.project_id.clone(),
            committed_revision: before.revision + 1,
            changed_ids: changed,
            render_status: "pending".into(),
            ..Default::default()
        };
        let history = scoped::envelope(
            &before,
            &candidate,
            canonical,
            asset_import::history(&request, &assets),
        )?;
        self.persist(
            &mut conn,
            &before,
            &candidate,
            &request.idempotency_key,
            &request_hash,
            &history,
            &result,
            &mut BatchMetrics::default(),
        )?;
        self.root.memory.evict(&self.project_id);
        Ok(ImportAssetsResult { assets, result })
    }
    /// Writes one content-addressed binary, or checks the one already there.
    fn publish_binary(&self, content_hash: &str, bytes: &[u8]) -> Result<()> {
        let assets = guard(&self.path.join("assets"), &self.path)?;
        let dest = assets.join(content_hash);
        no_symlink(&dest)?;
        if dest.exists() {
            if hash(&fs::read(self.asset_path(content_hash)?)?) != content_hash {
                return Err(StorageError::new(
                    "ASSET_INVALID",
                    "Existing asset content has a different hash",
                ));
            }
            return Ok(());
        }
        let temp = assets.join(format!(".import-{}", uuid::Uuid::new_v4().simple()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, &dest)?;
        sync_dir(&assets)?;
        Ok(())
    }
    /// Content-addressed binaries without committed DB refs. Read-only inventory;
    /// no GC is run because render/export handles may still own a binary.
    pub fn orphan_asset_keys(&self) -> Result<Vec<String>> {
        let snapshot = self.snapshot()?;
        let mut referenced: BTreeSet<_> = snapshot
            .assets
            .into_iter()
            .map(|a| a.relative_key)
            .collect();
        // Bundle restore also materializes the pinned fallback font. Its
        // committed bundled-font reference owns that copy without an asset row.
        referenced.extend(snapshot.fonts.into_iter().filter_map(|font| {
            (font.bundled_id.as_deref() == Some("font_bundled"))
                .then_some(font.content_hash)
                .flatten()
        }));
        let mut orphan = Vec::new();
        for entry in fs::read_dir(guard(&self.path.join("assets"), &self.path)?)? {
            let entry = entry?;
            let key = entry.file_name().to_string_lossy().to_string();
            if key.len() == 64
                && key.bytes().all(|b| b.is_ascii_hexdigit())
                && !referenced.contains(&key)
            {
                self.asset_path(&key)?;
                orphan.push(key);
            }
        }
        orphan.sort();
        Ok(orphan)
    }
}
/// The size an import records: required for PNG; read from the JPEG or WebP header when it
/// gives one (a header without a size stays accepted, sizeless); none for fonts. Images
/// over `MAX_IMAGE_PIXELS` are rejected.
pub(crate) fn asset_dimensions(mime: &str, bytes: &[u8]) -> Result<(Option<u32>, Option<u32>)> {
    if !signature_matches(mime, bytes) {
        return Err(StorageError::validation(
            "mediaType",
            "Supported asset types are PNG, JPEG, WebP, TTF, OTF and WOFF2 with matching signatures",
        ));
    }
    let size = image_size::image_size(mime, bytes);
    let over_budget = || {
        StorageError::validation(
            "bytes",
            &format!(
                "Image dimensions exceed the supported pixel budget of {} pixels",
                image_size::MAX_IMAGE_PIXELS
            ),
        )
    };
    match (mime, size) {
        ("image/png", None) => Err(over_budget()),
        (_, Some((w, h))) if u64::from(w) * u64::from(h) > image_size::MAX_IMAGE_PIXELS => {
            Err(over_budget())
        }
        (_, Some((w, h))) => Ok((Some(w), Some(h))),
        (_, None) => Ok((None, None)),
    }
}

/// Whether a stored asset's size agrees with its binary. JPEG and WebP assets imported
/// before their sizes were recorded keep a null size and stay valid.
pub(crate) fn stored_dimensions_match(
    mime: &str,
    bytes: &[u8],
    stored: (Option<u32>, Option<u32>),
) -> Result<bool> {
    if matches!(mime, "image/jpeg" | "image/webp") && stored == (None, None) {
        if !signature_matches(mime, bytes) {
            // The same signature error an import gives.
            asset_dimensions(mime, bytes)?;
        }
        return Ok(true);
    }
    Ok(asset_dimensions(mime, bytes)? == stored)
}

fn signature_matches(mime: &str, bytes: &[u8]) -> bool {
    match mime {
        "image/png" => {
            bytes.len() >= 33
                && bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                && &bytes[12..16] == b"IHDR"
        }
        "image/jpeg" => bytes.starts_with(b"\xff\xd8\xff"),
        "image/webp" => bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP",
        "font/ttf" => {
            bytes.len() > 12 && (bytes.starts_with(&[0, 1, 0, 0]) || bytes.starts_with(b"true"))
        }
        "font/otf" => bytes.len() > 12 && bytes.starts_with(b"OTTO"),
        "font/woff2" => bytes.len() > 48 && bytes.starts_with(b"wOF2"),
        _ => false,
    }
}

pub(crate) fn register_font(
    candidate: &mut Candidate,
    media_type: &str,
    font: Option<&FontRegistration>,
    asset: &Asset,
    changed: &mut Vec<String>,
) -> Result<()> {
    if let Some(font) = font {
        if !media_type.starts_with("font/")
            || !validate::valid_id(&font.id)
            || font.id == "font_bundled"
            || font.family.is_empty()
            || font.family.len() > 128
            || !(100..=900).contains(&font.weight)
            || !["normal", "italic"].contains(&font.style.as_str())
        {
            return Err(StorageError::validation(
                "font",
                "Invalid font registration",
            ));
        }
        candidate.fonts.insert(
            font.id.clone(),
            Font {
                id: font.id.clone(),
                family: font.family.clone(),
                weight: font.weight,
                style: font.style.clone(),
                asset_id: Some(asset.id.clone()),
                bundled_id: None,
                content_hash: Some(asset.content_hash.clone()),
                license: font.license.clone(),
                status: "ready".into(),
            },
        );
        changed.push(font.id.clone());
    }
    Ok(())
}
