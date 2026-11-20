pub mod cell;
pub mod commits;
pub mod mcp;
mod parity;
pub mod scale;
pub mod seed;
pub mod snapshots;
pub mod source;
pub mod stats;

use fagent_storage::{PostgresBackend, PostgresConfig, Storage, Store};
use std::{path::Path, sync::Arc, time::Duration};

/// The production-shaped storage: node memory on; `database_snapshot` reads bypass it.
#[derive(Clone)]
pub struct Backends {
    pub primary: Storage,
    _root: Option<Arc<tempfile::TempDir>>,
}

impl Backends {
    pub fn sqlite(root: Arc<tempfile::TempDir>) -> Self {
        let storage = Storage::from(Store::open(root.path()).unwrap());
        Self {
            primary: storage,
            _root: Some(root),
        }
    }

    /// Production pool size; the 15 s statement and 30 s operation timeouts are
    /// raised so the largest scales report a time instead of STORAGE_UNAVAILABLE.
    pub fn postgres(url: &str) -> Self {
        let config = || {
            let mut config = PostgresConfig::new(url);
            config.statement_timeout = Duration::from_secs(600);
            config.operation_timeout = Duration::from_secs(600);
            config
        };
        Self {
            primary: Storage::new(PostgresBackend::with_config(config()).unwrap()),
            _root: None,
        }
    }
}

pub fn temp_root() -> Arc<tempfile::TempDir> {
    let parent = std::env::var("TASK24_SQLITE_PARENT")
        .unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned());
    Arc::new(
        tempfile::Builder::new()
            .prefix("task24-sqlite.")
            .tempdir_in(Path::new(&parent))
            .unwrap(),
    )
}
