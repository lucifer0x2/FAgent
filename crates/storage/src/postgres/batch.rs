//! PostgreSQL commits and previews, staged in `NodeMemory`: the project row lock
//! (`FOR UPDATE`), the idempotency replay and base check, the resident project (or
//! a cold load of the stored rows), the engine's validated batch, page guard and
//! history, then the changed rows are persisted, the transaction commits and the
//! batch is published.
use super::*;
use crate::memory::{
    guard,
    stage::{self, begin},
    Memory,
};
use crate::{BatchRequest, BatchResult, DryRunResult, SnapshotCacheIdentity, StorageError};
use ::postgres::GenericClient;
use fagent_node_memory::{Begin, HotProject};
use request::{canonical, history_after};

mod request;

pub(super) enum Outcome {
    Commit(BatchResult),
    Preview(DryRunResult),
}

pub(super) fn apply_mode(
    session: &mut Session,
    key: &str,
    request: BatchRequest,
    dry: bool,
    pages: Option<Vec<String>>,
    memory: &Memory,
) -> Result<Outcome> {
    if dry {
        return preview(session, key, &request, pages.as_deref(), memory).map(Outcome::Preview);
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
        return Ok(Outcome::Commit(result));
    }
    let revision = project.meta.revision;
    base.check(revision, pages.is_some())?;
    let identity = identity(&project);
    let id = identity.project_id.clone();
    let first = for_write(&mut db, &project, &identity, memory)?;
    let mut retry = None;
    let (hot, prepared) = memory.begin(
        &id,
        (&first, &mut retry),
        (revision, &request.operations),
        || for_write(&mut db, &project, &identity, memory),
    )?;
    let history = match base.history_after(revision, pages.is_some()) {
        Some(after) => history_after(&mut db, &id, after)?,
        None => vec![],
    };
    if let (Some(pages), true) = (&pages, base.strict) {
        crate::bench_phase::time("scoped.check", || {
            guard::check(&prepared, &request, pages, &history)
        })?;
    }
    let (result, envelope) = stage::outcome(&prepared, &request, &history, &value)?;
    let persisted = {
        let _phase = crate::bench_phase::Span::new("persist");
        let rows = prepared.rows();
        memory_write::written(&rows)
            .and_then(|written| {
                let (key, envelope) = (&request.idempotency_key, &envelope);
                transaction::persist(
                    &mut db, &id, revision, written, key, &hash, envelope, &result, None,
                )
            })
            .and_then(|()| tx.commit().map_err(sql))
    };
    if let Err(error) = persisted {
        drop(prepared);
        memory.write_failed(&id);
        return Err(error);
    }
    prepared.commit();
    let mut next = identity;
    next.revision = result.committed_revision;
    memory.committed(hot, Some(&next));
    Ok(Outcome::Commit(result))
}

/// A preview runs the commit path's request checks in the same order on one
/// repeatable-read snapshot, without the project row lock, then stages the batch in
/// memory and rolls it back. It waits for a batch another worker has staged; when
/// memory has published past this snapshot's revision, the preview runs once more on
/// a fresh snapshot, and if memory is past that one too, on a private load that
/// leaves the newer resident copy alone. A reused key with a different canonical
/// request still fails; a matching key still previews.
fn preview(
    session: &mut Session,
    key: &str,
    request: &BatchRequest,
    pages: Option<&[String]>,
    memory: &Memory,
) -> Result<DryRunResult> {
    if let Some(result) = preview_once(session, key, request, pages, memory, false)? {
        return Ok(result);
    }
    let result = preview_once(session, key, request, pages, memory, true)?;
    result.ok_or_else(|| StorageError::new("STORAGE_FAILED", "Project changed during preview"))
}

/// `None` when memory is past this snapshot's revision (not on the `last` try).
fn preview_once(
    session: &mut Session,
    key: &str,
    request: &BatchRequest,
    pages: Option<&[String]>,
    memory: &Memory,
    last: bool,
) -> Result<Option<DryRunResult>> {
    let (mut tx, statements) = session.read_only()?;
    let mut db = Db::new(&mut tx, statements);
    let project = projects::lookup(&mut db, key, false)?;
    let (_, hash) = canonical(&project, request, pages)?;
    let base = crate::concurrent::Base::of(request)?;
    transaction::replay(
        &mut db,
        &project.meta.project_id,
        &request.idempotency_key,
        &hash,
    )?;
    let revision = project.meta.revision;
    base.check(revision, pages.is_some())?;
    let identity = identity(&project);
    let ops = &request.operations;
    let resident = {
        let load = || load_rows(&mut db, &project);
        memory.for_preview(&identity.project_id, &identity, revision, load)?
    };
    let staged = match &resident {
        Some(hot) => begin(hot, revision, ops)?,
        None => Begin::Stale,
    };
    let detached: HotProject;
    let prepared = match staged {
        Begin::Ready(prepared) => prepared,
        Begin::Stale if !last => return Ok(None),
        Begin::Stale => {
            detached = memory.detached(|| load_rows(&mut db, &project))?;
            match begin(&detached, revision, ops)? {
                Begin::Ready(prepared) => prepared,
                Begin::Stale => return Err(StorageError::conflict(revision)),
            }
        }
    };
    if let (Some(pages), true) = (pages, base.strict) {
        let history = match base.history_after(revision, true) {
            Some(after) => history_after(&mut db, &identity.project_id, after)?,
            None => vec![],
        };
        guard::check(&prepared, request, pages, &history)?;
    }
    Ok(Some(DryRunResult {
        project_id: identity.project_id,
        dry_run: true,
        base_revision: revision,
        changed_ids: prepared.changed_ids().to_vec(),
        diagnostics: vec![],
    }))
}

fn identity(project: &projects::ProjectRow) -> SnapshotCacheIdentity {
    snapshot::identity(
        project.meta.project_id.clone(),
        project.meta.schema_version,
        project.meta.revision,
        project.generation,
        &project.namespace,
    )
}

/// The stored rows with asset readiness, as reads see them.
fn load_rows<C: GenericClient>(
    db: &mut Db<'_, C>,
    project: &projects::ProjectRow,
) -> Result<crate::Snapshot> {
    crate::bench_phase::time("load", || {
        snapshot::entities(&mut *db, project.clone(), true)
    })
}

/// The resident project at the row's revision, or a cold load of the stored rows.
fn for_write<C: GenericClient>(
    db: &mut Db<'_, C>,
    project: &projects::ProjectRow,
    identity: &SnapshotCacheIdentity,
    memory: &Memory,
) -> Result<HotProject> {
    let load = || load_rows(&mut *db, project);
    let id = &identity.project_id;
    memory.for_write(id, Some(identity), identity.revision, load)
}
