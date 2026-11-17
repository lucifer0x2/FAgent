//! PostgreSQL persistence for the complete authored product domain.
//! Operations run on a bounded pool of reusable, dedicated blocking workers.
use crate::{Result, SnapshotCache};
use std::{sync::Arc, time::Duration};

mod adapter;
mod batch;
mod commit;
mod config;
mod failure;
#[cfg(test)]
mod failure_tests;
mod history;
mod memory_write;
mod pool;
mod projects;
mod resources;
mod schema;
mod schema_name;
mod shadow_hook;
mod snapshot;
mod statements;
#[cfg(test)]
mod tests;
mod transaction;
mod viewer_auth;
mod write_through;
pub use config::PostgresConfig;
use failure::{sql, unavailable};
pub use schema_name::{DatabaseSchema, DEFAULT_DATABASE_SCHEMA};
use statements::{Db, Session};

/// A cloneable adapter. Construction initializes only its versioned SQL schema;
/// project creation remains an explicit domain operation.
#[derive(Clone)]
pub struct PostgresBackend {
    pool: Arc<pool::Pool>,
    namespace: Arc<str>,
    cache: Option<SnapshotCache>,
    indexes: crate::revision_indexes::IndexCache,
    shadow: crate::shadow::Shadow,
}

impl PostgresBackend {
    pub fn connect(url: &str) -> Result<Self> {
        Self::with_config(PostgresConfig::new(url))
    }

    pub fn with_config(config: PostgresConfig) -> Result<Self> {
        let shadow = crate::shadow::Shadow::new(
            config.node_memory_shadow,
            config.node_memory_reads,
            config.node_memory_writes,
        );
        let runtime_schema = config.schema.clone();
        let pool = Arc::new(pool::Pool::new(config)?);
        let namespace =
            pool.run(move |session| schema::initialize(&mut session.client, &runtime_schema))?;
        Ok(Self {
            pool,
            namespace: namespace.into(),
            cache: None,
            indexes: crate::revision_indexes::IndexCache::default().with_owner(shadow.clone()),
            shadow,
        })
    }

    /// Durable database identity for an opt-in snapshot cache.
    pub fn cache_namespace(&self) -> &str {
        &self.namespace
    }

    /// Cache reads are always preceded by a successful authoritative SQL probe.
    pub fn with_cache(mut self, cache: SnapshotCache) -> Result<Self> {
        if cache.namespace() != self.cache_namespace() {
            return Err(crate::StorageError::new(
                "INVALID_CACHE_CONFIG",
                "Snapshot cache namespace does not match this database",
            ));
        }
        self.cache = Some(cache.with_owner(self.shadow.clone()));
        Ok(self)
    }
}

fn milliseconds(value: Duration) -> u64 {
    value.as_millis().clamp(1, u32::MAX as u128) as u64
}
