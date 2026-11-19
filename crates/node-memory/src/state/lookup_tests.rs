//! The store's `Lookup` answers every question exactly like the `Candidate` oracle.
use super::ProjectState;
use fagent_scene_input::validate::{Candidate, Lookup};
use fagent_scene_input::{Operation, Snapshot};
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

/// The questions end-of-batch validation asks, compared over every id and key the
/// project names.
fn assert_same_indexes(state: &ProjectState) {
    let oracle = Candidate::from_snapshot(&state.to_snapshot());
    let (store, oracle): (&dyn Lookup, &dyn Lookup) = (&state.store, &oracle);
    let pages = |l: &dyn Lookup| ids(l.pages(), |n| &n.id);
    assert_eq!(pages(store), pages(oracle));
    assert_eq!(store.note_count(), oracle.note_count());
    assert_eq!(store.named_themes(), oracle.named_themes());
    let mut keys = std::collections::BTreeSet::new();
    for node in oracle.tokens().map(|t| t.id.as_str()).chain(["missing"]) {
        assert_eq!(
            store.aliasers(node),
            oracle.aliasers(node),
            "aliasers {node}"
        );
    }
    for theme in ["default", "dark", "dim", "night", "x", "missing"] {
        assert_eq!(store.theme_declared(theme), oracle.theme_declared(theme));
        keys.insert(format!("theme:{theme}"));
    }
    for id in ["t1", "t2", "t3", "t4", "font", "m", "f", "missing"] {
        for kind in ["token", "style", "font", "asset", "component", "link"] {
            keys.insert(format!("{kind}:{id}"));
        }
    }
    for key in keys {
        assert_eq!(store.referencing(&key), oracle.referencing(&key), "{key}");
    }
}

#[test]
fn store_indexes_match_candidate_across_writes() {
    let mut snapshot = snapshot();
    // Writes run the end-of-batch rules, which recheck the instances of the edited
    // master `m`: drop their stored (invalid) links.
    for node in snapshot.nodes.iter_mut().filter(|n| n.id.starts_with('i')) {
        node.props.as_object_mut().unwrap().remove("links");
    }
    let tokens = json!([
        {"id":"t1","name":"T1","type":"color","value":{"default":"#000000","themes":{"dark":"#ffffff","dim":"#777777"}}},
        {"id":"t2","name":"T2","type":"color","value":{"default":{"aliasId":"t1"},"themes":{"dark":{"aliasId":"t1"}}}},
        {"id":"t3","name":"T3","type":"number","value":4}
    ]);
    snapshot.tokens = serde_json::from_value(tokens).unwrap();
    let page = json!({"id":"q","kind":"page","parentId":null,"sortKey":-1,"props":{}});
    snapshot.nodes.push(serde_json::from_value(page).unwrap());
    let mut state = ProjectState::from_snapshot(snapshot).expect("snapshot loads");
    assert_same_indexes(&state);
    let token = |id: &str, kind: &str, value| {
        let token = json!({"id":id,"name":id,"type":kind,"value":value});
        Operation::UpsertToken {
            token: serde_json::from_value(token).unwrap(),
        }
    };
    let update = |id: &str, props| Operation::UpdateNode {
        id: id.into(),
        props,
        merge: true,
    };
    state
        .apply(&[
            token("t2", "color", json!("#101010")),
            token(
                "t4",
                "color",
                json!({"default":{"aliasId":"t1"},"themes":{"night":{"aliasId":"t1"}}}),
            ),
            token(
                "t1",
                "color",
                json!({"default":"#000000","themes":{"dark":"#ffffff"}}),
            ),
            update("f", json!({"theme":"dark","fill":{"tokenId":"t1"}})),
            update("m", json!({"theme":"night"})),
        ])
        .expect("batch applies");
    assert_same_indexes(&state);
    let store: &dyn Lookup = &state.store;
    assert_eq!(store.aliasers("t1"), ["t4"]);
    assert_eq!(store.referencing("theme:night"), ["m"]);
    assert_eq!(store.named_themes(), 2);
    state
        .apply(&[
            update("f", json!({"theme":null})),
            Operation::DeleteSubtree { id: "q".into() },
        ])
        .expect("batch applies");
    assert_same_indexes(&state);
}

/// Two stored notes sharing an id collapse to one: the anchor index follows the note
/// kept, so no node lists an id whose note is anchored elsewhere.
#[test]
fn duplicate_note_ids_index_only_the_note_kept() {
    let mut snapshot = snapshot();
    snapshot.notes = serde_json::from_value(json!([
        {"id":"n","nodeId":"a","text":"x"},
        {"id":"n","nodeId":"b","text":"y"}
    ]))
    .unwrap();
    let state = ProjectState::from_snapshot(snapshot).unwrap();
    for node in ["a", "b"] {
        assert!(state.notes_on(node).all(|n| n.node_id == node), "{node}");
    }
    let listed = state.notes_on("a").count() + state.notes_on("b").count();
    assert_eq!(listed, 1);
}
