//! Task28 I3: commits staged in the in-memory project.
//!
//! An engine asks [`super::Shadow::memory_base`] for the project at the database's
//! current revision and incarnation for the conservative reference fallback.
//! I4's `memory_handle`/`prepare` stages first and validates from delta-indexed
//! revision views. The fallback validates the full snapshot, then stages in the lib and persists
//! [`MemoryRows`] from the staged state in its own transaction and commits the
//! staged batch after the database commit; dropping it rolls memory back exactly.
use super::stats::{bump, COUNTERS};
use crate::validate::Candidate;
use crate::{Node, Note, Operation, Snapshot, SnapshotCacheIdentity, Style, Token};
use fagent_node_memory::{ProjectHandle, Staged};
use std::collections::BTreeSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

/// The in-memory project a commit is staged in.
pub(crate) struct MemoryBase {
    pub(super) handle: Arc<ProjectHandle>,
    pub(super) strict: bool,
}

/// Rows of one staged batch: every row the lib touched, as memory now holds it.
#[derive(Debug, Default)]
pub(crate) struct MemoryRows {
    pub nodes: Vec<Node>,
    pub deleted_nodes: Vec<String>,
    pub tokens: Vec<Token>,
    pub styles: Vec<Style>,
    pub notes: Vec<Note>,
    pub deleted_notes: Vec<String>,
}

impl MemoryBase {
    /// Stages under the write lock. Guarded writer-state readers wait; published
    /// immutable reader views remain available. A rejection returns None so the
    /// reference adapter can preserve public error precedence or persist its candidate.
    pub fn stage(&self, operations: &[Operation]) -> Option<Staged<'_>> {
        match crate::bench_phase::time("memory.stage", || self.handle.begin(operations)) {
            Ok(staged) => Some(staged),
            Err(_) => {
                bump(&COUNTERS.write_fallbacks);
                None
            }
        }
    }

    /// Strict mode (tests): the staged rows and changed ids must equal the
    /// candidate the validator accepted.
    pub fn verify(&self, staged: &Staged<'_>, candidate: &Candidate, changed: &[String]) {
        if !self.strict {
            return;
        }
        let state = staged.state();
        let delta = staged.delta();
        assert_eq!(
            delta.changed_ids(),
            changed,
            "node-memory write: changed ids"
        );
        for id in &delta.nodes.upserted {
            assert_eq!(
                state.node(id),
                candidate.nodes.get(id),
                "node-memory write: node {id}"
            );
        }
        for id in &delta.notes.upserted {
            assert_eq!(
                state.note(id),
                candidate.notes.get(id),
                "node-memory write: note {id}"
            );
        }
        let gone = delta.nodes.deleted.iter().chain(&delta.notes.deleted);
        for id in gone {
            let kept = candidate.nodes.contains_key(id) || candidate.notes.contains_key(id);
            assert!(!kept, "node-memory write: {id} deleted only in memory");
        }
        let tokens: BTreeSet<&String> = delta.tokens.iter().collect();
        for token in state.tokens().filter(|t| tokens.contains(&t.id)) {
            assert_eq!(
                Some(token),
                candidate.tokens.get(&token.id),
                "node-memory write: token"
            );
        }
        let styles: BTreeSet<&String> = delta.styles.iter().collect();
        for style in state.styles().filter(|s| styles.contains(&s.id)) {
            assert_eq!(
                Some(style),
                candidate.styles.get(&style.id),
                "node-memory write: style"
            );
        }
    }
}

/// The rows the staged batch wrote, cloned from memory (touched rows only).
pub(crate) fn rows(staged: &Staged<'_>) -> MemoryRows {
    let state = staged.state();
    let delta = staged.delta();
    let tokens: BTreeSet<&String> = delta.tokens.iter().collect();
    let styles: BTreeSet<&String> = delta.styles.iter().collect();
    MemoryRows {
        nodes: delta
            .nodes
            .upserted
            .iter()
            .filter_map(|id| state.node(id).cloned())
            .collect(),
        deleted_nodes: delta.nodes.deleted.clone(),
        tokens: state
            .tokens()
            .filter(|t| tokens.contains(&t.id))
            .cloned()
            .collect(),
        styles: state
            .styles()
            .filter(|s| styles.contains(&s.id))
            .cloned()
            .collect(),
        notes: delta
            .notes
            .upserted
            .iter()
            .filter_map(|id| state.note(id).cloned())
            .collect(),
        deleted_notes: delta.notes.deleted.clone(),
    }
}

/// The public entity order both engines load snapshots in.
pub(super) fn sort(snapshot: &mut Snapshot) {
    snapshot
        .nodes
        .sort_by(|a, b| (&a.parent_id, a.sort_key, &a.id).cmp(&(&b.parent_id, b.sort_key, &b.id)));
    snapshot.tokens.sort_by(|a, b| a.id.cmp(&b.id));
    snapshot.styles.sort_by(|a, b| a.id.cmp(&b.id));
    snapshot.assets.sort_by(|a, b| a.id.cmp(&b.id));
    snapshot.fonts.sort_by(|a, b| a.id.cmp(&b.id));
    snapshot.notes.sort_by(|a, b| a.id.cmp(&b.id));
}

impl super::Shadow {
    /// Whether commits go through memory (Task28 I3).
    pub fn writes(&self) -> bool {
        self.writes.is_some()
    }

    /// The in-memory project for a commit at the database's `identity`, with its
    /// snapshot in the order both engines load it. `None` (today's path) unless
    /// writes are on and the project is HOT, tagged with this incarnation and at
    /// `identity.revision`; today's commit then (re)loads and tags it.
    pub fn memory_base(&self, identity: &SnapshotCacheIdentity) -> Option<(Snapshot, MemoryBase)> {
        let (Some(strict), Some(reads), Some(mirror)) = (self.writes, &self.reads, &self.mirror)
        else {
            return None;
        };
        let found = catch_unwind(AssertUnwindSafe(|| {
            let handle = mirror.registry.get(&identity.project_id)?;
            if !reads.tagged(&handle, identity) {
                return None;
            }
            let state = handle.read();
            if state.revision() != identity.revision {
                return None;
            }
            let mut before = crate::bench_phase::time("memory.base", || state.to_snapshot());
            drop(state);
            sort(&mut before);
            Some((before, MemoryBase { handle, strict }))
        }));
        match found {
            Ok(Some(found)) => {
                bump(&COUNTERS.write_hits);
                Some(found)
            }
            _ => {
                bump(&COUNTERS.write_misses);
                None
            }
        }
    }

    /// Counts a staged batch rolled back because persisting it failed.
    pub fn write_aborted(&self) {
        bump(&COUNTERS.write_aborts);
    }

    /// Records the incarnation a project was loaded from, so reads and writes use it.
    pub(super) fn tag(&self, handle: &Arc<ProjectHandle>, identity: &SnapshotCacheIdentity) {
        if let Some(reads) = &self.reads {
            reads.tag(handle, identity.clone());
        }
    }
}

impl super::Shadow {
    /// After a commit under the writer lock: moves the tag of the project memory now
    /// holds at `identity.revision` to `identity` (SQLite's incarnation is the file
    /// stamp, which a write may change). Untagged or other projects are left alone.
    pub fn retag(&self, identity: &SnapshotCacheIdentity) {
        let (Some(reads), Some(mirror)) = (&self.reads, &self.mirror) else {
            return;
        };
        let Some(handle) = mirror.registry.get(&identity.project_id) else {
            return;
        };
        let current = handle.read().revision() == identity.revision;
        if current && reads.tags_handle(&handle) {
            reads.tag(&handle, identity.clone());
        }
    }
}
