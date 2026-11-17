use super::{public_support::PublicServer, support::Server};
use reqwest::{Method, RequestBuilder};
use std::{fs, path::PathBuf};
use tempfile::TempDir;

pub const OLD: &str = "<html><script src='/assets/index-OLD12345.js'></script></html>";
pub const NEW: &str = "<html><script src='/assets/index-NEW12345.js'></script></html>";
pub const HASHED: &str = "/assets/index-B0w-Y3c9.js";
pub const CSS: &str = "/assets/index-D4a_1234.css";
pub const WASM: &str = "/scene-wasm/fagent_scene_bg.wasm";
pub const ENGINE_JS: &str = "/engine-wasm/fagent_engine_wasm-0123456789ab.js";
pub const ENGINE_WASM: &str = "/engine-wasm/fagent_engine_wasm_bg-0123456789ab.wasm";

pub struct Fixture {
    pub local: Server,
    pub public: PublicServer,
    workspace: TempDir,
}

impl Fixture {
    pub async fn start() -> Self {
        let workspace = tempfile::tempdir().unwrap();
        let dist = workspace.path().join("web/dist");
        fs::create_dir_all(dist.join("assets")).unwrap();
        fs::create_dir_all(dist.join("scene-wasm")).unwrap();
        fs::create_dir_all(dist.join("engine-wasm/nested")).unwrap();
        fs::write(dist.join("index.html"), OLD).unwrap();
        for (path, bytes) in [
            (HASHED, "export const version = 1;"),
            (CSS, "body { color: black; }"),
            (WASM, "old-wasm"),
            ("/assets/stable.js", "old-stable"),
            ("/assets/stable.css", "body {}"),
            ("/assets/index-toolong123.js", "long-hash"),
            ("/assets/index-1234567.js", "short-hash"),
            ("/assets/index-12345678.wasm", "named-wasm"),
            ("/scene-wasm/index-12345678.js", "stable-path-js"),
            ("/assets/index-12345678.js.map", "{}"),
            ("/assets/index-B0w-Y3c9.js.br", "brotli-representation"),
            ("/assets/index-B0w-Y3c9.js.gz", "gzip-representation"),
            (ENGINE_JS, "export default function init() {}"),
            (ENGINE_WASM, "\0asm-engine"),
            (
                "/engine-wasm/fagent_engine_wasm_bg-0123456789ab.wasm.br",
                "engine-brotli",
            ),
            (
                "/engine-wasm/fagent_engine_wasm_bg-0123456789ab.wasm.gz",
                "engine-gzip",
            ),
            (
                "/engine-wasm/fagent_engine_wasm-0123456789ab.js.br",
                "engine-js-brotli",
            ),
            ("/engine-wasm/manifest.json", "{}"),
            (
                "/engine-wasm/fagent_engine_wasm_bg-0123456789a.wasm",
                "short",
            ),
            (
                "/engine-wasm/fagent_engine_wasm_bg-0123456789abc.wasm",
                "long",
            ),
            (
                "/engine-wasm/fagent_engine_wasm_bg-0123456789ag.wasm",
                "non-hex",
            ),
            (
                "/engine-wasm/nested/fagent_engine_wasm_bg-0123456789ab.wasm",
                "nested",
            ),
            (
                "/engine-wasm/other_engine_wasm_bg-0123456789ab.wasm",
                "other-prefix",
            ),
            (
                "/engine-wasm/fagent_engine_wasm-0123456789ab.wasm",
                "js-prefix-wasm",
            ),
            (
                "/engine-wasm/fagent_engine_wasm_bg-0123456789ab.js",
                "bg-prefix-js",
            ),
            (
                "/scene-wasm/fagent_engine_wasm_bg-0123456789ab.wasm",
                "scene-dir",
            ),
        ] {
            fs::write(dist.join(&path[1..]), bytes).unwrap();
        }
        let local = Server::configured(|config| config.workspace = workspace.path().into()).await;
        let public = PublicServer::start(local.state.clone()).await;
        Self {
            local,
            public,
            workspace,
        }
    }

    pub fn request(&self, public: bool, method: Method, path: &str) -> RequestBuilder {
        if public {
            self.public.request(method, path)
        } else {
            self.local
                .client
                .request(method, format!("{}{path}", self.local.url))
        }
    }

    pub fn replace_preserving_mtime(&self, path: &str, content: &str) {
        let file = self.dist().join(&path[1..]);
        let modified = fs::metadata(&file).unwrap().modified().unwrap();
        fs::write(&file, content).unwrap();
        fs::File::options()
            .write(true)
            .open(file)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
    }

    fn dist(&self) -> PathBuf {
        self.workspace.path().join("web/dist")
    }
}
