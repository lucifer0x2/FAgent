//! Batch files from a configured inbox root work on a backend without its own staging (as
//! PostgreSQL), under the same request checks, containment, cap and parsing as SQLite.
mod backend_contract_support;
use backend_contract_support::*;
use fagent_storage::*;
use serde_json::Value;
use std::{fs, path::Path, sync::Arc};

/// SQLite storage that, like PostgreSQL, has no staged batch inbox of its own.
struct Unstaged(SqliteBackend);
impl StorageBackend for Unstaged {
    fn list_projects(&self) -> Result<Vec<ProjectMeta>> {
        self.0.list_projects()
    }
    fn create_project(&self, slug: &str, name: &str) -> Result<ProjectMeta> {
        self.0.create_project(slug, name)
    }
    fn project_meta(&self, key: &str) -> Result<ProjectMeta> {
        self.0.project_meta(key)
    }
    fn snapshot(&self, project: &str) -> Result<Snapshot> {
        self.0.snapshot(project)
    }
    fn apply_batch(&self, project: &str, request: BatchRequest) -> Result<BatchResult> {
        self.0.apply_batch(project, request)
    }
    fn dry_run_batch(&self, project: &str, request: BatchRequest) -> Result<DryRunResult> {
        self.0.dry_run_batch(project, request)
    }
    fn commits_after(&self, project: &str, revision: u64, limit: usize) -> Result<Vec<Commit>> {
        self.0.commits_after(project, revision, limit)
    }
    fn import_asset(
        &self,
        project: &str,
        request: ImportAssetRequest,
    ) -> Result<ImportAssetResult> {
        self.0.import_asset(project, request)
    }
    fn import_assets(
        &self,
        project: &str,
        request: ImportAssetsRequest,
    ) -> Result<ImportAssetsResult> {
        self.0.import_assets(project, request)
    }
    fn read_asset(&self, project: &str, id: &str) -> Result<(Asset, Vec<u8>)> {
        self.0.read_asset(project, id)
    }
    fn read_font(&self, project: &str, id: &str) -> Result<(Font, String, Vec<u8>)> {
        self.0.read_font(project, id)
    }
    fn read_batch_file_operations(&self, _: &str, _: &BatchFileRequest) -> Result<Value> {
        Err(StorageError::new("STAGING_UNSUPPORTED", "No staged inbox"))
    }
}

/// Storage on a data root, and a separate directory that may become the inbox root.
fn unstaged() -> (tempfile::TempDir, tempfile::TempDir, Storage) {
    let (data, inbox) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let backend = Unstaged(SqliteBackend::new(Store::open(data.path()).unwrap()));
    (data, inbox, Storage::from_shared(Arc::new(backend)))
}

fn json_ops(path: &Path, name: &str) {
    fs::write(path, serde_json::to_vec(&page(name)).unwrap()).unwrap();
}

#[test]
fn without_an_inbox_root_batch_files_stay_unsupported() {
    let (_data, _inbox, storage) = unstaged();
    let p = storage.create_project("fixture", "Fixture").unwrap();
    for path in ["ops.json", "page.ocd"] {
        let error = p.read_batch_file(&file(p.id(), 0, "k", path)).unwrap_err();
        assert_eq!(error.code, "STAGING_UNSUPPORTED", "{path}");
        let error = p.apply_batch_file(file(p.id(), 0, "k", path)).unwrap_err();
        assert_eq!(error.code, "STAGING_UNSUPPORTED", "{path}");
    }
}

