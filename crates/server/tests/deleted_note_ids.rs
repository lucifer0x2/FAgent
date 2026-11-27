//! Deleting a node deletes the notes anchored to it, and the result lists them in
//! `deletedNoteIds`, in a dry run and on commit.
#[allow(dead_code)]
mod support;
use serde_json::json;
use support::Server;

#[tokio::test]
async fn deleting_a_node_reports_the_notes_it_removes() {
    let s = Server::start().await;
    let p = s.project().await;
    let source = "page#home\n  frame#card 200x100 col\n    rect#label 20x20\n@note card \"Card note\" id=n_card\n@note label \"Label note\" id=n_label\n@note home \"Page note\" id=n_page\n";
    let created = s
        .call(
            "apply_batch",
            json!({"projectId":p,"expectedRevision":0,"idempotencyKey":"seed","source":source}),
        )
        .await;
    assert_eq!(created["committedRevision"], 1, "{created}");
    assert!(created.get("deletedNoteIds").is_none(), "{created}");
    let delete = |dry: bool| json!({"projectId":p,"expectedRevision":1,"idempotencyKey":"del","source":"@del card\n","dryRun":dry});
    let preview = s.call("apply_batch", delete(true)).await;
    assert_eq!(preview["dryRun"], true, "{preview}");
    assert_eq!(preview["deletedNoteIds"], json!(["n_card", "n_label"]));
    let committed = s.call("apply_batch", delete(false)).await;
    assert_eq!(committed["committedRevision"], 2, "{committed}");
    assert_eq!(committed["deletedNoteIds"], json!(["n_card", "n_label"]));
    let changed = committed["changedIds"].as_array().unwrap();
    assert!(changed.contains(&json!("n_card")) && changed.contains(&json!("n_label")));
    // The replay returns the stored result, the list included.
    let replay = s.call("apply_batch", delete(false)).await;
    assert_eq!(replay["replayed"], true, "{replay}");
    assert_eq!(replay["deletedNoteIds"], committed["deletedNoteIds"]);
}
