//! Entity rows one commit persisted, engine-neutral and keyed by `(kind, id)` with
//! the row as a JSON value, so both engines feed the same comparison.
use fagent_node_memory::ProjectState;
use fagent_scene_input::Snapshot;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Entity tables a commit can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Kind {
    Node,
    Token,
    Style,
    Asset,
    Font,
    Note,
}

impl Kind {
    pub const ALL: [Kind; 6] = [
        Kind::Node,
        Kind::Token,
        Kind::Style,
        Kind::Asset,
        Kind::Font,
        Kind::Note,
    ];

    /// The PostgreSQL `entities.kind` value.
    pub fn parse(kind: &str) -> Option<Kind> {
        Some(match kind {
            "node" => Kind::Node,
            "token" => Kind::Token,
            "style" => Kind::Style,
            "asset" => Kind::Asset,
            "font" => Kind::Font,
            "note" => Kind::Note,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Node => "node",
            Kind::Token => "token",
            Kind::Style => "style",
            Kind::Asset => "asset",
            Kind::Font => "font",
            Kind::Note => "note",
        }
    }
}

/// Upserted rows (with their value) and deleted row ids of one commit.
#[derive(Debug, Default)]
pub(crate) struct Rows {
    pub upserts: BTreeMap<(Kind, String), Value>,
    pub deletes: BTreeSet<(Kind, String)>,
}

impl Rows {
    pub fn upsert<T: Serialize>(&mut self, kind: Kind, id: &str, row: &T) {
        // Domain rows always serialize; a failure is reported as a missing row.
        let value = serde_json::to_value(row).unwrap_or(Value::Null);
        self.upserts.insert((kind, id.to_owned()), value);
    }

    pub fn delete(&mut self, kind: Kind, id: &str) {
        self.deletes.insert((kind, id.to_owned()));
    }

    /// Parses stored row text; an unknown kind or unparsable text is kept as `Null`,
    /// which then compares unequal.
    pub fn upsert_text(&mut self, kind: &str, id: &str, text: &str) {
        let value = serde_json::from_str(text).unwrap_or(Value::Null);
        let kind = Kind::parse(kind).unwrap_or(Kind::Asset);
        self.upserts.insert((kind, id.to_owned()), value);
    }

    pub fn delete_text(&mut self, kind: &str, id: &str) {
        self.delete(Kind::parse(kind).unwrap_or(Kind::Asset), id);
    }
}

fn value<T: Serialize>(row: Option<&T>) -> Option<Value> {
    row.and_then(|r| serde_json::to_value(r).ok())
}

/// One row of the in-memory project as JSON; `None` when absent. Assets and fonts
/// are not exposed by the lib and are compared only through full snapshots.
pub(crate) fn ram_row(state: &ProjectState, kind: Kind, id: &str) -> Option<Value> {
    match kind {
        Kind::Node => value(state.node(id)),
        Kind::Token => value(state.tokens().find(|t| t.id == id)),
        Kind::Style => value(state.styles().find(|s| s.id == id)),
        Kind::Note => value(state.note(id)),
        Kind::Asset | Kind::Font => None,
    }
}

/// The rows of one table for which `keep` holds, keyed by `(kind, id)`.
fn table<T: Serialize>(
    out: &mut BTreeMap<(Kind, String), Value>,
    kind: Kind,
    rows: &[T],
    id: impl Fn(&T) -> &String,
    keep: impl Fn(&str) -> bool,
) {
    for row in rows.iter().filter(|r| keep(id(r))) {
        let value = serde_json::to_value(row).unwrap_or(Value::Null);
        out.insert((kind, id(row).clone()), value);
    }
}

/// Every row of a snapshot (with `keep`, only the kept ids) keyed by `(kind, id)`.
fn rows_where(s: &Snapshot, keep: impl Fn(Kind, &str) -> bool) -> BTreeMap<(Kind, String), Value> {
    let mut out = BTreeMap::new();
    let k = &keep;
    table(
        &mut out,
        Kind::Node,
        &s.nodes,
        |r| &r.id,
        |id| k(Kind::Node, id),
    );
    table(
        &mut out,
        Kind::Token,
        &s.tokens,
        |r| &r.id,
        |id| k(Kind::Token, id),
    );
    table(
        &mut out,
        Kind::Style,
        &s.styles,
        |r| &r.id,
        |id| k(Kind::Style, id),
    );
    table(
        &mut out,
        Kind::Asset,
        &s.assets,
        |r| &r.id,
        |id| k(Kind::Asset, id),
    );
    table(
        &mut out,
        Kind::Font,
        &s.fonts,
        |r| &r.id,
        |id| k(Kind::Font, id),
    );
    table(
        &mut out,
        Kind::Note,
        &s.notes,
        |r| &r.id,
        |id| k(Kind::Note, id),
    );
    out
}

/// Every row of a snapshot keyed by `(kind, id)`.
pub(crate) fn snapshot_rows(snapshot: &Snapshot) -> BTreeMap<(Kind, String), Value> {
    rows_where(snapshot, |_, _| true)
}

/// The base-revision rows for `wanted`, by one pass over the snapshot that
/// serializes only the wanted rows.
pub(crate) fn before_rows(
    before: &Snapshot,
    wanted: &BTreeSet<(Kind, String)>,
) -> BTreeMap<(Kind, String), Value> {
    let mut ids: HashMap<Kind, HashSet<&str>> = HashMap::new();
    for (kind, id) in wanted {
        ids.entry(*kind).or_default().insert(id);
    }
    rows_where(before, |kind, id| {
        ids.get(&kind).is_some_and(|set| set.contains(id))
    })
}
