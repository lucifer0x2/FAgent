//! `HotProject::begin` against the reference: `Candidate::apply` per operation, link
//! pruning and a full `validate()`. Differences are collected, never panicked on, so one
//! run reports every one of them.
use super::oracle::candidate;
use super::Rng;
use fagent_node_memory::{Begin, HotProject, NodeMemory, Operation, Path, Rows};
use fagent_scene_input::validate::Candidate;
use fagent_scene_input::{Snapshot, StorageError};
use std::collections::BTreeMap;

/// What the reference says about a batch.
enum Expected {
    /// `Candidate::apply` failed on an operation.
    PerOp(StorageError),
    /// Every operation applied but `validate()` failed.
    EndOfBatch(StorageError),
    Accept(Box<Candidate>, Vec<String>),
}

fn expected(snapshot: &Snapshot, ops: &[Operation]) -> Expected {
    match candidate(snapshot, ops) {
        Err(error) => Expected::PerOp(error),
        Ok((candidate, changed)) => match candidate.validate() {
            Err(error) => Expected::EndOfBatch(error),
            Ok(()) => Expected::Accept(Box::new(candidate), changed),
        },
    }
}

/// Counts and differences of one differential run.
#[derive(Default)]
pub struct Totals {
    pub batches: usize,
    pub accepted: usize,
    pub committed: usize,
    pub dropped: usize,
    pub local: usize,
    pub full: usize,
    pub rejected: BTreeMap<String, usize>,
    /// Accept/reject, rows, changed ids, per-op errors: must stay empty.
    pub differences: Vec<String>,
    /// Reference per-operation rejections the engine returned straight from `Txn`
    /// (`MemoryStats::rejected_direct`) and otherwise.
    pub per_op_direct: usize,
    pub per_op_routed: usize,
    /// End-of-batch rejections where code or field differ from `validate()`'s first
    /// error (error order may differ; accept/reject agree).
    pub order: Vec<String>,
}

fn same_error(a: &StorageError, b: &StorageError) -> bool {
    serde_json::to_value(a).unwrap() == serde_json::to_value(b).unwrap()
}

fn brief(e: &StorageError) -> String {
    format!("{} {:?} {:?} {}", e.code, e.field, e.op_index, e.message)
}

fn expected_rows(before: &Snapshot, after: &Candidate, changed: &[String]) -> Rows {
    let mut rows = Rows::default();
    for id in changed {
        let old = before.nodes.iter().find(|n| n.id == *id);
        match (old, after.nodes.get(id)) {
            (b, Some(a)) if b != Some(a) => rows.nodes.push(a.clone()),
            (Some(_), None) => rows.deleted_nodes.push(id.clone()),
            _ => {}
        }
        let old = before.notes.iter().find(|n| n.id == *id);
        match (old, after.notes.get(id)) {
            (b, Some(a)) if b != Some(a) => rows.notes.push(a.clone()),
            (Some(_), None) => rows.deleted_notes.push(id.clone()),
            _ => {}
        }
        let old = before.tokens.iter().find(|t| t.id == *id);
        if let Some(a) = after.tokens.get(id).filter(|a| old != Some(*a)) {
            rows.tokens.push(a.clone());
        }
        let old = before.styles.iter().find(|s| s.id == *id);
        if let Some(a) = after.styles.get(id).filter(|a| old != Some(*a)) {
            rows.styles.push(a.clone());
        }
    }
    rows
}

fn advance(snapshot: &Snapshot, after: &Candidate) -> Snapshot {
    let mut next = snapshot.clone();
    next.revision += 1;
    next.nodes = after.nodes.values().cloned().collect();
    next.tokens = after.tokens.values().cloned().collect();
    next.styles = after.styles.values().cloned().collect();
    next.notes = after.notes.values().cloned().collect();
    next
}

/// Runs one batch through both and compares; commits or drops an accepted batch
/// (`snapshot` then follows the reference).
pub fn round(
    memory: &NodeMemory,
    project: &HotProject,
    snapshot: &mut Snapshot,
    ops: &[Operation],
    rng: &mut Rng,
    totals: &mut Totals,
    at: &str,
) {
    totals.batches += 1;
    let want = expected(snapshot, ops);
    let base = snapshot.revision;
    let direct = memory.stats().rejected_direct;
    let actual = project.begin(base, ops);
    let direct = memory.stats().rejected_direct > direct;
    let mut diffs = Vec::new();
    let mut diff = |what: String| diffs.push(format!("{at}: {what}"));
    match (actual, want) {
        (Ok(Begin::Stale), _) => diff("stale at the tracked revision".into()),
        (Err(a), Expected::PerOp(e)) => {
            match direct {
                true => totals.per_op_direct += 1,
                false => totals.per_op_routed += 1,
            }
            if !same_error(&a, &e) {
                diff(format!(
                    "per-op error\n   engine: {}\n   candidate: {}",
                    brief(&a),
                    brief(&e)
                ));
            }
            *totals
                .rejected
                .entry(format!("op {} {:?}", e.code, e.field))
                .or_default() += 1;
        }
        (Err(a), Expected::EndOfBatch(e)) => {
            if a.op_index.is_some() {
                diff(format!("engine reports an op index: {}", brief(&a)));
            }
            if a.code != e.code || a.field != e.field {
                totals.order.push(format!(
                    "{at}: engine {} vs validate {}",
                    brief(&a),
                    brief(&e)
                ));
            }
            *totals
                .rejected
                .entry(format!("end {} {:?}", e.code, e.field))
                .or_default() += 1;
        }
        (Err(a), Expected::Accept(..)) => diff(format!("only the engine rejects: {}", brief(&a))),
        (Ok(Begin::Ready(_)), Expected::PerOp(e) | Expected::EndOfBatch(e)) => {
            diff(format!("only Candidate rejects: {}", brief(&e)));
        }
        (Ok(Begin::Ready(prepared)), Expected::Accept(after, changed)) => {
            totals.accepted += 1;
            match prepared.path() {
                Path::Local => totals.local += 1,
                Path::Full => totals.full += 1,
            }
            if prepared.changed_ids() != changed.as_slice() {
                diff(format!(
                    "changed ids {:?} vs {:?}",
                    prepared.changed_ids(),
                    changed
                ));
            }
            if prepared.rows() != expected_rows(snapshot, &after, &changed) {
                diff("rows differ".into());
            }
            if prepared.base_revision() != base || prepared.revision() != base + 1 {
                diff("revisions".into());
            }
            for row in &prepared.rows().nodes {
                if !prepared.footprint().contains(&format!("node:{}", row.id)) {
                    diff(format!("footprint misses node {}", row.id));
                }
            }
            if rng.chance(20) {
                totals.dropped += 1;
                drop(prepared);
                if project.view().metadata().revision != base {
                    diff("dropped batch was published".into());
                }
            } else {
                totals.committed += 1;
                prepared.commit();
                *snapshot = advance(snapshot, &after);
                if let Some(d) = view_difference(memory, project, snapshot) {
                    diff(d);
                }
            }
        }
    }
    totals.differences.extend(diffs);
}

/// The committed engine state against the tracked reference snapshot.
fn view_difference(memory: &NodeMemory, project: &HotProject, want: &Snapshot) -> Option<String> {
    let got = memory.to_snapshot(project.id())?;
    let key = |s: &Snapshot| {
        let mut nodes = s.nodes.clone();
        nodes.sort_by(|a, b| a.id.cmp(&b.id));
        (nodes, s.tokens.clone(), s.styles.clone(), s.notes.clone())
    };
    (got.revision != want.revision || key(&got) != key(want))
        .then(|| "committed state differs from the reference".into())
}
