use super::*;
use crate::{BatchRequest, BatchResult, DryRunResult};
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
    memory: Option<&crate::memory::Memory>,
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
    let (before, binaries, stored) =
        crate::bench_phase::time("load", || snapshot::entities_for_write(&mut db, project))?;
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
    let rows = crate::bench_phase::time("persist.delta", || stored.delta(&candidate))?;
    drop((stored, candidate));
    let written = {
        let _phase = crate::bench_phase::Span::new("persist");
        let written =
            transaction::persist(&mut db, &before, rows, key, &hash, &history, &result, None)?;
        tx.commit().map_err(sql)?;
        written
    };
    // Commits do not stage in memory yet: the next read reloads the new revision.
    if let Some(memory) = memory {
        memory.evict(&before.project_id);
    }
    let revision = result.committed_revision;
    let mut next_identity = cache_identity;
    next_identity.revision = revision;
    indexes.publish(next_identity, next_indexes);
    let committed = Committed::new(before, Some(binaries), written, revision, identity);
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
