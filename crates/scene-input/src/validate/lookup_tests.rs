//! The `Candidate` oracle answers and the Candidate-free entry points.
use super::super::{check_node, check_style, token_value, validate_tokens};
use super::*;
use crate::{Operation, Snapshot};
use serde_json::json;
use std::collections::BTreeSet;

fn candidate() -> Candidate {
    let snapshot: Snapshot = serde_json::from_value(json!({
        "schemaVersion":4,"projectId":"p","slug":"p","name":"p","revision":1,
        "retention":{"oldestCommitRevision":0,"idempotencyPolicy":"bounded"},
        "tokens":[
            {"id":"t2","name":"b","type":"color","value":{"aliasId":"t1"}},
            {"id":"t1","name":"a","type":"color","value":{"default":"#000000","themes":{"dark":"#ffffff"}}}
        ],
        "styles":[],"assets":[],"fonts":[],
        "notes":[
            {"id":"n2","nodeId":"f","text":"b"},
            {"id":"n1","nodeId":"f","text":"a"},
            {"id":"n3","nodeId":"g","text":"c"}
        ],
        "nodes":[
            {"id":"p","parentId":null,"sortKey":0,"kind":"page","props":{}},
            {"id":"f","parentId":"p","sortKey":1,"kind":"frame","props":{"width":400,"height":400,
                "links":[{"to":"m","on":"click","action":"navigate"}]}},
            {"id":"m","parentId":"p","sortKey":0,"kind":"frame","props":{"width":40,"height":40,"component":{"name":"M"}}},
            {"id":"j","parentId":"f","sortKey":0,"kind":"instance","props":{"componentId":"m"}},
            {"id":"i","parentId":"f","sortKey":0,"kind":"instance","props":{"componentId":"m"}},
            {"id":"g","parentId":"f","sortKey":1,"kind":"group","props":{"width":100,"height":100,
                "links":[{"to":"m","on":"click","action":"navigate"}]}}
        ]
    }))
    .unwrap();
    Candidate::from_snapshot(&snapshot)
}

#[test]
fn candidate_oracle_orders() {
    let c = candidate();
    let lookup: &dyn Lookup = &c;
    let ids = |nodes: Vec<&Node>| nodes.iter().map(|n| n.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(lookup.children("p")), ["m", "f"]);
    assert_eq!(ids(lookup.children("f")), ["i", "j", "g"]);
    assert_eq!(lookup.component_users("m"), ["i", "j"]);
    assert_eq!(lookup.incoming_links("m"), ["f", "g"]);
    let notes: Vec<_> = lookup.notes_on("f").iter().map(|n| n.id.as_str()).collect();
    assert_eq!(notes, ["n1", "n2"]);
    let tokens: Vec<_> = lookup.tokens().map(|t| t.id.as_str()).collect();
    assert_eq!(tokens, ["t1", "t2"]);
    assert!(lookup.theme_declared("dark") && !lookup.theme_declared("light"));
    assert_eq!((lookup.node_count(), lookup.token_count()), (6, 2));
    assert_eq!(ids(lookup.pages()), ["p"]);
    assert_eq!(lookup.referencing("link:m"), ["f", "g"]);
    assert_eq!(lookup.referencing("component:m"), ["i", "j"]);
    assert!(lookup.referencing("theme:dark").is_empty());
    assert_eq!(lookup.aliasers("t1"), ["t2"]);
    assert!(lookup.aliasers("t2").is_empty());
    assert_eq!((lookup.named_themes(), lookup.note_count()), (1, 3));
}

#[test]
fn reference_keys_add_the_own_theme() {
    let props = json!({"theme":"dark","fill":{"tokenId":"t1"},"overrides":{"x":{"theme":"dim"}}});
    let keys: Vec<String> = reference_keys(&props).into_iter().collect();
    assert_eq!(keys, ["theme:dark", "token:t1"]);
}

#[test]
fn free_entry_points_match_candidate() {
    let c = candidate();
    assert_eq!(validate_tokens(&c).unwrap(), c.validate_tokens().unwrap());
    assert_eq!(
        token_value(&c, "t2", "dark").unwrap(),
        c.token_value("t2", "dark").unwrap()
    );
    let orphan: Node = serde_json::from_value(json!(
        {"id":"x","parentId":"missing","sortKey":0,"kind":"rectangle","props":{}}
    ))
    .unwrap();
    let error = check_node(&c, &orphan).unwrap_err();
    let mut applied = c.clone();
    let op: Operation = serde_json::from_value(json!(
        {"op":"create_node","id":"x","kind":"rectangle","parentId":"missing","props":{}}
    ))
    .unwrap();
    let same = applied.apply(&op, &mut BTreeSet::new()).unwrap_err();
    assert_eq!(format!("{error:?}"), format!("{same:?}"));
    let style: Style = serde_json::from_value(json!(
        {"id":"s","name":"s","kind":"fill","value":{"tokenId":"t1"}}
    ))
    .unwrap();
    assert!(check_style(&c, &style).is_ok());
    assert!(check_node(&c, Lookup::node(&c, "f").unwrap()).is_ok());
}
