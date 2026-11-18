//! The store's `Lookup` answers every question exactly like the `Candidate` oracle.
use super::ProjectState;
use fagent_scene_input::validate::{Candidate, Lookup};
use fagent_scene_input::Snapshot;
use serde_json::json;

fn snapshot() -> Snapshot {
    let node = |id: &str, kind: &str, parent: Option<&str>, sort: i64, props| json!({"id":id,"kind":kind,"parentId":parent,"sortKey":sort,"props":props});
    let text = json!({"text":"T","fontId":"font","fontSize":12,"lineHeight":16});
    serde_json::from_value(json!({
        "schemaVersion":4,"projectId":"proj","slug":"proj","name":"Proj","revision":1,
        "nodes":[
            node("p", "page", None, 0, json!({})),
            node("m", "frame", Some("p"), 0, json!({"width":10,"height":10,"component":{"name":"M"}})),
            node("f", "frame", Some("p"), 1, json!({"width":10,"height":10})),
            node("b", "text", Some("f"), 1, text.clone()),
            node("a", "text", Some("f"), 1, text),
            node("i2", "instance", Some("f"), 0, json!({"componentId":"m","links":[{"to":"m"}]})),
            node("i1", "instance", Some("f"), 2, json!({"componentId":"m","links":[{"to":"m"}]})),
        ],
        "tokens":[
            {"id":"t2","name":"T2","type":"color","value":{"value":"#000000","themes":{"dark":"#ffffff"}}},
            {"id":"t1","name":"T1","type":"number","value":{"value":4}}
        ],
        "styles":[],"assets":[],
        "fonts":[{"id":"font","family":"Noto Sans","weight":400,"style":"normal",
            "assetId":null,"bundledId":"noto","contentHash":"h","license":"OFL",
            "status":"ready"}],
        "notes":[
            {"id":"n2","nodeId":"a","text":"x"},
            {"id":"n1","nodeId":"a","text":"y"}
        ],
        "retention":{"oldestCommitRevision":0,"idempotencyPolicy":"bounded"}
    }))
    .expect("test snapshot")
}

fn ids<'a, T: 'a>(items: impl IntoIterator<Item = &'a T>, id: impl Fn(&T) -> &str) -> Vec<String> {
    items.into_iter().map(|x| id(x).to_owned()).collect()
}

#[test]
fn store_lookup_matches_candidate() {
    let snapshot = snapshot();
    let oracle = Candidate::from_snapshot(&snapshot);
    let state = ProjectState::from_snapshot(snapshot).expect("snapshot loads");
    let (store, oracle): (&dyn Lookup, &dyn Lookup) = (&state.store, &oracle);
    assert_eq!(store.node_count(), oracle.node_count());
    assert_eq!(store.token_count(), oracle.token_count());
    assert_eq!(
        ids(store.tokens(), |t| &t.id),
        ids(oracle.tokens(), |t| &t.id)
    );
    assert_eq!(
        ids(store.fonts(), |f| &f.id),
        ids(oracle.fonts(), |f| &f.id)
    );
    for name in ["dark", "light"] {
        assert_eq!(store.theme_declared(name), oracle.theme_declared(name));
    }
    for id in ["p", "m", "f", "a", "b", "i1", "i2", "missing"] {
        assert_eq!(store.node(id), oracle.node(id), "node {id}");
        let kids = |l: &dyn Lookup| ids(l.children(id), |n| &n.id);
        assert_eq!(kids(store), kids(oracle), "children of {id}");
        let notes = |l: &dyn Lookup| ids(l.notes_on(id), |n| &n.id);
        assert_eq!(notes(store), notes(oracle), "notes on {id}");
        assert_eq!(store.component_users(id), oracle.component_users(id));
        assert_eq!(store.incoming_links(id), oracle.incoming_links(id));
    }
    for id in ["t1", "t2", "font", "n1"] {
        assert_eq!(store.token(id), oracle.token(id));
        assert_eq!(store.font(id), oracle.font(id));
        assert_eq!(store.note(id), oracle.note(id));
    }
}