#[test]
fn the_inbox_root_serves_json_and_source_like_sqlite_staging() {
    let (data, root, storage) = unstaged();
    let storage = storage.with_inbox_root(root.path());
    let p = storage.create_project("fixture", "Fixture").unwrap();
    // Nothing creates the inbox; a missing one reads as a missing file.
    let error = p
        .read_batch_file(&file(p.id(), 0, "k", "ops.json"))
        .unwrap_err();
    assert_eq!(error.code, "FILE_NOT_FOUND");
    let inbox = root.path().join("fixture/inbox");
    fs::create_dir_all(&inbox).unwrap();
    // The SQLite data root's inbox is not the configured one.
    fs::create_dir_all(data.path().join("fixture/inbox")).unwrap();
    json_ops(&data.path().join("fixture/inbox/data.json"), "Data root");
    let error = p
        .read_batch_file(&file(p.id(), 0, "k", "data.json"))
        .unwrap_err();
    assert_eq!(error.code, "FILE_NOT_FOUND");

    json_ops(&inbox.join("ops.json"), "From inbox");
    let req = file(p.id(), 0, "file-key", "ops.json");
    assert!(p.dry_run_batch_file(req.clone()).unwrap().dry_run);
    assert_eq!(p.snapshot().unwrap().revision, 0);
    assert_eq!(p.apply_batch_file(req).unwrap().committed_revision, 1);
    let inline = request(p.id(), 0, "file-key", page("From inbox"));
    assert!(p.apply_batch(inline).unwrap().replayed);
    let absolute = inbox.canonicalize().unwrap().join("ops.json");
    let read = p
        .read_batch_file_operations(&file(p.id(), 1, "abs", absolute.to_str().unwrap()))
        .unwrap();
    assert_eq!(read, serde_json::to_value(page("From inbox")).unwrap());

    fs::write(inbox.join("login.OCD"), "\u{feff}page#home\n").unwrap();
    let read = p
        .read_batch_file(&file(p.id(), 1, "ocd", "login.OCD"))
        .unwrap();
    assert_eq!(read, BatchFile::Source("page#home\n".into()));
    // Storage never applies source; the MCP server compiles it.
    let error = p
        .apply_batch_file(file(p.id(), 1, "ocd", "login.OCD"))
        .unwrap_err();
    assert_eq!(error.field.as_deref(), Some("path"), "{}", error.message);
    fs::write(inbox.join("binary.ocd"), [0xff, 0xfe, 0x00]).unwrap();
    let error = p
        .read_batch_file(&file(p.id(), 1, "ocd", "binary.ocd"))
        .unwrap_err();
    assert_eq!(error.code, "VALIDATION_FAILED");
    fs::write(inbox.join("empty.json"), b"[]").unwrap();
    let error = p
        .read_batch_file(&file(p.id(), 1, "e", "empty.json"))
        .unwrap_err();
    assert_eq!(error.field.as_deref(), Some("operations"));
    assert_eq!(p.snapshot().unwrap().revision, 1);
}

#[test]
fn the_inbox_root_keeps_request_checks_containment_and_the_cap() {
    let (_data, root, storage) = unstaged();
    let storage = storage.with_inbox_root(root.path());
    let p = storage.create_project("fixture", "Fixture").unwrap();
    let inbox = root.path().join("fixture/inbox");
    fs::create_dir_all(&inbox).unwrap();
    json_ops(&inbox.join("ops.json"), "Inbox");
    let mut other = file(p.id(), 0, "k", "ops.json");
    other.project_id = "someone-else".into();
    let error = p.read_batch_file(&other).unwrap_err();
    assert_eq!(error.field.as_deref(), Some("projectId"));
    let error = p
        .read_batch_file(&file(p.id(), 0, "bad key", "ops.json"))
        .unwrap_err();
    assert_eq!(error.field.as_deref(), Some("idempotencyKey"));

    fs::write(root.path().join("outside.json"), b"[]").unwrap();
    let outside = root.path().canonicalize().unwrap().join("outside.json");
    for bad in [
        "../outside.json",
        "sub/../../ops.json",
        outside.to_str().unwrap(),
        "",
        ".",
    ] {
        let error = p.read_batch_file(&file(p.id(), 0, "k", bad)).unwrap_err();
        assert_eq!(error.code, "PATH_REJECTED", "{bad:?}");
        assert!(error.message.starts_with("Batch"), "{}", error.message);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(&outside, inbox.join("link.json")).unwrap();
        let error = p
            .read_batch_file(&file(p.id(), 0, "k", "link.json"))
            .unwrap_err();
        assert_eq!(error.code, "PATH_REJECTED");
    }
    fs::File::create(inbox.join("big.ocd"))
        .unwrap()
        .set_len(MAX_BATCH_BYTES as u64 + 1)
        .unwrap();
    let error = p
        .read_batch_file(&file(p.id(), 0, "k", "big.ocd"))
        .unwrap_err();
    assert_eq!(error.message, "Batch file exceeds 2 MiB");
    #[cfg(unix)]
    {
        // A symlinked inbox is rejected, as on SQLite.
        fs::rename(&inbox, root.path().join("hidden")).unwrap();
        std::os::unix::fs::symlink(root.path().join("hidden"), &inbox).unwrap();
        let error = p
            .read_batch_file(&file(p.id(), 0, "k", "ops.json"))
            .unwrap_err();
        assert_eq!(error.code, "PATH_REJECTED");
    }
    assert_eq!(p.snapshot().unwrap().revision, 0);
}
