use super::{check_request, make_input};
use crate::jobs::SceneRequest;
use fagent_storage::Snapshot;
use serde_json::json;

fn snapshot() -> Snapshot {
    serde_json::from_value(json!({
        "schemaVersion":4,"projectId":"project","slug":"test","name":"Test","revision":7,
        "nodes":[{"id":"page","parentId":null,"sortKey":0,"kind":"page","props":{}},
            {"id":"frame","parentId":"page","sortKey":0,"kind":"frame",
            "props":{"width":100,"height":100}}],
        "tokens":[],"styles":[],"assets":[],"fonts":[],
        "retention":{"oldestCommitRevision":0,"idempotencyPolicy":"bounded"}
    }))
    .unwrap()
}

fn request(width: f64, height: f64, scale: f64) -> SceneRequest {
    SceneRequest {
        frame_id: "frame".into(),
        revision: 7,
        width,
        height,
        scale,
        theme: "default".into(),
        state: "default".into(),
    }
}

#[test]
fn server_preserves_scaled_budget_boundaries_through_input_assembly() {
    let snapshot = snapshot();
    for (width, height, scale) in [
        (4000.0, 4000.0, 1.0),
        (2000.0, 2000.0, 2.0),
        (1000.0, 1000.0, 4.0),
        (8000.0, 8000.0, 0.5),
        (100_000.0, 160.0, 1.0),
    ] {
        let request = request(width, height, scale);
        check_request(&snapshot, &request).unwrap();
        let input = make_input(&snapshot, &request, "").unwrap();
        assert_eq!(input["viewport"], json!({"width":width,"height":height}));
        let mut over = request;
        over.height += 0.001;
        // The request check names the budget (FB-13); input assembly keeps its own guard.
        let error = check_request(&snapshot, &over).unwrap_err();
        assert_eq!(error.0.code, "PIXEL_BUDGET");
        assert_eq!(error.value()["error"]["maxPixels"], 16_000_000);
        assert!(!error.0.retryable);
        let error = make_input(&snapshot, &over, "").unwrap_err();
        assert_eq!(error.0.code, "INVALID_REQUEST");
        assert_eq!(
            error.0.message,
            "Viewport or scale exceeds the render budget"
        );
    }
}

/// FB-13/FB-49: an over-budget request states the limit, the requested pixels and the largest
/// scale that fits.
#[test]
fn pixel_budget_errors_state_the_limit_and_the_scale_that_fits() {
    let snapshot = snapshot();
    let error = check_request(&snapshot, &request(1440.0, 6000.0, 2.0)).unwrap_err();
    let body = error.value()["error"].clone();
    assert_eq!(body["code"], "PIXEL_BUDGET");
    assert_eq!(body["maxScale"], 1.36, "{body}");
    assert_eq!(body["maxPixels"], 16_000_000);
    assert_eq!(body["requestedPixels"], 34_560_000);
    let error = check_request(&snapshot, &request(1440.0, 12000.0, 1.0)).unwrap_err();
    let body = error.value()["error"].clone();
    assert_eq!(body["requestedPixels"], 17_280_000);
    assert_eq!(body["maxScale"], 0.96);
    let message = body["message"].as_str().unwrap();
    assert!(
        message.contains("17,280,000") && message.contains("16,000,000"),
        "{message}"
    );
    // The returned scale really fits, and one hundredth more does not.
    for (width, height) in [(1440.0, 6000.0), (1440.0, 12000.0), (333.0, 777.0)] {
        let scale = super::super::png::max_scale(width, height);
        assert!(super::super::png::check_pixels(width, height, scale).is_ok());
        let next = scale + 0.01;
        assert!(next > 4.0 || super::super::png::check_pixels(width, height, next).is_err());
    }
}

#[test]
fn revision_errors_name_the_latest_revision() {
    let snapshot = snapshot();
    let mut request = request(100.0, 100.0, 1.0);
    request.revision = 3;
    let body = check_request(&snapshot, &request).unwrap_err().value()["error"].clone();
    assert_eq!(body["code"], "REVISION_UNAVAILABLE");
    assert_eq!(body["currentRevision"], 7);
    assert_eq!(body["field"], "revision");
}

#[test]
fn server_keeps_revision_and_state_validation_order() {
    let snapshot = snapshot();
    let mut request = request(f64::NAN, 1.0, 1.0);
    request.revision = 6;
    assert_eq!(
        check_request(&snapshot, &request).unwrap_err().0.code,
        "REVISION_UNAVAILABLE"
    );
    request.revision = 7;
    request.state = "unknown".into();
    assert_eq!(
        check_request(&snapshot, &request).unwrap_err().0.message,
        "Viewport or scale exceeds the render budget"
    );
    request.width = 1.0;
    assert_eq!(
        check_request(&snapshot, &request).unwrap_err().0.message,
        "Unknown interaction state"
    );
}
