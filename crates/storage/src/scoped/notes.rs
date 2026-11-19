//! Scoped notes: footprints name anchor pages; writes need guarded anchors.
use super::index::PageIndex;
use crate::*;

/// A note change also names its old and new anchor pages, so guards on those
/// pages see note edits, moves, cascades and delete/recreate.
pub(super) fn footprint(
    before: &Snapshot,
    after: &Candidate,
    old_pages: &PageIndex,
    new_pages: &PageIndex,
    keys: &mut BTreeSet<String>,
) -> Result<()> {
    let old: BTreeMap<_, _> = before.notes.iter().map(|n| (n.id.as_str(), n)).collect();
    let ids: BTreeSet<_> = old
        .keys()
        .copied()
        .chain(after.notes.keys().map(String::as_str))
        .collect();
    for id in ids {
        let (old, new) = (old.get(id).copied(), after.notes.get(id));
        if super::history::same(old, new)? {
            continue;
        }
        keys.insert(format!("note:{id}"));
        for (note, pages) in [(old, old_pages), (new, new_pages)] {
            if let Some(page) = note.and_then(|n| pages.page(&n.node_id)) {
                keys.insert(format!("page:{page}"));
            }
        }
    }
    Ok(())
}
