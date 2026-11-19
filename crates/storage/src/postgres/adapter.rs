use super::*;
use crate::{
    Asset, BatchFileRequest, BatchRequest, BatchResult, Commit, DryRunResult, Font,
    ImportAssetRequest, ImportAssetResult, ImportAssetsRequest, ImportAssetsResult, PrivacyStatus,
    ProjectMeta, Snapshot, SnapshotCacheIdentity, StorageBackend, StorageError,
};
use serde_json::Value;
use std::sync::Arc;

impl PostgresBackend {
    /// One import commit: the transaction, then the memory eviction.
    fn import(
        &self,
        project: &str,
        request: ImportAssetsRequest,
        fields: crate::asset_import::Fields,
    ) -> Result<ImportAssetsResult> {
        let key = project.to_owned();
        let result = self
            .pool
            .run(move |session| resources::import(session, &key, request, fields))?;
        self.memory.evict(&result.result.project_id);
        Ok(result)
    }

    /// One repeatable-read load on a pool worker, with the identity of that read.
    fn read_snapshot(&self, project: &str) -> Result<snapshot::Identified> {
        let key = project.to_owned();
        self.pool.run(move |session| snapshot::read(session, &key))
    }

    /// The database shared snapshot with the identity of the same read.
    fn shared_with_identity(&self, project: &str) -> Result<crate::memory::Loaded> {
        let (snapshot, identity) = self.database_snapshot(project)?;
        let indexes = self.indexes.for_snapshot(&snapshot, identity.clone());
        Ok((
            crate::SharedSnapshot::committed(snapshot, indexes),
            identity,
        ))
    }
}

impl StorageBackend for PostgresBackend {
    fn apply_scoped_batch(
        &self,
        project: &str,
        request: crate::ScopedBatchRequest,
    ) -> Result<BatchResult> {
        let (request, pages) = request.split();
        self.commit(project, request, Some(pages))
    }
    fn dry_run_scoped_batch(
        &self,
        project: &str,
        request: crate::ScopedBatchRequest,
    ) -> Result<DryRunResult> {
        let (request, pages) = request.split();
        self.preview(project, request, Some(pages))
    }
    fn viewer_auth(&self) -> Option<&dyn crate::ViewerAuthStore> {
        Some(self)
    }

    fn list_projects(&self) -> Result<Vec<ProjectMeta>> {
        self.pool.run(|session| {
            let mut projects: Vec<ProjectMeta> = session
                .db()
                .query(projects::LIST, &[])?
                .into_iter()
                .map(|row| Ok(projects::decode(row)?.meta))
                .collect::<Result<_>>()?;
            projects.sort_by(|a, b| a.slug.cmp(&b.slug));
            Ok(projects)
        })
    }

    fn create_project(&self, slug: &str, name: &str) -> Result<ProjectMeta> {
        let (slug, name) = (slug.to_owned(), name.to_owned());
        self.pool
            .run(move |session| projects::create(&mut session.client, &slug, &name))
    }

    fn project_meta(&self, key: &str) -> Result<ProjectMeta> {
        let key = key.to_owned();
        self.pool
            .run(move |session| Ok(projects::lookup(&mut session.db(), &key, false)?.meta))
    }

    fn update_project_privacy(
        &self,
        key: &str,
        privacy: PrivacyStatus,
    ) -> Result<(ProjectMeta, bool)> {
        let key = key.to_owned();
        let updated = self
            .pool
            .run(move |session| projects::update_privacy(session, &key, privacy));
        if let Ok((meta, _)) = &updated {
            self.memory.evict(&meta.project_id);
        }
        updated
    }

    fn snapshot(&self, project: &str) -> Result<Snapshot> {
        self.read_snapshot(project)
            .map(|(snapshot, _, _)| Arc::unwrap_or_clone(snapshot))
    }

