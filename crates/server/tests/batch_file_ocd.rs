//! `apply_batch_file` with a `.ocd` file: the text is compiled exactly like inline `source`
//! (same result, errors at the file's line and column, `autoIds`), replay hashes the compiled
//! content rather than the filename, and the inbox guards still hold.
#[allow(dead_code)]
mod support;
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
use support::Server;

/// The published OCD sample, as `tests/ocd_source.rs` reads it.
fn sample() -> String {
    let text = include_str!("../skills/ocd.md");
    let (_, after) = text.split_once("<!-- ocd-sample -->").unwrap();
    let (_, after) = after.split_once("```ocd\n").unwrap();
    after.split_once("```").unwrap().0.to_owned()
}
fn inbox(s: &Server) -> PathBuf {
    s.root.path().join("test-project/inbox")
}
fn file(project: &str, revision: u64, key: &str, path: &str) -> Value {
    json!({"projectId":project,"expectedRevision":revision,"idempotencyKey":key,"path":path})
}
fn inline(project: &str, revision: u64, key: &str, source: &str) -> Value {
    json!({"projectId":project,"expectedRevision":revision,"idempotencyKey":key,"source":source})
}

#[tokio::test]
async fn ocd_file_commits_like_inline_source_and_replays_by_content() {
    let s = Server::start().await;
    let p = s.project().await;
    fs::write(inbox(&s).join("login.ocd"), sample()).unwrap();
    let mut preview = file(&p, 0, "login", "login.ocd");
    preview["dryRun"] = json!(true);
    let dry = s.call("apply_batch_file", preview).await;
    assert_eq!(dry["dryRun"], true, "{dry}");
    assert_eq!(dry["baseRevision"], 0);
    let committed = s
        .call("apply_batch_file", file(&p, 0, "login", "login.ocd"))
        .await;
    assert_eq!(committed["committedRevision"], 1, "{committed}");
    assert_eq!(committed["changedIds"], dry["changedIds"]);
    assert!(committed["autoIds"]
        .as_object()
        .is_some_and(|ids| !ids.is_empty()));
    // The same source inline, under the same key, is the same canonical request.
    let same = s
        .call("apply_batch", inline(&p, 0, "login", &sample()))
        .await;
    assert_eq!(same["replayed"], true, "{same}");
    assert_eq!(same["autoIds"], committed["autoIds"]);
    // The filename is not hashed: another name with the same content replays.
    fs::write(inbox(&s).join("copy.ocd"), sample()).unwrap();
    let copy = s
        .call("apply_batch_file", file(&p, 0, "login", "copy.ocd"))
        .await;
    assert_eq!(copy["replayed"], true, "{copy}");
    // Changed content under a used key is rejected; nothing is written.
    fs::write(
        inbox(&s).join("login.ocd"),
        sample().replace("Sign in", "Login"),
    )
    .unwrap();
    let reused = s
        .call("apply_batch_file", file(&p, 0, "login", "login.ocd"))
        .await;
    assert_eq!(
        reused["error"]["code"], "IDEMPOTENCY_KEY_REUSED",
        "{reused}"
    );
    // An edit file with pageIds follows the scoped path like inline source.
    fs::write(inbox(&s).join("edit.ocd"), "@set title text=\"Hello\"\n").unwrap();
    let mut edit = file(&p, 1, "edit", "edit.ocd");
    edit["pageIds"] = json!(["login"]);
    let edited = s.call("apply_batch_file", edit).await;
    assert_eq!(edited["committedRevision"], 2, "{edited}");
    assert_eq!(edited["editGuard"]["pageIds"], json!(["login"]));
}

#[tokio::test]
async fn ocd_file_errors_point_at_the_file_line_and_column() {
    let s = Server::start().await;
    let p = s.project().await;
    let cases = [
        // Parse error on line 3, at the token after the parent ID.
        "page#pg\n  frame#f 100x100\n@in f row gap=8 fill=#ff0000\n  rect#r 1x1\n",
        // Compiles, then fails in storage on the operation from line 3.
        "page#pg\n  frame#f 100x100\n@set missing w=10\n",
    ];
    for (index, source) in cases.into_iter().enumerate() {
        let name = format!("bad{index}.ocd");
        fs::write(inbox(&s).join(&name), source).unwrap();
        let mut dry = file(&p, 0, "bad", &name);
        dry["dryRun"] = json!(true);
        let from_file = s.call("apply_batch_file", dry).await["error"].clone();
        assert_eq!(from_file["line"], 3, "{source}: {from_file}");
        let mut same = inline(&p, 0, "bad", source);
        same["dryRun"] = json!(true);
        let from_inline = s.call("apply_batch", same).await["error"].clone();
        assert_eq!(from_file, from_inline, "{source}");
    }
    let parse = s
        .call("apply_batch_file", file(&p, 0, "bad", "bad0.ocd"))
        .await;
    assert_eq!(parse["error"]["code"], "SOURCE_PARSE_FAILED", "{parse}");
    assert_eq!(parse["error"]["column"], 7);
    // An empty file is a source error at 1:1, not a JSON error.
    fs::write(inbox(&s).join("empty.ocd"), "").unwrap();
    let empty = s
        .call("apply_batch_file", file(&p, 0, "empty", "empty.ocd"))
        .await;
    assert_eq!(empty["error"]["code"], "SOURCE_PARSE_FAILED", "{empty}");
    let outline = s.call("get_outline", json!({"projectId":p})).await;
    assert_eq!(outline["revision"], 0);
}

#[tokio::test]
async fn ocd_paths_keep_the_inbox_guards() {
    let s = Server::start().await;
    let p = s.project().await;
    let outside = s.root.path().join("outside.ocd");
    fs::write(&outside, sample()).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, inbox(&s).join("link.ocd")).unwrap();
    let absolute = fs::canonicalize(&outside).unwrap();
    for path in [
        "../outside.ocd",
        "link.ocd",
        "./x.ocd",
        absolute.to_str().unwrap(),
    ] {
        let result = s.call("apply_batch_file", file(&p, 0, "bad", path)).await;
        assert_eq!(result["error"]["code"], "PATH_REJECTED", "{path}: {result}");
        assert!(!result.to_string().contains(s.root.path().to_str().unwrap()));
    }
    let missing = s
        .call("apply_batch_file", file(&p, 0, "bad", "missing.ocd"))
        .await;
    assert_eq!(missing["error"]["code"], "FILE_NOT_FOUND", "{missing}");
    // JSON keeps today's behaviour for every other name, OCD text included.
    fs::write(inbox(&s).join("source.txt"), sample()).unwrap();
    let json = s
        .call("apply_batch_file", file(&p, 0, "bad", "source.txt"))
        .await;
    assert_eq!(json["error"]["field"], "operations", "{json}");
}

/// SQLite reports its data root's inbox as `inboxDir`; an absolute path under it applies.
#[tokio::test]
async fn sqlite_reports_inbox_dir_and_accepts_absolute_paths_under_it() {
    let s = Server::start().await;
    let p = s.project().await;
    let listed = s.call("list_projects", json!({})).await;
    let dir = listed["projects"][0]["inboxDir"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(PathBuf::from(&dir), fs::canonicalize(inbox(&s)).unwrap());
    fs::write(inbox(&s).join("login.ocd"), sample()).unwrap();
    let absolute = format!("{dir}/login.ocd");
    let committed = s
        .call("apply_batch_file", file(&p, 0, "abs", &absolute))
        .await;
    assert_eq!(committed["committedRevision"], 1, "{committed}");
}
