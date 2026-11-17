//! `import_assets`: several assets in one revision, all or nothing. Replay, `concurrent`
//! history and bundle restore accept the multi-asset commit, and a one-item request is the
//! same canonical request as `import_asset`.
mod common;
use common::*;
use fagent_storage::*;
use serde_json::json;

fn font_bytes() -> Vec<u8> {
    include_bytes!("../../../renderer/fonts/NotoSans.ttf").to_vec()
}
fn png_item() -> ImportAssetItem {
    ImportAssetItem {
        media_type: "image/png".into(),
        bytes: png(),
        font: None,
    }
}
fn font_item(id: &str, weight: u16) -> ImportAssetItem {
    ImportAssetItem {
        media_type: "font/ttf".into(),
        bytes: font_bytes(),
        font: Some(FontRegistration {
            id: id.into(),
            family: "Brand".into(),
            weight,
            style: "normal".into(),
            license: None,
        }),
    }
}
fn many(p: &Project, revision: u64, key: &str, items: Vec<ImportAssetItem>) -> ImportAssetsRequest {
    ImportAssetsRequest {
        project_id: p.id().into(),
        expected_revision: revision,
        idempotency_key: key.into(),
        items,
    }
}

#[test]
fn several_assets_commit_as_one_revision_and_replay() {
    let (_temp, _store, p) = setup();
    // One font file registered twice (a variable font's weights) shares one asset.
    let items = vec![
        png_item(),
        font_item("brand", 400),
        font_item("brand_bold", 700),
    ];
    let imported = p
        .import_assets(many(&p, 0, "assets", items.clone()))
        .unwrap();
    assert_eq!(imported.result.committed_revision, 1);
    assert!(!imported.result.replayed);
    let [image, regular, bold] = &imported.assets[..] else {
        panic!("{:?}", imported.assets)
    };
    assert_eq!(image.media_type, "image/png");
    assert_eq!(regular.id, bold.id);
    // Each changed ID once, in item order.
    let expected = [
        image.id.as_str(),
        regular.id.as_str(),
        "brand",
        "brand_bold",
    ]
    .map(String::from);
    assert_eq!(imported.result.changed_ids, expected);
    let snapshot = p.snapshot().unwrap();
    assert_eq!(snapshot.revision, 1);
    assert_eq!(snapshot.assets.len(), 2);
    assert!(snapshot
        .fonts
        .iter()
        .any(|f| f.id == "brand_bold" && f.weight == 700));
    assert_eq!(p.commits_after(0, 10).unwrap().len(), 1);
    assert_eq!(p.read_asset(&image.id).unwrap().1, png());
    // Same key, same content: the stored result, assets in request order.
    let replay = p
        .import_assets(many(&p, 0, "assets", items.clone()))
        .unwrap();
    assert!(replay.result.replayed);
    assert_eq!(replay.assets, imported.assets);
    // Same key, other order: a different canonical request.
    let mut reordered = items;
    reordered.swap(0, 1);
    let error = p
        .import_assets(many(&p, 0, "assets", reordered))
        .unwrap_err();
    assert_eq!(error.code, "IDEMPOTENCY_KEY_REUSED");
    assert_eq!(p.snapshot().unwrap().revision, 1);
}

#[test]
fn one_bad_item_fails_the_whole_call_and_names_it() {
    let (_temp, _store, p) = setup();
    let not_png = ImportAssetItem {
        media_type: "image/png".into(),
        bytes: b"not a png".to_vec(),
        font: None,
    };
    let big = ImportAssetItem {
        media_type: "image/png".into(),
        bytes: vec![0; MAX_ASSET_BYTES],
        font: None,
    };
    let cases = [
        (vec![png_item(), not_png], "items[1].mediaType"),
        (
            vec![font_item("brand", 400), font_item("brand", 700)],
            "items[1].font",
        ),
        (vec![png_item(); MAX_IMPORT_ITEMS + 1], "items"),
        (Vec::new(), "items"),
        // Seven 10 MiB items: over the 64 MiB total before any item is checked.
        (vec![big; 7], "items"),
    ];
    for (items, field) in cases {
        let error = p.import_assets(many(&p, 0, "bad", items)).unwrap_err();
        assert_eq!(error.code, "VALIDATION_FAILED", "{field}");
        assert_eq!(error.field.as_deref(), Some(field), "{}", error.message);
    }
    let snapshot = p.snapshot().unwrap();
    assert_eq!(snapshot.revision, 0);
    assert!(snapshot.assets.is_empty());
    // A stale revision fails before anything is written, like import_asset.
    let error = p
        .import_assets(many(&p, 3, "stale", vec![png_item()]))
        .unwrap_err();
    assert_eq!(error.code, "REVISION_CONFLICT");
}

