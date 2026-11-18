//! The reference: `Candidate::from_snapshot` + `Candidate::apply` per operation +
//! `prune_links`, as `validated_candidate_indexed` in storage runs them before
//! validation. Comparisons name the first difference only.
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

fn brief(e: &StorageError) -> String {
    format!("{} {:?} {:?} {}", e.code, e.field, e.op_index, e.message)
}

/// Outcome counts of [`round`]s, by error code and field for rejections.
#[derive(Default)]
pub struct Tally {
    pub accepted: usize,
    pub rejected: BTreeMap<String, usize>,
}

impl Tally {
    pub fn rejected(&self) -> usize {
        self.rejected.values().sum()
    }
}

/// Applies `ops` to the state and to the reference and checks they agree; on success
/// also that incremental indexes equal a rebuild, on failure that nothing changed.
pub fn round(state: &mut ProjectState, ops: &[Operation], tally: &mut Tally, at: &str) {
    let before = observe(state);
    let expected = candidate(&before.snapshot, ops);
    match (state.apply(ops), expected) {
        (Ok(delta), Ok((candidate, changed))) => {
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
        (Err(actual), Err(expected)) => {
            let same =
                serde_json::to_value(&actual).unwrap() == serde_json::to_value(&expected).unwrap();
            assert!(
                same,
                "{at}: errors differ\n lib: {}\n ref: {}",
                brief(&actual),
                brief(&expected)
            );
            assert_unchanged(state, &before);
            let kind = format!("{} {:?}", actual.code, actual.field);
            *tally.rejected.entry(kind).or_default() += 1;
        }
        (Err(actual), Ok(_)) => panic!("{at}: only the lib rejects: {}", brief(&actual)),
        (Ok(_), Err(expected)) => panic!("{at}: only Candidate rejects: {}", brief(&expected)),
    }
}
