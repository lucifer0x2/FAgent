//! Scene node indexing shared by every painter: unique ids, roots, children, no cycles,
//! depth ≤ 128. Painters may walk `children` recursively without their own guards.
use crate::RenderError;
use fagent_layout::SceneNode;
use std::collections::BTreeMap;

const MAX_DEPTH: usize = 128;

#[derive(Clone, Debug, Default)]
pub struct SceneIndex {
    /// Nodes without a parent in the scene, in scene order.
    pub roots: Vec<usize>,
    /// Children of each node, in scene order.
    pub children: Vec<Vec<usize>>,
}

impl SceneIndex {
    pub fn new(nodes: &[SceneNode]) -> Result<Self, RenderError> {
        let mut ids = BTreeMap::new();
        for (i, n) in nodes.iter().enumerate() {
            if ids.insert(&n.id, i).is_some() {
                return Err(RenderError::scene("Duplicate scene node id"));
            }
        }
        let mut index = Self {
            roots: Vec::new(),
            children: vec![Vec::new(); nodes.len()],
        };
        for (i, n) in nodes.iter().enumerate() {
            match n.parent_id.as_ref().and_then(|id| ids.get(id)) {
                Some(&parent) => index.children[parent].push(i),
                None => index.roots.push(i),
            }
        }
        // Every node must be reached exactly once from a root (self-parents and cycles are not).
        let mut seen = vec![false; nodes.len()];
        let mut stack: Vec<(usize, usize)> = index.roots.iter().map(|&i| (i, 0)).collect();
        let mut reached = 0;
        while let Some((i, depth)) = stack.pop() {
            if depth > MAX_DEPTH || seen[i] {
                return Err(RenderError::scene("Scene depth or cycle"));
            }
            seen[i] = true;
            reached += 1;
            stack.extend(index.children[i].iter().map(|&c| (c, depth + 1)));
        }
        if reached != nodes.len() {
            return Err(RenderError::scene("Scene cycle"));
        }
        Ok(index)
    }
}
