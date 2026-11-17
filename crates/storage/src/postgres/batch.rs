use super::*;
use crate::{BatchRequest, BatchResult, DryRunResult};
use memory_write::Source;
use request::{canonical, history_after};
use write_through::Committed;

mod request;

pub(super) enum Outcome {
    /// A new commit carries its rows for the post-commit cache fill; a replay
    /// carries none.
    Commit(BatchResult, Option<Box<Committed>>),
    Preview(DryRunResult),
}

pub(super) fn apply(
    session: &mut Session,
    key: &str,
    request: BatchRequest,
    dry: bool,
    indexes: &crate::revision_indexes::IndexCache,
) -> Result<Outcome> {
    apply_mode(session, key, request, dry, None, indexes, None)
}

pub(super) fn apply_mode(
    session: &mut Session,
    key: &str,
    request: BatchRequest,
    dry: bool,
    pages: Option<Vec<String>>,
    indexes: &crate::revision_indexes::IndexCache,
    shadow: Option<&crate::shadow::Shadow>,
) -> Result<Outcome> {
    if dry {
        return preview(session, key, request, pages, indexes).map(Outcome::Preview);
    }
    let mut tx = session.client.transaction().map_err(sql)?;
    let mut db = Db::new(&mut tx, &mut session.statements);
    let project = projects::lookup(&mut db, key, true)?;
    let (value, hash) = canonical(&project, &request, pages.as_deref())?;
    let base = crate::concurrent::Base::of(&request)?;
    // Read committed: this separate query sees any writer that released our row lock.
    if let Some((mut result, _)) = transaction::replay(
        &mut db,
        &project.meta.project_id,
        &request.idempotency_key,
        &hash,
    )? {
        result.replayed = true;
        return Ok(Outcome::Commit(result, None));
    }
    base.check(project.meta.revision, pages.is_some())?;
    let identity = (project.generation, project.namespace.clone());
    let cache_identity = snapshot::identity(
        project.meta.project_id.clone(),
        project.meta.schema_version,
        project.meta.revision,
        identity.0,
        &identity.1,
    );
    // Supported HOT operations use persistent endpoint dependency closures with
    // the unchanged strict/history oracle. Unsupported operations fall through.
    {
        if let Some(memory) = shadow.and_then(|s| s.memory_handle(&cache_identity)) {
            if let Some((staged, local)) = memory.prepare(&request.operations)? {
                let history = match base.history_after(cache_identity.revision, pages.is_some()) {
                    Some(after) => history_after(&mut db, &cache_identity.project_id, after)?,
                    None => vec![],
                };
                if let (Some(pages), true) = (&pages, base.strict) {
                    local.check_pages(&request, pages, &history)?;
                }
                let result = local.result(&request, &history, &value["operations"]);
                let history = crate::scoped::envelope_with_scopes(
                    local.footprint,
                    value.clone(),
                    value["operations"].clone(),
                )?;
                let rows = memory_write::written(&local.preimages, &local.rows)?;
                let persisted = transaction::persist(
                    &mut db,
                    &local.preimages,
                    rows,
                    &request.idempotency_key,
                    &hash,
                    &history,
                    &result,
                    None,
                )
                .and_then(|written| tx.commit().map_err(sql).map(|_| written));
                let written = match persisted {
                    Ok(written) => {
                        staged.commit();
                        written
                    }
                    Err(error) => {
                        drop(staged);
                        if let Some(shadow) = shadow {
                            shadow.write_aborted();
                            shadow.evict(&cache_identity.project_id);
                        }
                        return Err(error);
                    }
                };
                let committed = Committed::from_memory(
                    local.before,
                    written,
                    result.committed_revision,
                    identity,
                );
                return Ok(Outcome::Commit(result, Some(Box::new(committed))));
            }
        }
    }
    let (before, source) = memory_write::load(&mut db, project, shadow, &cache_identity)?;
    let old_indexes = indexes.for_snapshot(&before, Some(cache_identity.clone()));
    let (candidate, changed_ids, mut work, next_indexes) =
        crate::batch::validated_candidate_indexed(
            &before,
            &request.operations,
            old_indexes.clone(),
        )?;
    let scope = crate::scoped::BatchScope::indexed(
        &before,
        &candidate,
        &changed_ids,
        old_indexes,
        next_indexes.clone(),
    )?;
    let history = match base.history_after(before.revision, pages.is_some()) {
        Some(after) => history_after(&mut db, &before.project_id, after)?,
        None => vec![],
    };
    if let (Some(pages), true) = (&pages, base.strict) {
        crate::bench_phase::time("scoped.check", || {
            crate::scoped::check(
                &before, &candidate, &request, pages, &history, &scope, &mut work,
            )
        })?;
    }
    drop(work);
    let mut result = BatchResult {
        project_id: before.project_id.clone(),
        committed_revision: before.revision + 1,
        changed_ids,
        render_status: "pending".into(),
        ..Default::default()
    };
    let operations = &value["operations"];
    crate::concurrent::fill(
        &mut result,
        base,
        before.revision,
        &history,
        operations,
        &candidate,
    );
    let history = crate::bench_phase::time("history.envelope", || {
        crate::scoped::envelope_with_scopes(
            scope.footprint,
            value.clone(),
            value["operations"].clone(),
        )
    })?;
    let key = &request.idempotency_key;
    let (written, binaries) = match source {
        Source::Database { binaries, stored } => {
            let rows = crate::bench_phase::time("persist.delta", || stored.delta(&candidate))?;
            drop((stored, candidate));
            let _phase = crate::bench_phase::Span::new("persist");
            let written =
                transaction::persist(&mut db, &before, rows, key, &hash, &history, &result, None)?;
            tx.commit().map_err(sql)?;
            (written, Some(binaries))
        }
        Source::Memory(memory) => {
            let Some(staged) = memory.stage(&request.operations) else {
                return Err(memory_write::fallback());
            };
            memory.verify(&staged, &candidate, &result.changed_ids);
            drop(candidate);
            let rows = crate::bench_phase::time("persist.delta", || {
                memory_write::written(&before, &crate::shadow::memory_rows(&staged))
            })?;
            let _phase = crate::bench_phase::Span::new("persist");
            let persisted =
                transaction::persist(&mut db, &before, rows, key, &hash, &history, &result, None)
                    .and_then(|written| tx.commit().map_err(sql).map(|_| written));
            match persisted {
                // The project row lock is released; the next writer waits on memory's
                // write lock until this commit is kept.
                Ok(written) => {
                    drop(staged.commit());
                    (written, None)
                }
                Err(error) => {
                    drop(staged);
                    if let Some(shadow) = shadow {
                        shadow.write_aborted();
                        shadow.evict(&before.project_id);
                    }
                    return Err(error);
                }
            }
        }
    };
    let revision = result.committed_revision;
    let mut next_identity = cache_identity;
    next_identity.revision = revision;
    indexes.publish(next_identity, next_indexes);
    let committed = Committed::new(before, binaries, written, revision, identity);
    Ok(Outcome::Commit(result, Some(Box::new(committed))))
}

