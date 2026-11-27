use crate::{
    error::{ApiError, Result},
    jobs::SceneRequest,
};
use fagent_scene_input::{check_viewport, SceneOptions};
use fagent_storage::Snapshot;
use serde_json::Value;

pub fn make_input(snapshot: &Snapshot, request: &SceneRequest, base: &str) -> Result<Value> {
    let options = SceneOptions {
        frame_id: request.frame_id.clone(),
        width: request.width,
        height: request.height,
        theme: request.theme.clone(),
        state: request.state.clone(),
    };
    let mut input = fagent_scene_input::make_input_with_scale(snapshot, &options, request.scale)?;
    attach_urls(&mut input, &snapshot.project_id, base);
    Ok(input)
}

pub(super) fn attach_urls(input: &mut Value, project: &str, base: &str) {
    for font in input["fonts"].as_array_mut().unwrap() {
        font["url"] = format!(
            "{base}/api/v1/projects/{}/fonts/{}/download",
            project,
            font["id"].as_str().unwrap()
        )
        .into();
    }
    for asset in input["assets"].as_array_mut().unwrap() {
        asset["url"] = format!(
            "{base}/api/v1/projects/{}/assets/{}",
            project,
            asset["id"].as_str().unwrap()
        )
        .into();
    }
}
/// Rejects a request outside the render budget or the supported interaction state.
pub fn check_request(snapshot: &Snapshot, request: &SceneRequest) -> Result<()> {
    if snapshot.revision != request.revision {
        return Err(crate::reads::revision_unavailable(
            request.revision,
            snapshot.revision,
        ));
    }
    let (width, height, scale) = (request.width, request.height, request.scale);
    // Only the area is over budget: say by how much and which scale fits. This is
    // check_viewport's area rule, so nothing it accepts is rejected here.
    let sane = [width, height]
        .iter()
        .all(|v| v.is_finite() && *v > 0.0 && *v <= 100_000.0)
        && scale.is_finite()
        && scale > 0.0
        && scale <= 4.0;
    if sane && width * height * scale * scale > super::png::MAX_PIXELS {
        return Err(super::png::pixel_budget(width, height, scale));
    }
    check_viewport(width, height, scale)?;
    if !matches!(
        request.state.as_str(),
        "default" | "hover" | "pressed" | "focus" | "disabled"
    ) {
        return Err(ApiError::invalid("Unknown interaction state"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
