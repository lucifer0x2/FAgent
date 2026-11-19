//! PostgreSQL persistence for the complete authored product domain.
//! Operations run on a bounded pool of reusable, dedicated blocking workers.
use crate::Result;
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
mod snapshot;
mod statements;
#[cfg(test)]
mod tests;
mod transaction;
mod viewer_auth;
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
    memory: crate::memory::Memory,
}

impl PostgresBackend {
    pub fn connect(url: &str) -> Result<Self> {
        Self::with_config(PostgresConfig::new(url))
    }

    pub fn with_config(config: PostgresConfig) -> Result<Self> {
        let memory = crate::memory::Memory::default();
        let runtime_schema = config.schema.clone();
        let pool = Arc::new(pool::Pool::new(config)?);
        let namespace =
            pool.run(move |session| schema::initialize(&mut session.client, &runtime_schema))?;
        Ok(Self {
            pool,
            namespace: namespace.into(),
            memory,
        })
    }

    /// The durable identity of this database, stored in its schema registry. Snapshot
    /// identities carry it in their incarnation (`postgres:{generation}:{namespace}`).
    pub fn database_namespace(&self) -> &str {
        &self.namespace
    }
}

fn milliseconds(value: Duration) -> u64 {
    value.as_millis().clamp(1, u32::MAX as u128) as u64
}