#[test]
fn history_concurrency_and_bundle_restore_accept_multi_asset_commits() {
    let (_temp, store, p) = setup();
    // A one-item import_assets replays the import_asset that used the same key.
    p.import_asset(asset_request(&p, 0, "logo")).unwrap();
    let one = p
        .import_assets(many(&p, 0, "logo", vec![png_item()]))
        .unwrap();
    assert!(one.result.replayed);
    p.apply_batch(request(&p, 1, "design", initial())).unwrap();
    p.import_assets(many(
        &p,
        2,
        "fonts",
        vec![png_item(), font_item("brand", 400)],
    ))
    .unwrap();
    // A batch based on revision 2 sees the import as known history that wrote no node keys.
    let update = Operation::UpdateNode {
        id: "text".into(),
        props: json!({"text":"After import"}),
        merge: false,
    };
    let stale = p.apply_batch(request(&p, 2, "late", vec![update])).unwrap();
    assert!(!stale.concurrent_unknown);
    assert_eq!(stale.concurrent, Some(Vec::new()));
    let before = p.snapshot().unwrap();
    let bundle = p.export_bundle().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let restored = Store::open(destination.path())
        .unwrap()
        .restore_bundle("recovered", &bundle.bytes)
        .unwrap();
    assert_eq!(restored.snapshot().unwrap().revision, before.revision);
    assert_eq!(restored.commits_after(0, 10).unwrap().len(), 4);
    let replay = restored
        .import_assets(many(
            &restored,
            2,
            "fonts",
            vec![png_item(), font_item("brand", 400)],
        ))
        .unwrap();
    assert!(replay.result.replayed);
    drop(store);
}

mod postgres_support;

/// The PostgreSQL adapter stores the manifests of a multi-asset import as an array and the
/// single-asset shape for one item, so both replay; a bad item writes nothing.
#[test]
#[ignore = "requires fresh PostgreSQL via scripts/task19-check.sh"]
fn postgres_imports_several_assets_as_one_revision_and_replays() {
    let (_backend, storage) = postgres_support::setup();
    let p = postgres_support::project(&storage, "assets");
    let request = |revision: u64, key: &str, items: Vec<ImportAssetItem>| ImportAssetsRequest {
        project_id: p.id().into(),
        expected_revision: revision,
        idempotency_key: key.into(),
        items,
    };
    let single = ImportAssetRequest {
        project_id: p.id().into(),
        expected_revision: 0,
        idempotency_key: "logo".into(),
        media_type: "image/png".into(),
        bytes: png(),
        font: None,
    };
    let logo = p.import_asset(single).unwrap();
    let one = p
        .import_assets(request(0, "logo", vec![png_item()]))
        .unwrap();
    assert!(one.result.replayed);
    assert_eq!(one.assets, vec![logo.asset]);
    let bad = ImportAssetItem {
        media_type: "image/png".into(),
        bytes: b"not a png".to_vec(),
        font: None,
    };
    let error = p
        .import_assets(request(1, "bad", vec![font_item("brand", 400), bad]))
        .unwrap_err();
    assert_eq!(error.field.as_deref(), Some("items[1].mediaType"));
    assert_eq!(p.snapshot().unwrap().revision, 1);
    let items = vec![
        png_item(),
        font_item("brand", 400),
        font_item("brand_bold", 700),
    ];
    let imported = p.import_assets(request(1, "fonts", items.clone())).unwrap();
    assert_eq!(imported.result.committed_revision, 2);
    assert_eq!(imported.assets.len(), 3);
    let replay = p.import_assets(request(1, "fonts", items)).unwrap();
    assert!(replay.result.replayed);
    assert_eq!(replay.assets, imported.assets);
    let snapshot = p.snapshot().unwrap();
    assert_eq!(snapshot.assets.len(), 2);
    let brand = snapshot.fonts.iter().filter(|f| f.family == "Brand");
    assert_eq!(brand.count(), 2);
    assert_eq!(p.read_font("brand_bold").unwrap().2, font_bytes());
    let commits = p.commits_after(1, 10).unwrap();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].operations.as_array().unwrap().len(), 3);
}
