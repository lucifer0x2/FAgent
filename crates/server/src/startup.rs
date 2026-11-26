mod artifacts;
mod note_limits;
mod options;
mod postgres_env;

use fagent_server::{PublicViewer, RenderBackend};
use fagent_storage::{NoteLimits, PostgresBackend, PostgresConfig, Storage, Store};
use options::{
    bind_host, env_port, live_updates, render_backend, switch, trusted_proxy, viewer_overscan,
};
use std::{env, error::Error, net::Ipv4Addr, path::PathBuf};

pub const HELP: &str =
    "fagent-server [--data-root DIR] [--port NUMBER] [--workspace DIR] [--host 127.0.0.1|0.0.0.0]
[--public-viewer-port 3848 --public-viewer-origin https://machine.tailnet.ts.net]
[--public-viewer-live-updates on|off]
[--render-backend chromium|vello]
RENDER_BACKEND defaults to the backend chosen at build time by FAGENT_RENDER_BACKEND (chromium
unless the binary was built with vello). --render-backend overrides RENDER_BACKEND. chromium
needs the Node worker runtime (worker/, renderer/dist, layout-step) in the workspace; a build
made with FAGENT_RENDER_BACKEND=vello does not package it, and starting it with chromium fails.
DATABASE_PROVIDER defaults to sqlite. Use postgresql with DATABASE_URL to enable PostgreSQL.
PostgreSQL uses the data root only to read import_asset files from <data-root>/<slug>/inbox;
it creates nothing there.
Optional PostgreSQL limits: DATABASE_POOL_SIZE (1-32, default 4) and, in milliseconds,
DATABASE_CONNECT_TIMEOUT_MS (3000), DATABASE_STATEMENT_TIMEOUT_MS (15000),
DATABASE_LOCK_TIMEOUT_MS (3000), DATABASE_OPERATION_TIMEOUT_MS (30000).
FAGENT_DATABASE_SCHEMA selects the schema for the runtime tables (default fagent_runtime_v1).
SQLite data root defaults to ~/.fagent. Neither backend creates a project until create_project.
--port overrides PORT (default 3847). Use 0 to bind an available port.
--host defaults to 127.0.0.1. 0.0.0.0 listens on every interface and is meant for a container
whose ports are published on host loopback only: the API has no authentication.
Public viewer is disabled by default; both public options are required. This separate listener
shares public projects, their resources and exports. Projects default to private.
Public live updates (event streams) are off by default: a public page shows the revision current
when it loaded. FAGENT_PUBLIC_LIVE_UPDATES=on or --public-viewer-live-updates on enables them;
the flag overrides the variable.
Local MCP can require a password for the public viewer. Behind a trusted reverse proxy set
FAGENT_PUBLIC_TRUSTED_PROXY_IP to its socket peer IP to use a single X-Forwarded-For address.
FAGENT_PUBLIC_ALLOWED_ORIGINS adds comma-separated exact HTTPS origins (at most 8 total).
Point Funnel only at the public viewer port, NEVER at the local MCP port.
FAGENT_VIEWER_OVERSCAN sets how far the Vello viewer draws beyond each viewport edge, as a
percent per side from 0 to 50 (for example 10 or 12.5). It defaults to 0 (off); both listeners
report it as the overscan ratio in /api/v1/viewer.
Note limits can only be raised: FAGENT_MAX_NOTES per project (default 20000, at most 1000000),
FAGENT_MAX_NOTE_TEXT characters per note or reply (2000, at most 100000),
FAGENT_MAX_NOTE_REPLIES per note (100, at most 1000) and FAGENT_MAX_NOTE_AUTHOR characters
(64, at most 256).";

pub struct Startup {
    pub storage: StorageOptions,
    pub port: u16,
    pub host: Ipv4Addr,
    pub public_viewer: Option<(u16, PublicViewer)>,
    pub workspace: PathBuf,
    pub render_backend: RenderBackend,
    /// Viewer overscan ratio per side (`FAGENT_VIEWER_OVERSCAN` percent / 100), 0 when unset.
    pub viewer_overscan: f64,
    /// Note limits from `FAGENT_MAX_NOTE*`; the defaults when unset.
    pub note_limits: NoteLimits,
}

impl Startup {
    /// Returns no startup configuration when help was requested.
    pub fn from_env() -> Result<Option<Self>, Box<dyn Error>> {
        if env::args().skip(1).any(|arg| arg == "--help") {
            return Ok(None);
        }
        let mut data_root = env::var_os("FAGENT_DATA_ROOT").map(PathBuf::from);
        let mut port = None;
        let mut host = Ipv4Addr::LOCALHOST;
        let mut public_port = None;
        let mut public_origin = None;
        let mut public_live = None;
        let mut backend = None;
        let mut workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--data-root" => {
                    data_root = Some(PathBuf::from(
                        args.next().ok_or("--data-root needs a directory")?,
                    ))
                }
                "--port" => {
                    port = Some(
                        args.next()
                            .ok_or("--port needs a number")?
                            .parse()
                            .map_err(|_| "--port must be a number from 0 to 65535")?,
                    )
                }
                "--host" => host = bind_host(&args.next().ok_or("--host needs an address")?)?,
                "--public-viewer-port" => {
                    public_port = Some(
                        args.next()
                            .ok_or("--public-viewer-port needs a number")?
                            .parse()
                            .map_err(|_| "--public-viewer-port must be a number from 0 to 65535")?,
                    )
                }
                "--public-viewer-origin" => {
                    public_origin = Some(PublicViewer::new(
                        &args
                            .next()
                            .ok_or("--public-viewer-origin needs an HTTPS origin")?,
                    )?)
                }
                "--public-viewer-live-updates" => {
                    public_live = Some(
                        switch(
                            &args
                                .next()
                                .ok_or("--public-viewer-live-updates needs on or off")?,
                        )
                        .ok_or("--public-viewer-live-updates must be on or off")?,
                    )
                }
                "--workspace" => {
                    workspace = PathBuf::from(args.next().ok_or("--workspace needs a directory")?)
                }
                "--render-backend" => {
                    backend = Some(
                        args.next()
                            .ok_or("--render-backend needs chromium or vello")?
                            .parse()
                            .map_err(|_| "--render-backend must be chromium or vello")?,
                    )
                }
                _ => return Err("Unknown server option".into()),
            }
        }

        let render_backend = render_backend(backend)?;
        let viewer_overscan = viewer_overscan()?;
        let note_limits = note_limits::note_limits()?;
        let port = match port {
            Some(port) => port,
            None => env_port()?,
        };
        let public_viewer = match (public_port, public_origin) {
            (Some(p), Some(_)) if p != 0 && p == port => {
                return Err("Local and public viewer ports must differ".into());
            }
            (Some(p), Some(viewer)) => {
                let viewer = match env::var("FAGENT_PUBLIC_ALLOWED_ORIGINS") {
                    Ok(origins) => viewer.with_allowed_origins(&origins)?,
                    Err(env::VarError::NotPresent) => viewer,
                    Err(_) => return Err("Invalid FAGENT_PUBLIC_ALLOWED_ORIGINS".into()),
                };
                let viewer = viewer.with_live_updates(live_updates(public_live)?);
                Some((
                    p,
                    match trusted_proxy()? {
                        Some(proxy) => viewer.with_trusted_proxy(proxy),
                        None => viewer,
                    },
                ))
            }
            (None, None) if public_live.is_some() => {
                return Err("--public-viewer-live-updates needs the public viewer options".into());
            }
            (None, None) => None,
            _ => {
                return Err(
                    "--public-viewer-port and --public-viewer-origin must be supplied together"
                        .into(),
                );
            }
        };

        let storage = StorageOptions::from_env(data_root)?;
        artifacts::check(render_backend, &workspace)?;
        Ok(Some(Self {
            storage,
            port,
            host,
            public_viewer,
            workspace,
            render_backend,
            viewer_overscan,
            note_limits,
        }))
    }
}

