//! Generated projects (`crates/node-memory/fixtures/generate.mjs`, never printed)
//! seeded into SQLite and copied, so memory-on and memory-off stores hold
//! identical bytes. Needs Node.js >= 20 on PATH.
use fagent_storage::{ReadsMode, ShadowMode, Snapshot, Storage, Store};
use std::path::Path;
use std::process::{Command, Stdio};

pub(super) fn generate(nodes: usize, dir: &Path) -> Snapshot {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = dir.join(format!("fixture-{nodes}.json"));
    let status = Command::new("node")
        .arg(manifest.join("../node-memory/fixtures/generate.mjs"))
        .args(["--nodes", &nodes.to_string(), "--seed", "7", "--out"])
        .arg(&out)
        .stdout(Stdio::null())
        .status()
        .expect("node on PATH");
    assert!(status.success(), "generate.mjs failed");
    let snapshot = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    std::fs::remove_file(out).unwrap();
    snapshot
}

/// Writes the snapshot's rows straight into a fresh project at revision 0.
fn seed(db: &Path, snapshot: &Snapshot) {
    let mut conn = rusqlite::Connection::open(db).unwrap();
    let tx = conn.transaction().unwrap();
    {
        let sql = "INSERT INTO nodes(id,parent_id,sort_key,kind,props_json) VALUES(?1,?2,?3,?4,?5)";
        let mut node = tx.prepare(sql).unwrap();
        for n in &snapshot.nodes {
            let kind = serde_json::to_value(&n.kind).unwrap();
            let props = serde_json::to_string(&n.props).unwrap();
            let row = (
                &n.id,
                &n.parent_id,
                n.sort_key,
                kind.as_str().unwrap(),
                props,
            );
            node.execute(row).unwrap();
        }
        let row = |table: &str, id: &str, data: String| {
            let sql = format!("INSERT OR REPLACE INTO {table}(id,data_json) VALUES(?1,?2)");
            tx.execute(&sql, (id, data)).unwrap();
        };
        fn text<T: serde::Serialize>(row: &T) -> String {
            serde_json::to_string(row).unwrap()
        }
        snapshot
            .tokens
            .iter()
            .for_each(|t| row("tokens", &t.id, text(t)));
        snapshot
            .styles
            .iter()
            .for_each(|s| row("styles", &s.id, text(s)));
        snapshot
            .fonts
            .iter()
            .for_each(|f| row("fonts", &f.id, text(f)));
        snapshot
            .notes
            .iter()
            .for_each(|n| row("notes", &n.id, text(n)));
    }
    assert!(snapshot.assets.is_empty(), "asset files are not seeded");
    tx.commit().unwrap();
}

/// Seeds `snapshot` as project `slug` under `root/on`, copies it to `root/off`, and
/// opens memory-on (strict shadow) and memory-off stores. Returns them and the id.
pub(super) fn sqlite_pair(
    root: &Path,
    slug: &str,
    snapshot: &Snapshot,
) -> (Storage, Storage, String) {
    let (on, off) = (root.join("on"), root.join("off"));
    let store = Store::open_with_node_memory(&on, ShadowMode::Off, ReadsMode::Off).unwrap();
    let id = store
        .create_project(slug, "Differential")
        .unwrap()
        .id()
        .to_owned();
    drop(store);
    seed(&on.join(slug).join("db.sql"), snapshot);
    copy(&on.join(slug), &off.join(slug));
    let open = |path: &Path, shadow, reads| -> Storage {
        Store::open_with_node_memory(path, shadow, reads)
            .unwrap()
            .into()
    };
    let memory = open(&on, ShadowMode::Strict, ReadsMode::On);
    (memory, open(&off, ShadowMode::Off, ReadsMode::Off), id)
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}
