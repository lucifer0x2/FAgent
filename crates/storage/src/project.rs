use crate::*;
pub struct Project {
    pub(crate) root: Arc<Root>,
    pub(crate) path: PathBuf,
    pub(crate) project_id: String,
    pub(crate) writer: Mutex<Connection>,
    pub(crate) file_identity: Option<(u64, u64)>,
}

impl Project {
    pub fn id(&self) -> &str {
        &self.project_id
    }
    pub(crate) fn check_paths(&self) -> Result<()> {
        guard(&self.path, &self.root.path)?;
        guard(&self.path.join("assets"), &self.path)?;
        guard(&self.path.join("db.sql"), &self.path)?;
        if self.file_identity != file_identity(&self.path.join("db.sql"))? {
            return Err(StorageError::new(
                "PROJECT_REPLACED",
                "Project database was replaced; reopen the project",
            ));
        }
        no_symlink(&self.path.join("db.sql-wal"))?;
        no_symlink(&self.path.join("db.sql-shm"))?;
        Ok(())
    }
    pub(crate) fn reader(&self) -> Result<Connection> {
        self.check_paths()?;
        let c = Connection::open_with_flags(
            self.path.join("db.sql"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        configure(&c, false)?;
        Ok(c)
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        let mut conn = self.reader()?;
        let mut s = read_snapshot(&mut conn)?;
        self.apply_readiness(&mut s);
        Ok(s)
    }
    /// Files can disappear independently of SQLite: readiness is surfaced separately
    /// from stored rows, as every read sees it.
    pub(crate) fn apply_readiness(&self, s: &mut Snapshot) {
        let ready: Vec<bool> = s
            .assets
            .iter()
            .map(|asset| self.asset_path(&asset.relative_key).is_ok())
            .collect();
        apply_observed_readiness(s, &ready);
        self.fill_image_sizes(s);
    }

    /// [`Self::apply_readiness`] from an earlier observation of the same rows (one flag
    /// per asset in id order), so a snapshot and the identity that tags it agree.
    /// Observes the files again when the observation does not cover these rows.
    pub(crate) fn apply_readiness_from(&self, s: &mut Snapshot, ready: &[bool]) {
        match ready.len() == s.assets.len() {
            true => {
                apply_observed_readiness(s, ready);
                self.fill_image_sizes(s);
            }
            false => self.apply_readiness(s),
        }
    }
    pub(crate) fn update_privacy(&self, privacy: PrivacyStatus) -> Result<(ProjectMeta, bool)> {
        self.check_paths()?;
        let mut conn = self.writer.lock().map_err(|_| poisoned())?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut meta = read_meta(&tx)?;
        let changed = meta.privacy_status != privacy;
        if changed {
            tx.execute(
                "UPDATE project_meta SET privacy_status=?1 WHERE singleton=1",
                [privacy.as_str()],
            )?;
            meta.privacy_status = privacy;
        }
        tx.commit()?;
        self.root.memory.evict(&self.project_id);
        Ok((meta, changed))
    }
    pub(crate) fn request_ids(&self, project: &str, key: &str) -> Result<()> {
        request_ids(&self.project_id, project, key)
    }
    pub fn commits_after(&self, revision: u64, limit: usize) -> Result<Vec<Commit>> {
        let mut conn = self.reader()?;
        let tx = conn.transaction()?;
        let meta = read_meta(&tx)?;
        if revision > meta.revision {
            let mut e = StorageError::new(
                "RESET_REQUIRED",
                "Event cursor is ahead of the project; refetch snapshot",
            );
            e.current_revision = Some(meta.revision);
            return Err(e);
        }
        let commits = {
            let mut stmt=tx.prepare("SELECT revision,base_revision,batch_id,ops_json,summary_json FROM commits WHERE revision>?1 ORDER BY revision LIMIT ?2")?;
            let rows =
                stmt.query_map(params![revision as i64, limit.clamp(1, 1000) as i64], |r| {
                    Ok((
                        r.get::<_, i64>(0)? as u64,
                        r.get::<_, i64>(1)? as u64,
                        r.get(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                })?;
            rows.map(|r| {
                let (revision, base_revision, batch_id, ops, summary) = r?;
                Ok(Commit {
                    revision,
                    base_revision,
                    batch_id,
                    operations: scoped::operations(serde_json::from_str(&ops)?),
                    changed_ids: serde_json::from_str(&summary)?,
                })
            })
            .collect::<Result<Vec<_>>>()?
        };
        tx.commit()?;
        Ok(commits)
    }
    pub fn checkpoint(&self) -> Result<Checkpoint> {
        self.check_paths()?;
        let conn = self.writer.lock().map_err(|_| poisoned())?;
        Ok(conn.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |r| {
            Ok(Checkpoint {
                busy: r.get(0)?,
                wal_pages: r.get(1)?,
                checkpointed_pages: r.get(2)?,
            })
        })?)
    }
}

#[cfg(unix)]
pub(crate) fn file_identity(path: &Path) -> Result<Option<(u64, u64)>> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::symlink_metadata(path)?;
    Ok(Some((metadata.dev(), metadata.ino())))
}

#[cfg(not(unix))]
pub(crate) fn file_identity(_path: &Path) -> Result<Option<(u64, u64)>> {
    Ok(None)
}

/// The checks every engine makes on a write request: it names `project` and carries a
/// valid idempotency key.
pub(crate) fn request_ids(project: &str, request_project: &str, key: &str) -> Result<()> {
    if project != request_project {
        return Err(StorageError::validation(
            "projectId",
            "Request project ID does not match this project",
        ));
    }
    if !validate::valid_id(key) {
        return Err(StorageError::validation(
            "idempotencyKey",
            "Idempotency key must contain 1–128 ASCII letters, digits, underscores or hyphens",
        ));
    }
    Ok(())
}

/// Asset statuses from `ready` (one flag per asset, in snapshot order), then each
/// asset-backed font's status from its asset.
fn apply_observed_readiness(s: &mut Snapshot, ready: &[bool]) {
    for (asset, &ready) in s.assets.iter_mut().zip(ready) {
        asset.status = if ready { "ready" } else { "missing" }.into();
    }
    for font in &mut s.fonts {
        if let Some(asset) = &font.asset_id {
            font.status = s
                .assets
                .iter()
                .find(|a| &a.id == asset)
                .map_or("missing", |a| a.status.as_str())
                .into();
        }
    }
}