pub enum StorageOptions {
    Sqlite(PathBuf),
    /// The optional root holds only filesystem inboxes for asset paths and batch files.
    Postgres(PostgresConfig, Option<PathBuf>),
}

impl StorageOptions {
    fn from_env(data_root: Option<PathBuf>) -> Result<Self, Box<dyn Error>> {
        let data_root = data_root
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".fagent")));
        let provider = env::var_os("DATABASE_PROVIDER");
        match provider.as_deref() {
            Some(value) if value == "postgresql" => {
                let url = env::var("DATABASE_URL")
                    .ok()
                    .filter(|url| !url.trim().is_empty())
                    .ok_or("DATABASE_URL must be set to a nonempty value for PostgreSQL")?;
                let config = postgres_env::config(url, |name| env::var_os(name))?;
                Ok(Self::Postgres(config, data_root))
            }
            Some(value) if value != "sqlite" => {
                Err("DATABASE_PROVIDER must be sqlite or postgresql".into())
            }
            _ => Ok(data_root.map(Self::Sqlite).ok_or("Data root is required")?),
        }
    }

    pub fn open(self) -> Result<Storage, Box<dyn Error>> {
        match self {
            Self::Sqlite(root) => Ok(Store::open(root)?.into()),
            Self::Postgres(config, inbox_root) => {
                let backend = PostgresBackend::with_config(config).map_err(|_| {
                    "PostgreSQL startup failed. Check DATABASE_URL and database access"
                })?;
                let storage = Storage::new(backend);
                Ok(match inbox_root {
                    Some(root) => storage.with_inbox_root(root),
                    None => storage,
                })
            }
        }
    }
}
