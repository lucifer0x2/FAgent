//! The reference: `Candidate::from_snapshot` + `Candidate::apply` per operation +
//! `prune_links` + a full `validate()`, as storage's database path runs them.
//! Comparisons name the first difference only.
use super::{assert_rebuild_matches, assert_unchanged, observe};
use fagent_node_memory::{Delta, Operation, ProjectState};
use fagent_scene_input::{validate::Candidate, Result, Snapshot, StorageError};
use std::collections::{BTreeMap, BTreeSet};

/// The candidate after the batch and its changed ids, or the first error.
pub fn candidate(before: &Snapshot, ops: &[Operation]) -> Result<(Candidate, Vec<String>)> {
    let mut candidate = Candidate::from_snapshot(before);
    let mut changed = BTreeSet::new();
    for (index, op) in ops.iter().enumerate() {
        candidate.apply(op, &mut changed).map_err(|e| e.at(index))?;
    }
    candidate.prune_links(&mut changed);
    Ok((candidate, changed.into_iter().collect()))
}

fn first<T: PartialEq>(
    name: &str,
    actual: &BTreeMap<String, T>,
    expected: &BTreeMap<String, T>,
) -> Option<String> {
    let ids: BTreeSet<&String> = actual.keys().chain(expected.keys()).collect();
    ids.into_iter()
        .find(|id| actual.get(*id) != expected.get(*id))
        .map(|id| format!("{name} {id} differs"))
}

fn by_id<T: Clone>(items: &[T], id: impl Fn(&T) -> &String) -> BTreeMap<String, T> {
    items.iter().map(|x| (id(x).clone(), x.clone())).collect()
}

/// First difference between the state and the candidate rows, if any.
pub fn state_difference(state: &ProjectState, candidate: &Candidate) -> Option<String> {
    let s = state.to_snapshot();
    first("node", &by_id(&s.nodes, |n| &n.id), &candidate.nodes)
        .or_else(|| first("token", &by_id(&s.tokens, |t| &t.id), &candidate.tokens))
        .or_else(|| first("style", &by_id(&s.styles, |t| &t.id), &candidate.styles))
        .or_else(|| first("note", &by_id(&s.notes, |t| &t.id), &candidate.notes))
        .or_else(|| first("asset", &by_id(&s.assets, |t| &t.id), &candidate.assets))
        .or_else(|| first("font", &by_id(&s.fonts, |t| &t.id), &candidate.fonts))
}

/// First difference between the delta's changed ids and the candidate's.
pub fn changed_difference(delta: &Delta, changed: &[String]) -> Option<String> {
    let ids = delta.changed_ids();
    if ids == changed {
        return None;
    }
    let a: BTreeSet<_> = ids.iter().collect();
    let b: BTreeSet<_> = changed.iter().collect();
    let extra = a.difference(&b).next();
    let missing = b.difference(&a).next();
    Some(format!(
        "changed ids: {} vs {}; extra {extra:?}, missing {missing:?}",
        a.len(),
        b.len()
    ))
}

pub fn brief(e: &StorageError) -> String {
    format!("{} {:?} {:?} {}", e.code, e.field, e.op_index, e.message)
}

pub fn same_error(a: &StorageError, b: &StorageError) -> bool {
    serde_json::to_value(a).unwrap() == serde_json::to_value(b).unwrap()
}

/// What the reference says about a batch.
pub enum Expected {
    /// `Candidate::apply` failed on an operation.
    PerOp(StorageError),
    /// Every operation applied but `validate()` failed.
    EndOfBatch(StorageError),
    Accept(Box<Candidate>, Vec<String>),
}

/// `Candidate::apply` per operation, link pruning, then a full `validate()`.
pub fn expected(snapshot: &Snapshot, ops: &[Operation]) -> Expected {
    match candidate(snapshot, ops) {
        Err(error) => Expected::PerOp(error),
        Ok((candidate, changed)) => match candidate.validate() {
            Err(error) => Expected::EndOfBatch(error),
            Ok(()) => Expected::Accept(Box::new(candidate), changed),
        },
    }
}

/// Outcome counts of [`round`]s, by error code and field for rejections.
#[derive(Default)]
pub struct Tally {
    pub accepted: usize,
    pub rejected: BTreeMap<String, usize>,
    /// End-of-batch rejections whose code or field differ from `validate()`'s first
    /// error (several rules fail; accept/reject agree).
    pub order: Vec<String>,
}

impl Tally {
    pub fn rejected(&self) -> usize {
        self.rejected.values().sum()
    }
}

/// Applies `ops` to the state and to the reference (apply + full validation) and checks
/// they agree; on success also that incremental indexes equal a rebuild, on failure
/// that nothing changed. Per-operation errors must be identical; an end-of-batch error
/// must carry no operation index, and a different code or field is tallied in `order`.
pub fn round(state: &mut ProjectState, ops: &[Operation], tally: &mut Tally, at: &str) {
    let before = observe(state);
    match (state.apply(ops), expected(&before.snapshot, ops)) {
        (Ok(delta), Expected::Accept(candidate, changed)) => {
            if let Some(diff) = state_difference(state, &candidate) {
                panic!("{at}: rows differ: {diff}");
            }
            if let Some(diff) = changed_difference(&delta, &changed) {
                panic!("{at}: {diff}");
            }
            assert_rebuild_matches(state);
            assert_eq!(delta.revision, before.revision + 1);
            tally.accepted += 1;
        }
        (Err(actual), Expected::PerOp(expected)) => {
            assert!(
                same_error(&actual, &expected),
                "{at}: errors differ\n lib: {}\n ref: {}",
                brief(&actual),
                brief(&expected)
            );
            assert_unchanged(state, &before);
            let kind = format!("op {} {:?}", actual.code, actual.field);
            *tally.rejected.entry(kind).or_default() += 1;
        }
        (Err(actual), Expected::EndOfBatch(expected)) => {
            assert_eq!(actual.op_index, None, "{at}: {}", brief(&actual));
            if (&actual.code, &actual.field) != (&expected.code, &expected.field) {
                let line = format!(
                    "{at}: lib {} vs validate {}",
                    brief(&actual),
                    brief(&expected)
                );
                tally.order.push(line);
            }
            assert_unchanged(state, &before);
            let kind = format!("end {} {:?}", expected.code, expected.field);
            *tally.rejected.entry(kind).or_default() += 1;
        }
        (Err(actual), _) => panic!("{at}: lib rejects differently: {}", brief(&actual)),
        (Ok(_), Expected::PerOp(e) | Expected::EndOfBatch(e)) => {
            panic!("{at}: only Candidate rejects: {}", brief(&e))
        }
    }
}
