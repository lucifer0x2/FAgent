//! `instances:"reference"` (default) keeps instance-heavy page/project scans to authored nodes.
use super::{equivalent, library::library_fixture};
use fagent_storage::read_scope::ScopedSnapshot;
use serde_json::{json, Value};

fn ids(value: &Value) -> Vec<&str> {
    value["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect()
}

#[test]
fn reference_skips_virtual_children_and_expand_includes_them() {
    let (pages, library) = (8, 10);
    let (_root, _storage, snapshot) = library_fixture(pages, library);
    let scoped = ScopedSnapshot::validate(&snapshot).unwrap();
    let authored = snapshot.nodes.len();
    let find = |extra: Value| {
        let mut args = json!({"projectId":snapshot.project_id,"limit":200});
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        equivalent(&scoped, args)
    };
    for default in [json!({}), json!({"instances":"reference"})] {
        let mut args = json!({"kinds":["rectangle"]});
        args.as_object_mut()
            .unwrap()
            .extend(default.as_object().unwrap().clone());
        let found = find(args);
        assert_eq!(found["totalMatches"], library, "{found}");
        assert_eq!(found["scannedNodes"], authored);
        assert_eq!(found["skippedInstanceMatches"], pages * library);
        assert!(ids(&found).iter().all(|id| !id.contains('/')));
    }
    // An instance still matches as itself, with no skipped descendants to report.
    let cards = find(json!({"kinds":["instance"]}));
    assert_eq!(cards["totalMatches"], pages);
    assert!(cards.get("skippedInstanceMatches").is_none(), "{cards}");
    let expanded = find(json!({"kinds":["rectangle"],"instances":"expand"}));
    assert_eq!(expanded["totalMatches"], library + pages * library);
    assert_eq!(expanded["scannedNodes"], authored + pages * library);
    assert_eq!(expanded["criteria"]["instances"], "expand");
    assert!(expanded.get("skippedInstanceMatches").is_none());
    assert!(ids(&expanded).contains(&"card3/part7"));
    let reference = serde_json::to_vec(&find(json!({"kinds":["rectangle"]}))).unwrap();
    let expand = serde_json::to_vec(&expanded).unwrap();
    assert!(
        reference.len() * 4 < expand.len(),
        "{} vs {}",
        reference.len(),
        expand.len()
    );
    // A frame holding only an instance: nothing authored matches, the skip is reported.
    let frame = find(json!({"frameId":"frame1","kinds":["rectangle"]}));
    assert_eq!(frame["totalMatches"], 0);
    assert_eq!(frame["skippedInstanceMatches"], library);
}

#[test]
fn explicit_virtual_addresses_still_resolve_in_reference_mode() {
    let (_root, _storage, snapshot) = library_fixture(4, 5);
    let scoped = ScopedSnapshot::validate(&snapshot).unwrap();
    let find = |extra: Value| {
        let mut args = json!({"projectId":snapshot.project_id});
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        equivalent(&scoped, args)
    };
    let by_id = find(json!({"ids":["card2/part4","part0"]}));
    assert_eq!(ids(&by_id), ["part0", "card2/part4"], "{by_id}");
    assert_eq!(by_id["matches"][1]["parentId"], "card2");
    let children = find(json!({"parentId":"card3"}));
    assert_eq!(children["totalMatches"], 5);
    assert!(ids(&children).iter().all(|id| id.starts_with("card3/")));
    // A scope on the instance itself, or inside it, searches its virtual subtree.
    let on = find(json!({"nodeId":"card1","kinds":["rectangle"]}));
    assert_eq!(on["totalMatches"], 5);
    assert!(on.get("skippedInstanceMatches").is_none());
    let inside = find(json!({"nodeId":"card1/part2","nameContains":"part"}));
    assert_eq!(ids(&inside), ["card1/part2"]);
}

#[test]
fn reference_pages_cover_exact_totals_and_cursors_bind_the_mode() {
    let (pages, library) = (6, 7);
    let (_root, _storage, snapshot) = library_fixture(pages, library);
    let scoped = ScopedSnapshot::validate(&snapshot).unwrap();
    let mut args = json!({"projectId":snapshot.project_id,"nameContains":"needle","limit":3});
    let mut seen = Vec::new();
    let first = loop {
        let page = equivalent(&scoped, args.clone());
        // The master, its parts, and per page: page, frame and card.
        assert_eq!(page["totalMatches"], 1 + library + 3 * pages, "{page}");
        assert_eq!(page["skippedInstanceMatches"], pages * library);
        seen.extend(ids(&page).into_iter().map(str::to_owned));
        if page["nextCursor"].is_null() {
            break args;
        }
        args["cursor"] = page["nextCursor"].clone();
    };
    assert_eq!(seen.len(), 1 + library + 3 * pages);
    assert!(seen.iter().all(|id| !id.contains('/')));
    // A reference cursor is not valid for an expand read of the same criteria.
    let mut expand = first;
    expand["instances"] = json!("expand");
    let error = equivalent(&scoped, expand);
    assert_eq!(error["error"]["code"], "INVALID_CURSOR", "{error}");
}