    fn snapshot_cache_identity(&self, project: &str) -> Result<Option<SnapshotCacheIdentity>> {
        let key = project.to_owned();
        let row = self
            .pool
            .run(move |session| projects::lookup(&mut session.db(), &key, false))?;
        Ok(Some(snapshot::identity(
            row.meta.project_id,
            row.meta.schema_version,
            row.meta.revision,
            row.generation,
            &row.namespace,
        )))
    }

    fn snapshot_with_cache_identity(
        &self,
        project: &str,
    ) -> Result<(Arc<Snapshot>, Option<SnapshotCacheIdentity>)> {
        self.database_snapshot(project)
    }

    fn database_snapshot(
        &self,
        project: &str,
    ) -> Result<(Arc<Snapshot>, Option<SnapshotCacheIdentity>)> {
        let (snapshot, generation, namespace) = self.read_snapshot(project)?;
        let identity = snapshot::identity(
            snapshot.project_id.clone(),
            snapshot.schema_version,
            snapshot.revision,
            generation,
            &namespace,
        );
        Ok((snapshot, Some(identity)))
    }

    fn shared_snapshot(&self, project: &str) -> Result<crate::SharedSnapshot> {
        self.shared_with_identity(project).map(|(shared, _)| shared)
    }

    fn scoped_snapshot(&self, project: &str, target: &str) -> Result<crate::ScopedRead> {
        let read = self.memory.scoped_read(
            target,
            || self.snapshot_cache_identity(project),
            || self.shared_with_identity(project),
        )?;
        if self
            .memory
            .claim_cache_cleanup(&read.shared.snapshot.project_id)
        {
            self.indexes.evict(&read.shared.snapshot.project_id);
        }
        Ok(read)
    }

    fn memory_stats(&self) -> crate::NodeMemoryStats {
        self.memory.stats()
    }

    fn memory_read_failed(&self) {
        self.memory.read_failed()
    }

    fn evict_memory(&self, project_id: &str) {
        self.memory.evict(project_id)
    }

    fn memory_snapshot(&self, project_id: &str) -> Option<Snapshot> {
        self.memory.snapshot(project_id)
    }

    fn apply_batch(&self, project: &str, request: BatchRequest) -> Result<BatchResult> {
        self.commit(project, request, None)
    }

    fn dry_run_batch(&self, project: &str, request: BatchRequest) -> Result<DryRunResult> {
        self.preview(project, request, None)
    }

    fn commits_after(&self, project: &str, revision: u64, limit: usize) -> Result<Vec<Commit>> {
        let key = project.to_owned();
        self.pool
            .run(move |session| history::after(session, &key, revision, limit))
    }

    fn import_asset(
        &self,
        project: &str,
        request: ImportAssetRequest,
    ) -> Result<ImportAssetResult> {
        let mut imported =
            self.import(project, request.into(), crate::asset_import::Fields::Single)?;
        Ok(ImportAssetResult {
            asset: imported.assets.remove(0),
            result: imported.result,
        })
    }

    fn import_assets(
        &self,
        project: &str,
        request: ImportAssetsRequest,
    ) -> Result<ImportAssetsResult> {
        self.import(project, request, crate::asset_import::Fields::Indexed)
    }

    fn read_asset(&self, project: &str, id: &str) -> Result<(Asset, Vec<u8>)> {
        let (key, id) = (project.to_owned(), id.to_owned());
        self.pool
            .run(move |session| resources::read_asset(session, &key, &id))
    }

    fn read_font(&self, project: &str, id: &str) -> Result<(Font, String, Vec<u8>)> {
        let (key, id) = (project.to_owned(), id.to_owned());
        self.pool
            .run(move |session| resources::read_font(session, &key, &id))
    }

    fn read_batch_file_operations(
        &self,
        project: &str,
        _request: &BatchFileRequest,
    ) -> Result<Value> {
        self.project_meta(project)?;
        Err(StorageError::new(
            "STAGING_UNSUPPORTED",
            "This storage backend does not provide a staged operations inbox",
        ))
    }
}
