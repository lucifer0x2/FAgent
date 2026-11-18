//! Persistent radix trie over dense `u32` keys: 64-way nodes, depth grown on demand
//! (3 levels hold 262,144 keys). Cloning is one `Arc` bump; a write copies only the
//! 64-slot nodes on its path (`Arc::make_mut`), so a batch touching k keys copies at
//! most `depth × k` small nodes and every untouched node stays shared with the
//! previous version. [`Trie::diff`] skips shared nodes by pointer.
use std::sync::Arc;

const BITS: u32 = 6;
const WIDTH: usize = 1 << BITS;
const MASK: u32 = (WIDTH as u32) - 1;

// Both variants live behind an `Arc`; branches are about 1 in 64 nodes, so sizing
// them like a leaf costs little and keeps one node type.
#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
enum Node<V> {
    Branch([Option<Arc<Node<V>>>; WIDTH]),
    Leaf([Option<V>; WIDTH]),
}

impl<V> Node<V> {
    fn branch() -> Self {
        Node::Branch(std::array::from_fn(|_| None))
    }

    fn leaf() -> Self {
        Node::Leaf(std::array::from_fn(|_| None))
    }
}

/// A persistent map `u32 -> V`.
pub(crate) struct Trie<V> {
    root: Option<Arc<Node<V>>>,
    /// Levels below the root; keys below `WIDTH << (BITS * depth)` fit.
    depth: u32,
    len: usize,
    /// Allocated trie nodes reachable from this version (for byte estimates).
    nodes: usize,
}

impl<V> Clone for Trie<V> {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
            depth: self.depth,
            len: self.len,
            nodes: self.nodes,
        }
    }
}

impl<V> Default for Trie<V> {
    fn default() -> Self {
        Self {
            root: None,
            depth: 0,
            len: 0,
            nodes: 0,
        }
    }
}

fn slot(key: u32, level: u32) -> usize {
    ((key >> (BITS * level)) & MASK) as usize
}

impl<V> Trie<V> {
    fn capacity(&self) -> u64 {
        (WIDTH as u64) << (BITS * self.depth)
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn get(&self, key: u32) -> Option<&V> {
        if u64::from(key) >= self.capacity() {
            return None;
        }
        let mut node = self.root.as_deref()?;
        let mut level = self.depth;
        loop {
            match node {
                Node::Branch(children) => node = children[slot(key, level)].as_deref()?,
                Node::Leaf(values) => return values[slot(key, 0)].as_ref(),
            }
            level = level.wrapping_sub(1);
        }
    }

    pub(crate) fn contains(&self, key: u32) -> bool {
        self.get(key).is_some()
    }

    /// Estimated heap bytes of the trie nodes of this version (shared nodes count in
    /// every version that reaches them).
    pub(crate) fn node_bytes(&self) -> usize {
        let branch = std::mem::size_of::<Node<V>>() + 2 * std::mem::size_of::<usize>();
        self.nodes * branch
    }

    /// Entries in key order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (u32, &V)> + '_ {
        let mut stack: Vec<(&Node<V>, u32, u32)> = Vec::new();
        if let Some(root) = self.root.as_deref() {
            stack.push((root, self.depth, 0));
        }
        let mut leaf: Option<(&[Option<V>; WIDTH], u32, usize)> = None;
        std::iter::from_fn(move || loop {
            if let Some((values, base, at)) = leaf.as_mut() {
                while *at < WIDTH {
                    let i = *at;
                    *at += 1;
                    if let Some(v) = values[i].as_ref() {
                        return Some((*base | i as u32, v));
                    }
                }
                leaf = None;
            }
            let (node, level, base) = stack.pop()?;
            match node {
                Node::Leaf(values) => leaf = Some((values, base, 0)),
                Node::Branch(children) => {
                    for (i, child) in children.iter().enumerate().rev() {
                        if let Some(child) = child.as_deref() {
                            stack.push((child, level - 1, base | ((i as u32) << (BITS * level))));
                        }
                    }
                }
            }
        })
    }
}

impl<V: Clone> Trie<V> {
    /// The value at `key` for writing, copying shared nodes on the path; `None` (and
    /// nothing copied) when absent.
    pub(crate) fn get_mut(&mut self, key: u32) -> Option<&mut V> {
        self.get(key)?;
        let mut node = Arc::make_mut(self.root.as_mut()?);
        let mut level = self.depth;
        loop {
            match node {
                Node::Branch(children) => {
                    node = Arc::make_mut(children[slot(key, level)].as_mut()?);
                }
                Node::Leaf(values) => return values[slot(key, 0)].as_mut(),
            }
            level = level.wrapping_sub(1);
        }
    }

