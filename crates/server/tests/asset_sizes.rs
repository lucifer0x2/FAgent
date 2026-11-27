//! `list_assets` reports the natural size of JPEG and WebP imports, like PNG. The images are
//! synthetic headers built here.
#[allow(dead_code)]
mod support;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::json;
use support::Server;

#[tokio::test]
async fn list_assets_reports_jpeg_and_webp_sizes() {
    let s = Server::start().await;
    let p = s.project().await;
    let jpeg = b"\xff\xd8\xff\xe0\x00\x04ab\xff\xc0\x00\x11\x08\x01\xe0\x02\x80\x03".to_vec();
    let mut webp = b"RIFF\0\0\0\0WEBPVP8X\0\0\0\0\0\0\0\0".to_vec();
    webp.extend([0x0f, 0, 0, 0x07, 0, 0]);
    for (revision, (media, bytes)) in [("image/jpeg", jpeg), ("image/webp", webp)]
        .into_iter()
        .enumerate()
    {
        let args = json!({"projectId":p,"expectedRevision":revision,"idempotencyKey":media.replace('/', "-"),"mediaType":media,"base64":STANDARD.encode(bytes)});
        let imported = s.call("import_asset", args).await;
        assert!(imported["asset"]["id"].is_string(), "{imported}");
    }
    let listed = s.call("list_assets", json!({"projectId":p})).await;
    let mut sizes: Vec<_> = listed["items"]
        .as_array()
        .unwrap_or_else(|| panic!("{listed}"))
        .iter()
        .map(|a| {
            (
                a["mediaType"].clone(),
                a["width"].clone(),
                a["height"].clone(),
            )
        })
        .collect();
    sizes.sort_by_key(|(media, _, _)| media.to_string());
    assert_eq!(
        sizes,
        [
            (json!("image/jpeg"), json!(640), json!(480)),
            (json!("image/webp"), json!(16), json!(8)),
        ]
    );
}
