//! Task28 I3: last-writer-wins reporting. A batch based on an older revision still
//! commits; its result lists the properties that commits after that base wrote and
//! this batch overwrote, computed from durable commit history so every engine and
//! commit path reports the same pairs.
mod writes;

use crate::{BatchResult, ConcurrentWrite};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
pub(crate) use writes::{batch_writes, Writes};

/// Pairs listed in a result; `concurrentTotal` counts them all.
pub(crate) const MAX_REPORTED: usize = 20;
/// History beyond this many commits (or the 8 MiB encoded cap of the history
/// query) after the base is reported as `concurrentUnknown`.
pub(crate) const MAX_COMMITS: usize = 1000;

/// What the request asked to be checked against, after request validation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Base {
    pub expected: Option<u64>,
    pub strict: bool,
}

impl Base {
    pub fn of(request: &crate::BatchRequest) -> crate::Result<Self> {
        if request.strict && request.expected_revision.is_none() {
            return Err(crate::StorageError::validation(
                "expectedRevision",
                "strict:true requires expectedRevision from the authored read it is based on",
            ));
        }
        Ok(Self {
            expected: request.expected_revision,
            strict: request.strict,
        })
    }

    /// Rejections that do not need history: strict exact matching without pages,
    /// and a base ahead of the project in every mode.
    pub fn check(&self, current: u64, scoped: bool) -> crate::Result<()> {
        let ahead = self.expected.is_some_and(|base| base > current);
        let stale = self.strict && !scoped && self.expected != Some(current);
        if ahead || stale {
            return Err(crate::StorageError::conflict(current));
        }
        Ok(())
    }

    /// The base whose later history this commit needs, if any.
    pub fn history_after(&self, current: u64, scoped: bool) -> Option<u64> {
        let base = self.expected?;
        (base < current || (self.strict && scoped)).then_some(base)
    }
}

/// Fills the result's `concurrent` fields. `commits` are those after `base` as the
/// history query returns them (`Value::Null` where the size cap cut it off).
pub(crate) fn report(
    result: &mut BatchResult,
    base: u64,
    current: u64,
    commits: &[(u64, Value)],
    ours: &Writes,
) {
    if base >= current {
        return;
    }
    let Some(theirs) = history_writes(base, current, commits) else {
        result.concurrent_unknown = true;
        return;
    };
    let mut hits = BTreeSet::new();
    for (id, key) in ours {
        let Some(written) = theirs.get(id.as_str()) else {
            continue;
        };
        let whole = matches!(key.as_str(), "created" | "deleted");
        for (their_key, revision) in written {
            let theirs_whole = matches!(their_key.as_str(), "created" | "deleted");
            if whole || theirs_whole || their_key == key {
                hits.insert((id.clone(), their_key.clone(), *revision));
            }
        }
    }
    result.concurrent_total = Some(hits.len());
    result.concurrent = Some(
        hits.into_iter()
            .take(MAX_REPORTED)
            .map(|(node_id, key, revision)| ConcurrentWrite {
                node_id,
                key,
                revision,
            })
            .collect(),
    );
}

/// For each id, the keys commits in `(base, current]` wrote with the latest
/// revision that wrote each. `None` when that history is incomplete.
fn history_writes(
    base: u64,
    current: u64,
    commits: &[(u64, Value)],
) -> Option<BTreeMap<String, BTreeMap<String, u64>>> {
    if commits.len() > MAX_COMMITS || commits.len() as u64 != current - base {
        return None;
    }
    let mut out: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    for (offset, (revision, value)) in commits.iter().enumerate() {
        if *revision != base + 1 + offset as u64 || value.is_null() {
            return None;
        }
        let operations = crate::scoped::operations(value.clone());
        for (id, key) in writes::operation_writes(&operations)? {
            out.entry(id).or_default().insert(key, *revision);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests;