/// A preview validates against one repeatable-read snapshot without the
/// project row lock, so it neither waits for nor blocks writers. It applies
/// the commit path's request checks in the same order. A reused key with a
/// different canonical request still fails; a matching key still previews.
fn preview(
    session: &mut Session,
    key: &str,
    request: BatchRequest,
    pages: Option<Vec<String>>,
    indexes: &crate::revision_indexes::IndexCache,
) -> Result<DryRunResult> {
    let (mut tx, statements) = session.read_only()?;
    let mut db = Db::new(&mut tx, statements);
    let project = projects::lookup(&mut db, key, false)?;
    let (_, hash) = canonical(&project, &request, pages.as_deref())?;
    let base = crate::concurrent::Base::of(&request)?;
    transaction::replay(
        &mut db,
        &project.meta.project_id,
        &request.idempotency_key,
        &hash,
    )?;
    base.check(project.meta.revision, pages.is_some())?;
    let identity = snapshot::identity(
        project.meta.project_id.clone(),
        project.meta.schema_version,
        project.meta.revision,
        project.generation,
        &project.namespace,
    );
    let (before, _) = snapshot::entities_with_binaries(&mut db, project)?;
    let old_indexes = indexes.for_snapshot(&before, Some(identity));
    let (candidate, changed_ids, mut work, next_indexes) =
        crate::batch::validated_candidate_indexed(
            &before,
            &request.operations,
            old_indexes.clone(),
        )?;
    if let (Some(pages), true) = (&pages, base.strict) {
        let scope = crate::scoped::BatchScope::indexed(
            &before,
            &candidate,
            &changed_ids,
            old_indexes,
            next_indexes,
        )?;
        let history = match base.history_after(before.revision, true) {
            Some(after) => history_after(&mut db, &before.project_id, after)?,
            None => vec![],
        };
        crate::scoped::check(
            &before, &candidate, &request, pages, &history, &scope, &mut work,
        )?;
    }
    Ok(DryRunResult {
        project_id: before.project_id,
        dry_run: true,
        base_revision: before.revision,
        changed_ids,
        diagnostics: vec![],
    })
}
