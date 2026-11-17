//! Small strict-page differential oracle, including untrusted durable history.
use super::*;
use crate::{scoped, BatchRequest, Store};
use serde_json::{json, Value};

fn fixture() -> Snapshot {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(temp.path()).unwrap();
    let project = store.create_project("guard", "Guard").unwrap();
    project.apply_batch(serde_json::from_value(json!({"projectId":project.id(),"idempotencyKey":"seed","operations":[
        {"op":"create_node","id":"p","kind":"page","props":{}},
        {"op":"create_node","id":"q","kind":"page","props":{}},
        {"op":"create_node","id":"f","kind":"frame","parentId":"p","props":{"width":100,"height":100}},
        {"op":"create_node","id":"g","kind":"frame","parentId":"q","props":{"width":100,"height":100}},
        {"op":"upsert_token","token":{"id":"old","name":"Old","type":"color","value":"#112233"}},
        {"op":"upsert_token","token":{"id":"new","name":"New","type":"color","value":"#334455"}},
        {"op":"create_node","id":"r","kind":"rectangle","parentId":"f","props":{"width":10,"height":10,"fill":{"tokenId":"old"}}}
    ]})).unwrap()).unwrap();
    let mut snapshot = project.snapshot().unwrap();
    snapshot.revision = 2;
    snapshot
}

fn history(before: &Snapshot, scopes: &[&str]) -> Value {
    let request = json!({"projectId":before.project_id,"expectedRevision":1,"idempotencyKey":"other","operations":[{"op":"update_node","id":"g","props":{"name":"Other"}}]});
    scoped::envelope_with_scopes(
        scopes.iter().map(|s| s.to_string()).collect(),
        request.clone(),
        request["operations"].clone(),
    )
    .unwrap()
}

fn compare(
    before: &Snapshot,
    operations: Value,
    pages: &[&str],
    history: &[(u64, Value)],
) -> crate::Result<()> {
    let request: BatchRequest = serde_json::from_value(json!({"projectId":before.project_id,"expectedRevision":1,"strict":true,"idempotencyKey":"ours","operations":operations})).unwrap();
    let (candidate, changed, mut work) =
        crate::batch::validated_candidate(before, &request.operations).unwrap();
    let scope = scoped::BatchScope::new(before, &candidate, &changed).unwrap();
    let mut after = before.clone();
    after.nodes = candidate.nodes.values().cloned().collect();
    let local = LocalBatch {
        before: Arc::new(ReadView::new(before.clone())),
        after: Arc::new(ReadView::new(after)),
        preimages: before.clone(),
        rows: MemoryRows::default(),
        changed,
        footprint: scope.footprint.clone(),
    };
    let pages: Vec<_> = pages.iter().map(|p| p.to_string()).collect();
    let expected = scoped::check(
        before, &candidate, &request, &pages, history, &scope, &mut work,
    );
    let actual = local.check_pages(&request, &pages, history);
    let public = |result: &crate::Result<()>| {
        result
            .as_ref()
            .err()
            .map(|e| serde_json::to_value(e).unwrap())
    };
    assert_eq!(public(&actual), public(&expected));
    actual
}

#[test]
fn i5_strict_history_and_both_dependency_endpoints_match_legacy() {
    let before = fixture();
    let edit = json!([{"op":"update_node","id":"r","props":{"fill":{"tokenId":"new"}}}]);
    compare(
        &before,
        edit.clone(),
        &["p"],
        &[(2, history(&before, &["page:q"]))],
    )
    .unwrap();
    // Removing an old reference and attaching a new one must protect both.
    for scopes in [
        vec!["token:old"],
        vec!["token:new"],
        vec!["page:p"],
        vec!["*"],
    ] {
        let error = compare(
            &before,
            edit.clone(),
            &["p"],
            &[(2, history(&before, &scopes))],
        )
        .unwrap_err();
        assert_eq!(error.code, "REVISION_CONFLICT");
    }
    let valid = history(&before, &["page:q"]);
    let mut version = valid.clone();
    version["scopeVersion"] = json!(2);
    let mut tampered = valid.clone();
    tampered["scopes"] = json!([]);
    let mut absent = valid.clone();
    absent.as_object_mut().unwrap().remove("scopes");
    for commits in [
        vec![],
        vec![(3, valid.clone())],
        vec![(2, version)],
        vec![(2, tampered)],
        vec![(2, absent)],
        vec![(2, Value::Null)],
        vec![(2, valid); 1001],
    ] {
        assert_eq!(
            compare(&before, edit.clone(), &["p"], &commits)
                .unwrap_err()
                .code,
            "REVISION_CONFLICT"
        );
    }
    for pages in [vec![], vec!["missing"], vec!["p", "p"]] {
        compare(&before, edit.clone(), &pages, &[]).unwrap_err();
    }
    // Cross-page source/destination and footprint checks still precede history.
    compare(
        &before,
        json!([{"op":"reparent_node","id":"r","parentId":"g"}]),
        &["p"],
        &[],
    )
    .unwrap_err();
}

#[test]
fn i5_edit_context_wire_summary_and_truncation_match_legacy() {
    let mut before = fixture();
    // Exercise the exact 100-key wire summary as well as normal/error responses.
    let prototype = before.nodes.iter().find(|n| n.id == "r").unwrap().clone();
    let token = before
        .tokens
        .iter()
        .find(|t| t.id == "old")
        .unwrap()
        .clone();
    for i in 0..110 {
        let mut node = prototype.clone();
        node.id = format!("extra{i}");
        node.sort_key = i + 1;
        let mut token = token.clone();
        token.id = format!("token{i}");
        node.props["fill"] = json!({"tokenId":token.id});
        before.tokens.push(token);
        before.nodes.push(node);
    }
    let shared = crate::SharedSnapshot::memory(Arc::new(ReadView::new(before.clone())));
    for pages in [
        vec!["p".into()],
        vec!["q".into()],
        vec!["missing".into()],
        vec![],
    ] {
        let bytes = |value: crate::Result<Value>| {
            value
                .map(|v| serde_json::to_vec(&v).unwrap())
                .map_err(|e| serde_json::to_value(e).unwrap())
        };
        assert_eq!(
            bytes(shared.edit_context(pages.clone())),
            bytes(scoped::edit_context(before.clone(), pages))
        );
    }
    let response = shared.edit_context(vec!["p".into()]).unwrap();
    assert_eq!(response["dependenciesTruncated"], true);
    assert_eq!(response["dependencies"].as_array().unwrap().len(), 100);
}
