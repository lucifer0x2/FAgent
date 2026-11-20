//! Batch application on a cloned state: update, create, delete subtree and reparent,
//! with the arena's placement rules (renumber siblings to positions, new node at `at`)
//! and reference-index upkeep. Structural checks only, like `ProjectState`.
use super::pmap::{Family, PMap};
use super::state::{Interner, Rec, Set, State};
use fagent_scene_input::{merge_props, Node, NodeKind, NodeProps, Operation};
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::Arc;

pub struct Txn<'a, F: Family> {
    pub state: State<F>,
    interner: &'a mut Interner,
    /// Keys written (the delta for maps that cannot diff).
    pub touched: Vec<u32>,
}

fn is_master(kind: &NodeKind, props: &Value) -> bool {
    *kind == NodeKind::Frame && NodeProps::new(props).component().is_some()
}

fn used_master(node: &Node) -> Option<&str> {
    match node.kind {
        NodeKind::Instance => NodeProps::new(&node.props).component_id(),
        _ => None,
    }
}

fn link_targets(node: &Node) -> BTreeSet<&str> {
    NodeProps::new(&node.props).link_targets().collect()
}

fn set_member<M: PMap<Set>>(map: &mut M, key: u32, member: u32, add: bool) {
    let mut set = map.get(key).cloned().unwrap_or_default();
    let changed = match add {
        true => Arc::make_mut(&mut set).insert(member),
        false => Arc::make_mut(&mut set).remove(&member),
    };
    if !changed {
        return;
    }
    match set.is_empty() {
        true => {
            map.remove(key);
        }
        false => map.insert(key, set),
    }
}

impl<'a, F: Family> Txn<'a, F> {
    pub fn begin(state: &State<F>, interner: &'a mut Interner) -> Self {
        Self {
            state: state.clone(),
            interner,
            touched: Vec::new(),
        }
    }