    /// Inserts or replaces, returning the previous value.
    pub(crate) fn insert(&mut self, key: u32, value: V) -> Option<V> {
        while u64::from(key) >= self.capacity() {
            if let Some(root) = self.root.take() {
                let mut branch = Node::branch();
                if let Node::Branch(children) = &mut branch {
                    children[0] = Some(root);
                }
                self.root = Some(Arc::new(branch));
                self.nodes += 1;
            }
            self.depth += 1;
        }
        let depth = self.depth;
        let mut created = 0;
        let root = self.root.get_or_insert_with(|| {
            created += 1;
            Arc::new(match depth {
                0 => Node::leaf(),
                _ => Node::branch(),
            })
        });
        let mut node = Arc::make_mut(root);
        let mut level = depth;
        let old = loop {
            match node {
                Node::Branch(children) => {
                    let child = children[slot(key, level)].get_or_insert_with(|| {
                        created += 1;
                        Arc::new(match level {
                            1 => Node::leaf(),
                            _ => Node::branch(),
                        })
                    });
                    node = Arc::make_mut(child);
                }
                Node::Leaf(values) => break values[slot(key, 0)].replace(value),
            }
            level -= 1;
        };
        self.nodes += created;
        self.len += usize::from(old.is_none());
        old
    }

    /// Clears `key`, returning its value. Emptied nodes are kept: keys are interned
    /// ids that a later create of the same id reuses.
    pub(crate) fn remove(&mut self, key: u32) -> Option<V> {
        self.get(key)?;
        let mut node = Arc::make_mut(self.root.as_mut()?);
        let mut level = self.depth;
        loop {
            match node {
                Node::Branch(children) => {
                    node = Arc::make_mut(children[slot(key, level)].as_mut()?);
                }
                Node::Leaf(values) => {
                    let old = values[slot(key, 0)].take();
                    self.len -= usize::from(old.is_some());
                    return old;
                }
            }
            level = level.wrapping_sub(1);
        }
    }
}

impl<V> Trie<V> {
    /// Keys whose entry differs from `old` under `same`, skipping subtrees shared by
    /// pointer. `self` must descend from `old` (depth only grows).
    pub(crate) fn diff(&self, old: &Self, same: impl Fn(&V, &V) -> bool) -> Vec<u32> {
        assert!(self.depth >= old.depth, "depth only grows");
        // A grown trie keeps the old root in slot 0 of each added level.
        let mut old_root = old.root.clone();
        for _ in old.depth..self.depth {
            if let Some(root) = old_root.take() {
                let mut branch = Node::branch();
                if let Node::Branch(children) = &mut branch {
                    children[0] = Some(root);
                }
                old_root = Some(Arc::new(branch));
            }
        }
        let mut out = Vec::new();
        walk(
            self.root.as_ref(),
            old_root.as_ref(),
            self.depth,
            0,
            &same,
            &mut out,
        );
        out
    }
}

fn walk<V>(
    new: Option<&Arc<Node<V>>>,
    old: Option<&Arc<Node<V>>>,
    level: u32,
    base: u32,
    same: &impl Fn(&V, &V) -> bool,
    out: &mut Vec<u32>,
) {
    if let (Some(a), Some(b)) = (new, old) {
        if Arc::ptr_eq(a, b) {
            return;
        }
    }
    let span = |i: usize| base | ((i as u32) << (BITS * level));
    if level == 0 {
        let (a, b) = (leaf_of(new), leaf_of(old));
        for i in 0..WIDTH {
            let x = a.and_then(|v| v[i].as_ref());
            let y = b.and_then(|v| v[i].as_ref());
            let differs = match (x, y) {
                (Some(x), Some(y)) => !same(x, y),
                (None, None) => false,
                _ => true,
            };
            if differs {
                out.push(span(i));
            }
        }
        return;
    }
    for i in 0..WIDTH {
        let (a, b) = (child_of(new, i), child_of(old, i));
        if a.is_some() || b.is_some() {
            walk(a, b, level - 1, span(i), same, out);
        }
    }
}

fn leaf_of<V>(n: Option<&Arc<Node<V>>>) -> Option<&[Option<V>; WIDTH]> {
    match n.map(|n| &**n) {
        Some(Node::Leaf(values)) => Some(values),
        _ => None,
    }
}

fn child_of<V>(n: Option<&Arc<Node<V>>>, i: usize) -> Option<&Arc<Node<V>>> {
    match n.map(|n| &**n) {
        Some(Node::Branch(children)) => children[i].as_ref(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::Trie;

    #[test]
    fn insert_get_remove_iter_and_diff() {
        let mut trie = Trie::default();
        for key in (0..300_000).step_by(7) {
            trie.insert(key, key * 2);
        }
        assert_eq!(trie.len(), 42_858);
        assert_eq!(trie.get(70), Some(&140));
        assert_eq!(trie.get(71), None);
        assert_eq!(trie.get(u32::MAX), None);
        let base = trie.clone();
        *trie.get_mut(70).unwrap() = 1;
        assert_eq!(trie.remove(7), Some(14));
        trie.insert(1 << 20, 5);
        assert_eq!(base.get(70), Some(&140));
        assert_eq!(base.get(7), Some(&14));
        assert_eq!(trie.diff(&base, |a, b| a == b), vec![7, 70, 1 << 20]);
        let keys: Vec<u32> = trie.iter().map(|(k, _)| k).take(3).collect();
        assert_eq!(keys, vec![0, 14, 21]);
        assert_eq!(trie.iter().count(), trie.len());
        assert!(trie.node_bytes() > 0);
    }
}
