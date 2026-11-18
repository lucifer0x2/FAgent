//! The token checks of one `upsert_token`, without revalidating the whole token set:
//! `Candidate::apply` runs `validate_tokens` after every upsert, which costs
//! O(tokens) per operation. Starting from a token set that passed `validate_tokens`,
//! only the written token, the tokens aliasing it and the alias paths through it can
//! fail, and they fail with the error `validate_tokens` reports.
use super::{
    token_graph::validate_token_graph,
    tokens::{
        alias_targets, check_token, too_many_themes, too_many_tokens, MAX_ALIAS_DEPTH, MAX_THEMES,
        MAX_TOKENS,
    },
    Lookup,
};
use crate::{error::*, model::Token};
use std::collections::{BTreeMap, BTreeSet};

/// Checks the token set after token `id` was written over `previous` (`None` for a
/// new token), with `validate_tokens`'s precedence: the token count, the per-token
/// checks in id order, the theme count, then the alias graph.
///
/// The token set before the write must have passed `validate_tokens`; tokens the
/// write cannot affect are not checked again. When the alias graph fails, the error
/// is the one a full graph check reports (cycle or depth, whichever its walk meets
/// first), so the whole graph is walked once on that rejection only.
pub fn check_token_write(lookup: &dyn Lookup, id: &str, previous: Option<&Token>) -> Result<()> {
    if lookup.token_count() > MAX_TOKENS {
        return Err(too_many_tokens());
    }
    let token = lookup
        .token(id)
        .ok_or_else(|| StorageError::validation("token.id", "Written token is missing"))?;
    let mut affected = BTreeSet::from([id]);
    if previous.is_some_and(|p| p.token_type != token.token_type) {
        affected.extend(lookup.aliasers(id));
    }
    for id in affected {
        if let Some(token) = lookup.token(id) {
            check_token(lookup, token)?;
        }
    }
    if 1 + lookup.named_themes() > MAX_THEMES + 1 {
        return Err(too_many_themes());
    }
    let edges = alias_targets(token);
    if previous.is_some_and(|p| alias_targets(p) == edges)
        || (previous.is_none() && edges.is_empty())
    {
        return Ok(());
    }
    match graph_fault(lookup, id) {
        None => Ok(()),
        Some(fault) => Err(validate_token_graph(lookup).err().unwrap_or(fault)),
    }
}

/// The fault of the alias paths through `id`, its out-edges being the only ones that
/// changed in a graph that was acyclic and at most 32 edges deep: a cycle (every new
/// cycle passes through `id`) or a path longer than 32 edges (chain into `id` plus
/// height of `id`).
fn graph_fault(lookup: &dyn Lookup, id: &str) -> Option<StorageError> {
    let mut heights = BTreeMap::new();
    let height = match height(lookup, id, &mut Vec::new(), &mut heights) {
        Ok(height) => height,
        Err(fault) => return Some(fault),
    };
    let mut chains = BTreeMap::new();
    match chain(lookup, id, MAX_ALIAS_DEPTH - height, &mut chains) {
        Some(_) => None,
        None => Some(too_deep()),
    }
}

/// Edges below `id` in its longest alias path, failing on a cycle or more than 32.
fn height<'a>(
    lookup: &'a dyn Lookup,
    id: &'a str,
    active: &mut Vec<&'a str>,
    memo: &mut BTreeMap<&'a str, usize>,
) -> Result<usize> {
    if active.contains(&id) {
        return Err(StorageError::validation(
            "token.aliasId",
            "Token alias cycle detected across theme branches",
        ));
    }
    if let Some(height) = memo.get(id) {
        return Ok(*height);
    }
    if active.len() > MAX_ALIAS_DEPTH {
        return Err(too_deep());
    }
    let Some(token) = lookup.token(id) else {
        return Ok(0);
    };
    active.push(id);
    let mut height = 0;
    for target in alias_targets(token) {
        height = height.max(self::height(lookup, target, active, memo)? + 1);
    }
    active.pop();
    if height > MAX_ALIAS_DEPTH {
        return Err(too_deep());
    }
    memo.insert(id, height);
    Ok(height)
}

/// Edges in the longest alias chain ending at `id` (its aliasers, theirs, ...), or
/// `None` when it exceeds `limit`. The graph is acyclic here.
fn chain<'a>(
    lookup: &'a dyn Lookup,
    id: &'a str,
    limit: usize,
    memo: &mut BTreeMap<&'a str, usize>,
) -> Option<usize> {
    if let Some(length) = memo.get(id) {
        return Some(*length);
    }
    let mut length = 0;
    for source in lookup.aliasers(id) {
        let above = chain(lookup, source, limit.checked_sub(1)?, memo)? + 1;
        length = length.max(above);
    }
    if length > limit {
        return None;
    }
    memo.insert(id, length);
    Some(length)
}

fn too_deep() -> StorageError {
    StorageError::validation("token.aliasId", "Token alias depth exceeds 32")
}

#[cfg(test)]
#[path = "token_write_tests.rs"]
mod tests;