    pub fn apply(&mut self, op: &Operation) -> Result<(), &'static str> {
        match op {
            Operation::UpdateNode { id, props, merge } => self.update(id, props, *merge),
            Operation::CreateNode {
                id,
                kind,
                parent_id: Some(parent),
                props,
                index,
            } => self.create(id, kind.clone(), parent, props.clone(), *index),
            Operation::DeleteSubtree { id } => self.delete(id),
            Operation::ReparentNode { id, parent_id, .. } => self.reparent(id, parent_id),
            _ => Err("unsupported in spike"),
        }
    }

    pub fn finish(mut self) -> (State<F>, Vec<u32>) {
        self.state.revision += 1;
        (self.state, self.touched)
    }

    fn live(&self, id: &str) -> Result<u32, &'static str> {
        let key = self.interner.get(id).ok_or("NOT_FOUND")?;
        self.state.rec(key).map(|_| key).ok_or("NOT_FOUND")
    }

    fn rec(&self, key: u32) -> &Rec {
        self.state.rec(key).expect("live key")
    }

    fn put(&mut self, key: u32, rec: Rec) {
        self.state.nodes.insert(key, rec);
        self.touched.push(key);
    }

    fn refs(&mut self, key: u32, node: &Node, add: bool) {
        if let Some(master) = used_master(node) {
            let master = self.interner.intern(master);
            set_member(&mut self.state.users, master, key, add);
        }
        for target in link_targets(node) {
            let target = self.interner.intern(target);
            set_member(&mut self.state.links, target, key, add);
        }
    }

    fn with_node(&mut self, key: u32, edit: impl FnOnce(&mut Node)) {
        let mut rec = self.rec(key).clone();
        let old = &rec.node;
        let mut node = Node {
            id: old.id.clone(),
            parent_id: old.parent_id.clone(),
            sort_key: old.sort_key,
            kind: old.kind.clone(),
            // A changed node is a new node: its props are copied (the arena edits in place).
            props: old.props.clone(),
        };
        edit(&mut node);
        rec.node = Arc::new(node);
        self.put(key, rec);
    }

    fn update(&mut self, id: &str, patch: &Value, merge: bool) -> Result<(), &'static str> {
        let key = self.live(id)?;
        let patch = patch.as_object().ok_or("props must be an object")?;
        let old = self.rec(key).node.clone();
        let mut props = old.props.as_object().cloned().ok_or("props")?;
        match merge {
            true => merge_props(&mut props, patch),
            false => props.extend(patch.clone()),
        }
        let props = Value::Object(props);
        if props == old.props {
            return Ok(());
        }
        let mut rec = self.rec(key).clone();
        rec.node = Arc::new(Node {
            id: old.id.clone(),
            parent_id: old.parent_id.clone(),
            sort_key: old.sort_key,
            kind: old.kind.clone(),
            props,
        });
        let (before, after) = (old, rec.node.clone());
        if used_master(&before) != used_master(&after)
            || link_targets(&before) != link_targets(&after)
        {
            self.refs(key, &before, false);
            self.refs(key, &after, true);
        }
        self.put(key, rec);
        Ok(())
    }

    /// Sets each sibling's sort key to its position, position `at` counted as taken
    /// (the arena's `renumber`). Every changed sibling gets a new node.
    fn renumber(&mut self, list: &[u32], at: usize) {
        for (position, &key) in list.iter().enumerate() {
            let wanted = position as i64 + i64::from(position >= at);
            if self.rec(key).node.sort_key != wanted {
                self.with_node(key, |node| node.sort_key = wanted);
            }
        }
    }

    fn set_children(&mut self, parent: u32, list: Vec<u32>) {
        match list.is_empty() {
            true => {
                self.state.children.remove(parent);
            }
            false => self.state.children.insert(parent, Arc::new(list)),
        }
    }

    fn derived(&self, parent: u32, key: u32, node: &Node) -> (u32, Option<u32>, Option<u32>) {
        let p = self.rec(parent);
        let root = match p.parent {
            None => Some(key),
            Some(_) => p.root,
        };
        let master = p
            .master
            .or(is_master(&node.kind, &node.props).then_some(key));
        (p.page, root, master)
    }

    fn create(
        &mut self,
        id: &str,
        kind: NodeKind,
        parent: &str,
        props: Value,
        index: Option<usize>,
    ) -> Result<(), &'static str> {
        if self.live(id).is_ok() {
            return Err("NODE_EXISTS");
        }
        let parent_key = self.live(parent)?;
        let mut list = self.state.children(Some(parent_key)).to_vec();
        let at = index.unwrap_or(list.len()).min(list.len());
        self.renumber(&list, at);
        let key = self.interner.intern(id);
        let node = Node {
            id: id.into(),
            parent_id: Some(parent.into()),
            sort_key: at as i64,
            kind,
            props,
        };
        let (page, root, master) = self.derived(parent_key, key, &node);
        self.refs(key, &node, true);
        let rec = Rec {
            node: Arc::new(node),
            parent: Some(parent_key),
            page,
            root,
            master,
        };
        self.put(key, rec);
        list.insert(at, key);
        self.set_children(parent_key, list);
        self.state.len += 1;
        Ok(())
    }

    fn delete(&mut self, id: &str) -> Result<(), &'static str> {
        let key = self.live(id)?;
        let gone = self.state.subtree(key);
        let parent = self
            .rec(key)
            .parent
            .ok_or("pages are not deleted in the spike")?;
        let mut list = self.state.children(Some(parent)).to_vec();
        list.retain(|k| *k != key);
        self.set_children(parent, list);
        let gone_set: BTreeSet<u32> = gone.iter().copied().collect();
        let gone_ids: BTreeSet<String> =
            gone.iter().map(|k| self.rec(*k).node.id.clone()).collect();
        let mut sources = BTreeSet::new();
        for &k in &gone {
            let node = self.rec(k).node.clone();
            self.refs(k, &node, false);
            if let Some(linking) = self.state.links.get(k) {
                sources.extend(linking.iter().filter(|s| !gone_set.contains(s)));
            }
            self.state.nodes.remove(k);
            self.state.children.remove(k);
            self.touched.push(k);
        }
        for source in sources {
            let before = self.rec(source).node.clone();
            self.with_node(source, |node| {
                if let Some(links) = node.props.get_mut("links").and_then(Value::as_array_mut) {
                    links.retain(|l| !l["to"].as_str().is_some_and(|to| gone_ids.contains(to)));
                }
            });
            let after = self.rec(source).node.clone();
            self.refs(source, &before, false);
            self.refs(source, &after, true);
        }
        self.state.len -= gone.len();
        Ok(())
    }

    fn reparent(&mut self, id: &str, parent: &str) -> Result<(), &'static str> {
        let key = self.live(id)?;
        let to = self.live(parent)?;
        let mut up = Some(to);
        for _ in 0..=128 {
            match up {
                Some(k) if k == key => return Err("CYCLE"),
                Some(k) => up = self.rec(k).parent,
                None => break,
            }
        }
        let from = self
            .rec(key)
            .parent
            .ok_or("pages are not moved in the spike")?;
        let mut old = self.state.children(Some(from)).to_vec();
        old.retain(|k| *k != key);
        self.set_children(from, old);
        let mut list = self.state.children(Some(to)).to_vec();
        let at = list.len();
        self.renumber(&list, at);
        let parent_id = parent.to_owned();
        self.with_node(key, |node| {
            node.parent_id = Some(parent_id);
            node.sort_key = at as i64;
        });
        list.push(key);
        self.set_children(to, list);
        // Derived indexes of the moved subtree: records only, nodes stay shared.
        let mut stack = vec![(to, key)];
        while let Some((parent, k)) = stack.pop() {
            let node = self.rec(k).node.clone();
            let (page, root, master) = self.derived(parent, k, &node);
            let rec = self.rec(k);
            if (rec.page, rec.root, rec.master) != (page, root, master) {
                let mut rec = rec.clone();
                (rec.page, rec.root, rec.master) = (page, root, master);
                self.state.nodes.insert(k, rec);
            }
            stack.extend(self.state.children(Some(k)).iter().map(|c| (k, *c)));
        }
        let mut rec = self.rec(key).clone();
        rec.parent = Some(to);
        self.state.nodes.insert(key, rec);
        Ok(())
    }
}
