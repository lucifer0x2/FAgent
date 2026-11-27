//! JPEG and WebP imports record their natural size like PNG, on both engines; an asset stored
//! without a size (imported before sizes were recorded) reads with the size from its binary.
//! Every image here is a synthetic header built in the test.
mod postgres_support;
use fagent_storage::*;
use std::sync::Arc;

/// Baseline JPEG: SOI, a 4-byte APP0 segment, then SOF0 with the given size.
fn jpeg(width: u16, height: u16) -> Vec<u8> {
    let mut bytes = b"\xff\xd8\xff\xe0\x00\x04ab\xff\xc0\x00\x11\x08".to_vec();
    bytes.extend(height.to_be_bytes());
    bytes.extend(width.to_be_bytes());
    bytes.extend([3, 1, 0x22, 0, 2, 0x11, 1, 3, 0x11, 1, 0xff, 0xd9]);
    bytes
}

fn webp_lossy(width: u16, height: u16) -> Vec<u8> {
    let mut bytes = b"RIFF\0\0\0\0WEBPVP8 \0\0\0\0\0\0\0\x9d\x01\x2a".to_vec();
    bytes.extend(width.to_le_bytes());
    bytes.extend(height.to_le_bytes());
    bytes
}

fn webp_lossless(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"RIFF\0\0\0\0WEBPVP8L\0\0\0\0\x2f".to_vec();
    bytes.extend(((width - 1) | ((height - 1) << 14)).to_le_bytes());
    bytes
}

fn webp_extended(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"RIFF\0\0\0\0WEBPVP8X\0\0\0\0\0\0\0\0".to_vec();
    bytes.extend(&(width - 1).to_le_bytes()[..3]);
    bytes.extend(&(height - 1).to_le_bytes()[..3]);
    bytes
}

fn import(
    p: &ProjectHandle,
    revision: u64,
    key: &str,
    media: &str,
    bytes: Vec<u8>,
) -> Result<Asset> {
    p.import_asset(ImportAssetRequest {
        project_id: p.id().into(),
        expected_revision: revision,
        idempotency_key: key.into(),
        media_type: media.into(),
        bytes,
        font: None,
    })
    .map(|imported| imported.asset)
}

/// Imports one image per format and checks the recorded and listed sizes.
fn check_imports(p: &ProjectHandle) {
    let images = [
        ("image/jpeg", jpeg(640, 480), (640, 480)),
        ("image/webp", webp_lossy(800, 600), (800, 600)),
        ("image/webp", webp_lossless(100, 50), (100, 50)),
        ("image/webp", webp_extended(4000, 3000), (4000, 3000)),
    ];
    for (revision, (media, bytes, (w, h))) in images.into_iter().enumerate() {
        let key = format!("image{revision}");
        let asset = import(p, revision as u64, &key, media, bytes).unwrap();
        assert_eq!((asset.width, asset.height), (Some(w), Some(h)), "{media}");
        let listed = p.snapshot().unwrap();
        let listed = listed.assets.iter().find(|a| a.id == asset.id).unwrap();
        assert_eq!((listed.width, listed.height), (Some(w), Some(h)));
    }
    // The 40 Mpx cap now applies to JPEG and WebP whose header gives a size.
    for (media, bytes) in [
        ("image/jpeg", jpeg(8000, 6000)),
        ("image/webp", webp_extended(8000, 6000)),
    ] {
        let error = import(p, 4, "huge", media, bytes).unwrap_err();
        assert_eq!(error.field.as_deref(), Some("bytes"), "{media}");
        assert!(
            error.message.contains("40000000 pixels"),
            "{}",
            error.message
        );
    }
    // A signed header without a readable size stays accepted, as before.
    let sizeless = import(p, 4, "sizeless", "image/jpeg", b"\xff\xd8\xff\xd9".to_vec()).unwrap();
    assert_eq!((sizeless.width, sizeless.height), (None, None));
}

/// The asset as stored before sizes were recorded: the same record without a size.
fn sizeless(asset: &Asset) -> String {
    let mut stored = asset.clone();
    (stored.width, stored.height) = (None, None);
    serde_json::to_string(&stored).unwrap()
}

fn check_filled(p: &ProjectHandle, asset: &Asset) {
    let snapshot = p.snapshot().unwrap();
    let listed = snapshot.assets.iter().find(|a| a.id == asset.id).unwrap();
    assert_eq!((listed.width, listed.height), (asset.width, asset.height));
    assert_eq!(p.read_asset(&asset.id).unwrap().0, *asset);
}

#[test]
fn sqlite_records_jpeg_and_webp_sizes() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::from(Store::open(temp.path()).unwrap());
    check_imports(&storage.create_project("fixture", "Fixture").unwrap());
}

#[test]
fn sqlite_fills_a_stored_null_size_on_read() {
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::from(Store::open(temp.path()).unwrap());
    let p = storage.create_project("fixture", "Fixture").unwrap();
    let asset = import(&p, 0, "photo", "image/webp", webp_lossy(320, 240)).unwrap();
    let id = p.id().to_owned();
    drop((p, storage));
    let db = rusqlite::Connection::open(temp.path().join("fixture/db.sql")).unwrap();
    db.execute(
        "UPDATE assets SET data_json=?1 WHERE id=?2",
        [sizeless(&asset), asset.id.clone()],
    )
    .unwrap();
    drop(db);
    let storage = Storage::from(Store::open(temp.path()).unwrap());
    check_filled(&storage.open_project(&id).unwrap(), &asset);
}

#[test]
#[ignore = "requires fresh PostgreSQL via scripts/task19-check.sh"]
fn postgres_records_jpeg_and_webp_sizes_and_fills_null_ones() {
    let (_, storage) = postgres_support::setup();
    check_imports(&postgres_support::project(&storage, "sizes"));
    let p = postgres_support::project(&storage, "legacy");
    let asset = import(&p, 0, "photo", "image/jpeg", jpeg(1024, 768)).unwrap();
    postgres_support::sql()
        .execute(
            "UPDATE fagent_runtime_v1.entities SET data=$1 WHERE project_id=$2 AND kind='asset' AND id=$3",
            &[&sizeless(&asset), &p.id(), &asset.id],
        )
        .unwrap();
    // A backend of its own starts with cold node memory, so it reads the stored rows.
    let cold = Storage::from_shared(Arc::new(postgres_support::backend()));
    check_filled(&cold.open_project(p.id()).unwrap(), &asset);
}
