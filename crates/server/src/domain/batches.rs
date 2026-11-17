//! File and inline mutation requests converge before candidate validation/commit.
//! A staged `.ocd` file becomes inline `source`; any other staged file is JSON operations.
use super::*;
use fagent_storage::{BatchFile, BatchFileRequest, BatchRequest};

pub(super) async fn run(state: &AppState, name: &str, mut args: Value) -> Result<(Value, bool)> {
    // The public schema checked this boolean; remove it before decoding the legacy
    // BatchRequest so Rust callers can keep constructing that struct unchanged.
    let dry_run = args.as_object_mut().unwrap().remove("dryRun") == Some(json!(true));
    let page_ids = args.as_object_mut().unwrap().remove("pageIds");
    if name == "apply_batch_file" {
        let file: BatchFileRequest = serde_json::from_value(args)?;
        let to_read = file.clone();
        let content = state
            .project(file.project_id.clone(), move |p| {
                p.read_batch_file(&to_read)
            })
            .await?;
        args = json!({"projectId":file.project_id,"idempotencyKey":file.idempotency_key});
        if let Some(expected) = file.expected_revision {
            args["expectedRevision"] = json!(expected);
        }
        if file.strict {
            args["strict"] = json!(true);
        }
        match content {
            BatchFile::Operations(operations) => {
                args["operations"] = operations;
                // Validate original JSON, before deserializing optional operation fields.
                crate::schemas::validate("apply_batch", &args)?;
            }
            // Compiled below exactly like inline source: lines and columns are the
            // file's, and the idempotency hash covers the compiled operations, never
            // the filename.
            BatchFile::Source(source) => args["source"] = json!(source),
        }
    }
    // OCD source compiles to the same operations (ahead of any JSON operations sent with it),
    // then follows the inline path unchanged; storage errors naming an operation also get its
    // source line and column, and results list generated IDs.
    let mut positions = None;
    if let Some(source) = args.as_object_mut().unwrap().remove("source") {
        let source = source.as_str().unwrap_or_default();
        positions = Some(crate::ocd::compile_into(
            &mut args,
            source,
            page_ids.as_ref(),
        )?);
    }
    let locate = |error: crate::ApiError| match &positions {
        Some(positions) => crate::ocd::Positions::locate(positions, error),
        None => error,
    };
    let report = |result: &mut Value| {
        if let Some(positions) = &positions {
            positions.report(result);
        }
    };
    let notes_only = fagent_storage::is_note_batch(&args["operations"]);
    if let Some(pages) = &page_ids {
        args["pageIds"] = pages.clone();
    }
    if page_ids.is_some() {
        let request: fagent_storage::ScopedBatchRequest = serde_json::from_value(args)?;
        let (project, expected, pages, strict) = (
            request.project_id.clone(),
            request.expected_revision.unwrap_or_default(),
            request.page_ids.clone(),
            request.strict,
        );
        let result = state
            .project(project.clone(), move |p| {
                if dry_run {
                    serde_json::to_value(p.dry_run_scoped_batch(request)?).map_err(Into::into)
                } else {
                    serde_json::to_value(p.apply_scoped_batch(request)?).map_err(Into::into)
                }
            })
            .await
            .map_err(locate);
        let mut result = conflict::checked(state, &project, expected, result).await?;
        let revision = if dry_run {
            result["baseRevision"].clone()
        } else {
            result["committedRevision"].clone()
        };
        // `strict` says whether the page guards were enforced: without it a scoped batch
        // is last-writer-wins, and a client replaying the guard must keep the mode.
        result["editGuard"] = json!({"projectId":project,"expectedRevision":revision,
            "pageIds":pages,"strict":strict});
        report(&mut result);
        return Ok((result, notes_only));
    }
    let request: BatchRequest = serde_json::from_value(args)?;
    let (project, expected) = (
        request.project_id.clone(),
        request.expected_revision.unwrap_or_default(),
    );
    let result = state
        .project(project.clone(), move |p| {
            if dry_run {
                serde_json::to_value(p.dry_run_batch(request)?).map_err(Into::into)
            } else {
                serde_json::to_value(p.apply_batch(request)?).map_err(Into::into)
            }
        })
        .await
        .map_err(locate);
    let mut result = conflict::checked(state, &project, expected, result).await?;
    report(&mut result);
    Ok((result, notes_only))
}
