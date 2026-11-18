//! Hand-written persistent radix trie over dense `u32` keys: 64-way nodes, depth grown
//! on demand (3 levels up to 262,144 keys). Clone is one `Arc` bump; a write copies the
//! 64-slot nodes on its path (`Arc::make_mut`), so a commit touching k keys copies at
//! most 3k small nodes. No dependencies.
use std::sync::Arc;

const BITS: u32 = 6;
const WIDTH: usize = 1 << BITS;
const MASK: u32 = (WIDTH as u32) - 1;

#[derive(Clone)]
enum Node<V> {
    Branch(Vec<Option<Arc<Node<V>>>>),
    Leaf(Vec<Option<V>>),
}

#[derive(Clone)]
pub struct Radix<V> {
    root: Option<Arc<Node<V>>>,
    /// Levels below the root; keys below `WIDTH << (BITS * depth)` fit.
    depth: u32,
    len: usize,
}

impl<V> Default for Radix<V> {
    fn default() -> Self {
        Self {
            root: None,
            depth: 0,
            len: 0,
        }
    }
}

fn slot(key: u32, level: u32) -> usize {
    ((key >> (BITS * level)) & MASK) as usize
}

fn empty_branch<V>() -> Node<V> {
    Node::Branch(vec![None; WIDTH])
}

fn empty_leaf<V>() -> Node<V> {
    Node::Leaf((0..WIDTH).map(|_| None).collect())
}

impl<V: Clone> Radix<V> {
    fn capacity(&self) -> u64 {
        (WIDTH as u64) << (BITS * self.depth)
    }

    pub fn get(&self, key: u32) -> Option<&V> {
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
            level -= 1;
        }
    }

    pub fn insert(&mut self, key: u32, value: V) {
        while u64::from(key) >= self.capacity() {
            let mut branch = vec![None; WIDTH];
            branch[0] = self.root.take();
            self.root = Some(Arc::new(Node::Branch(branch)));
            self.depth += 1;
        }
        let depth = self.depth;
        let root = self.root.get_or_insert_with(|| {
            Arc::new(match depth {
                0 => empty_leaf(),
                _ => empty_branch(),
            })
        });
        let mut node = Arc::make_mut(root);
        let mut level = depth;
        loop {
            match node {
                Node::Branch(children) => {
                    let child = children[slot(key, level)].get_or_insert_with(|| {
                        Arc::new(match level {
                            1 => empty_leaf(),
                            _ => empty_branch(),
                        })
                    });
                    node = Arc::make_mut(child);
                }
                Node::Leaf(values) => {
                    if values[slot(key, 0)].replace(value).is_none() {
                        self.len += 1;
                    }
                    return;
                }
            }
            level -= 1;
        }
    }

    /// Clears the key; empty nodes are kept (keys are not reused in the spike).
    pub fn remove(&mut self, key: u32) -> Option<V> {
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
            level -= 1;
        }
    }

    /// Keys whose entry differs from `old`, skipping subtrees shared by pointer.
    pub fn diff(&self, old: &Self) -> Vec<u32>
    where
        V: PartialEq,
    {
        assert!(self.depth >= old.depth, "depth only grows");
        // Lift `old` to this depth: a grown trie keeps the old root in slot 0.
        let mut old_root = old.root.clone();
        for _ in old.depth..self.depth {
            if let Some(root) = old_root.take() {
                let mut branch = vec![None; WIDTH];
                branch[0] = Some(root);
                old_root = Some(Arc::new(Node::Branch(branch)));
            }
        }
        let mut out = Vec::new();
        walk(
            self.root.as_ref(),
            old_root.as_ref(),
            self.depth,
            0,
            &mut out,
        );
        out
    }
}

fn walk<V: PartialEq>(
    new: Option<&Arc<Node<V>>>,
    old: Option<&Arc<Node<V>>>,
    level: u32,
    base: u32,
    out: &mut Vec<u32>,
) {
    if let (Some(a), Some(b)) = (new, old) {
        if Arc::ptr_eq(a, b) {
            return;
        }
    }
    let span = |i: usize| base | ((i as u32) << (BITS * level));
    match (new.map(|n| &**n), old.map(|n| &**n)) {
        (None, None) => {}
        (Some(Node::Leaf(a)), b) => {
            for (i, value) in a.iter().enumerate() {
                let before = match b {
                    Some(Node::Leaf(b)) => b[i].as_ref(),
                    _ => None,
                };
                if value.as_ref() != before {
                    out.push(span(i));
                }
            }
        }
        (None, Some(Node::Leaf(b))) => {
            out.extend(
                b.iter()
                    .enumerate()
                    .filter(|(_, v)| v.is_some())
                    .map(|(i, _)| span(i)),
            );
        }
        (a, b) => {
            fn child<V>(n: Option<&Node<V>>, i: usize) -> Option<&Arc<Node<V>>> {
                match n {
                    Some(Node::Branch(c)) => c[i].as_ref(),
                    _ => None,
                }
            }
            for i in 0..WIDTH {
                walk(child(a, i), child(b, i), level - 1, span(i), out);
            }
        }
    }
}
