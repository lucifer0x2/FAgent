//! Eviction policy: TTL and sparse projects first, then least-recently-used until
//! within budget.
use super::Registry;

/// Why a project was dropped from memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Eviction {
    Expired(String),
    OverBudget(String),
    /// Most interned ids belong to deleted nodes: the next access reloads the live
    /// rows, which compacts them.
    Sparse(String),
}

impl Registry {
    pub(super) fn evict_except(&self, keep: Option<&str>) -> Vec<Eviction> {
        let now = self.clock.now();
        let mut map = self.map_mut();
        let mut evicted = Vec::new();
        map.retain(|id, handle| {
            if Some(id.as_str()) == keep {
                return true;
            }
            let eviction = if now.saturating_sub(handle.last_access()) >= self.config.ttl {
                Eviction::Expired(id.clone())
            } else if handle.sparse() {
                Eviction::Sparse(id.clone())
            } else {
                return true;
            };
            evicted.push(eviction);
            false
        });
        let mut total: usize = map.values().map(|h| h.estimated_bytes()).sum();
        if total <= self.config.budget_bytes {
            return evicted;
        }
        let mut lru: Vec<_> = map
            .iter()
            .filter(|(id, _)| Some(id.as_str()) != keep)
            .map(|(id, h)| (h.last_access(), id.clone(), h.estimated_bytes()))
            .collect();
        lru.sort();
        for (_, id, bytes) in lru {
            if total <= self.config.budget_bytes {
                break;
            }
            map.remove(&id);
            total -= bytes;
            evicted.push(Eviction::OverBudget(id));
        }
        evicted
    }
}
