//! The storage engine chosen by `DATABASE_PROVIDER`. Both engines need a data root: SQLite
//! keeps its projects there, and PostgreSQL reads import and batch files from
//! `<data-root>/<slug>/inbox`, so a missing root fails at startup instead of at the first call.
use super::postgres_env;
use fagent_storage::{PostgresBackend, PostgresConfig, Storage, Store};
use std::{env, error::Error, ffi::OsString, path::PathBuf};

const NO_DATA_ROOT: &str =
    "Data root is required: pass --data-root DIR or set FAGENT_DATA_ROOT (or HOME for ~/.fagent)";

pub enum StorageOptions {
    Sqlite(PathBuf),
    /// The root holds only filesystem inboxes for asset paths and batch files.
    Postgres(PostgresConfig, PathBuf),
}

impl StorageOptions {
    pub(super) fn from_env(data_root: Option<PathBuf>) -> Result<Self, Box<dyn Error>> {
        Self::resolve(data_root, |name| env::var_os(name))
    }

    /// `data_root` comes from `--data-root` or `FAGENT_DATA_ROOT`; `lookup` reads the rest.
    fn resolve(
        data_root: Option<PathBuf>,
        lookup: impl Fn(&str) -> Option<OsString>,
    ) -> Result<Self, Box<dyn Error>> {
        let data_root =
            data_root.or_else(|| lookup("HOME").map(|home| PathBuf::from(home).join(".fagent")));
        match lookup("DATABASE_PROVIDER").as_deref() {
            Some(value) if value == "postgresql" => {
                let url = lookup("DATABASE_URL")
                    .and_then(|url| url.into_string().ok())
                    .filter(|url| !url.trim().is_empty())
                    .ok_or("DATABASE_URL must be set to a nonempty value for PostgreSQL")?;
                let config = postgres_env::config(url, &lookup)?;
                Ok(Self::Postgres(config, data_root.ok_or(NO_DATA_ROOT)?))
            }
            Some(value) if value != "sqlite" => {
                Err("DATABASE_PROVIDER must be sqlite or postgresql".into())
            }
            _ => Ok(Self::Sqlite(data_root.ok_or(NO_DATA_ROOT)?)),
        }
    }

    pub fn open(self) -> Result<Storage, Box<dyn Error>> {
        match self {
            Self::Sqlite(root) => Ok(Store::open(root)?.into()),
            Self::Postgres(config, inbox_root) => {
                let backend = PostgresBackend::with_config(config).map_err(|_| {
                    "PostgreSQL startup failed. Check DATABASE_URL and database access"
                })?;
                let storage = Storage::new(backend).with_inbox_root(inbox_root);
                if let Some(root) = storage.inbox_root() {
                    println!(
                        "PostgreSQL file inputs: {}/<slug>/inbox (inboxDir in list_projects)",
                        root.display()
                    );
                }
                Ok(storage)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::StorageOptions;
    use std::{collections::HashMap, ffi::OsString, path::PathBuf};

    fn resolve(root: Option<&str>, vars: &[(&str, &str)]) -> Result<StorageOptions, String> {
        let vars: HashMap<&str, OsString> = vars.iter().map(|(k, v)| (*k, (*v).into())).collect();
        StorageOptions::resolve(root.map(PathBuf::from), |name| vars.get(name).cloned())
            .map_err(|error| error.to_string())
    }

    const POSTGRES: [(&str, &str); 2] = [
        ("DATABASE_PROVIDER", "postgresql"),
        ("DATABASE_URL", "postgresql://user@db/fagent"),
    ];

    #[test]
    fn postgres_without_a_data_root_fails_like_sqlite() {
        let postgres = resolve(None, &POSTGRES).err().unwrap();
        let sqlite = resolve(None, &[]).err().unwrap();
        assert!(postgres.starts_with("Data root is required"), "{postgres}");
        assert!(postgres.contains("--data-root") && postgres.contains("FAGENT_DATA_ROOT"));
        assert_eq!(postgres, sqlite);
    }

    #[test]
    fn postgres_reads_its_inbox_from_the_data_root_or_home() {
        let Ok(StorageOptions::Postgres(_, root)) = resolve(Some("/data"), &POSTGRES) else {
            panic!("expected PostgreSQL");
        };
        assert_eq!(root, PathBuf::from("/data"));
        let mut vars = POSTGRES.to_vec();
        vars.push(("HOME", "/srv/fixture"));
        let Ok(StorageOptions::Postgres(_, root)) = resolve(None, &vars) else {
            panic!("expected PostgreSQL");
        };
        assert_eq!(root, PathBuf::from("/srv/fixture/.fagent"));
    }

    #[test]
    fn sqlite_stays_the_default_engine() {
        let Ok(StorageOptions::Sqlite(root)) = resolve(Some("/data"), &[]) else {
            panic!("expected SQLite");
        };
        assert_eq!(root, PathBuf::from("/data"));
        let error = resolve(Some("/data"), &[("DATABASE_PROVIDER", "mysql")]).err();
        assert_eq!(
            error.as_deref(),
            Some("DATABASE_PROVIDER must be sqlite or postgresql")
        );
    }
}
