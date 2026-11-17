//! Asset imports and batch files may name a file in the project's inbox instead of carrying
//! their bytes. The inbox is a local directory for every engine: SQLite uses its data root,
//! and any other backend uses the root given to `with_inbox_root`. Nothing here creates a path.
use super::{ProjectHandle, Storage};
use crate::{
    batch_file,
    inbox::{self, Purpose},
    validate::validate_slug,
    BatchFile, BatchFileRequest, Result, StorageError,
};
use std::{path::PathBuf, sync::Arc};

impl Storage {
    /// Selects the local directory whose `<slug>/inbox` holds files for path imports and
    /// batch files. It is resolved on each read, so it may appear after startup.
    pub fn with_inbox_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.inbox_root = Some(Arc::new(root.into()));
        self
    }

    /// The configured inbox root as an absolute path, or `None` without one. It is canonical
    /// when it exists, so `<root>/<slug>/inbox/<file>` is accepted as an absolute `path`;
    /// nothing is created.
    pub fn inbox_root(&self) -> Option<PathBuf> {
        let root = self.inbox_root.as_deref()?;
        std::fs::canonicalize(root)
            .or_else(|_| std::path::absolute(root))
            .ok()
    }
}

impl ProjectHandle {
    /// Bytes of one asset file in this project's inbox, at most `MAX_ASSET_BYTES`, under
    /// the same containment rules as staged batches: no traversal, symlinks or special files.
    pub fn read_inbox_asset(&self, path: &str) -> Result<Vec<u8>> {
        let (root, project) = self.inbox(Purpose::Asset)?.ok_or_else(|| {
            StorageError::new(
                "STAGING_UNSUPPORTED",
                "This server has no filesystem inbox; send base64",
            )
        })?;
        inbox::read(&root, &project, path, Purpose::Asset)
    }

    /// A batch file from the configured inbox root, read exactly as SQLite reads its own
    /// inbox (`batch_file::read_staged`); `None` when no inbox root is configured.
    pub(super) fn read_inbox_batch(&self, request: &BatchFileRequest) -> Result<Option<BatchFile>> {
        if self.storage.inbox_root.is_none() {
            return Ok(None);
        }
        // Request checks precede any metadata or filesystem access, as in SQLite.
        crate::project::request_ids(self.id(), &request.project_id, &request.idempotency_key)?;
        let Some((root, project)) = self.inbox(Purpose::Batch)? else {
            return Ok(None);
        };
        batch_file::read_staged(&root, &project, self.id(), request).map(Some)
    }

    /// The canonical inbox root and this project's directory below it, or `None` without a
    /// configured root. An absent root reads as an absent file.
    fn inbox(&self, purpose: Purpose) -> Result<Option<(PathBuf, PathBuf)>> {
        let meta = self.storage.backend.project_meta(self.id())?;
        validate_slug(&meta.slug)?;
        let Some(root) = self.storage.inbox_root.as_deref() else {
            return Ok(None);
        };
        // The descriptor walk opens the canonical root; an absent root is an absent file.
        let root = std::fs::canonicalize(root).map_err(|_| purpose.missing())?;
        let project = root.join(&meta.slug);
        Ok(Some((root, project)))
    }
}
