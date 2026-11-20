//! Registry CRUD, TTL and budget eviction with a manual clock, and lock isolation.
mod support;

use fagent_node_memory::{Eviction, ManualClock, Registry, RegistryConfig};
use fagent_scene_input::{NodeKind, Snapshot};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use support::*;

fn project(id: &str) -> Snapshot {
    let mut snapshot = small();
    snapshot.project_id = id.into();
    snapshot
}

fn registry(ttl_secs: u64, budget: usize) -> (Registry, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::default());
    let config = RegistryConfig {
        ttl: Duration::from_secs(ttl_secs),
        budget_bytes: budget,
    };
    (Registry::new(config, clock.clone()), clock)
}

#[test]
fn load_get_list_remove() {
    let (registry, clock) = registry(60, usize::MAX);
    let (a, evicted) = registry.load(project("a")).unwrap();
    assert!(evicted.is_empty());
    clock.advance(Duration::from_secs(1));
    registry.load(project("b")).unwrap();
    let error = registry.load(project("a")).err().expect("duplicate");
    assert_eq!(error.code, "PROJECT_EXISTS");
    let rows = registry.list();
    let ids: Vec<_> = rows.iter().map(|r| r.project_id.as_str()).collect();
    assert_eq!(ids, ["a", "b"]);
    assert_eq!(rows[0].last_access, Duration::ZERO);
    assert_eq!(rows[1].last_access, Duration::from_secs(1));
    assert_eq!(registry.total_bytes(), 2 * a.estimated_bytes());
    assert_eq!(registry.get("a").unwrap().read().len(), 18);
    assert!(registry.remove("a").is_some());
    assert!(registry.get("a").is_none());
    // An evicted or removed handle stays usable by its holder.
    assert_eq!(a.read().revision(), 1);
}

#[test]
fn ttl_evicts_idle_projects_only() {
    let (registry, clock) = registry(10, usize::MAX);
    registry.load(project("a")).unwrap();
    registry.load(project("b")).unwrap();
    clock.advance(Duration::from_secs(6));
    let b = registry.get("b").unwrap();
    clock.advance(Duration::from_secs(5));
    assert_eq!(registry.evict(), [Eviction::Expired("a".into())]);
    b.apply(&[delete("t2")]).unwrap();
    clock.advance(Duration::from_secs(9));
    assert!(registry.evict().is_empty());
    clock.advance(Duration::from_secs(1));
    assert_eq!(registry.evict(), [Eviction::Expired("b".into())]);
    assert!(registry.list().is_empty());
}

#[test]
fn budget_evicts_least_recently_used_first() {
    let (probe, _) = registry(3600, usize::MAX);
    let size = probe.load(project("probe")).unwrap().0.estimated_bytes();
    let (registry, clock) = registry(3600, size * 2);
    registry.load(project("a")).unwrap();
    clock.advance(Duration::from_secs(1));
    registry.load(project("b")).unwrap();
    clock.advance(Duration::from_secs(1));
    registry.get("a").unwrap();
    clock.advance(Duration::from_secs(1));
    let (_, evicted) = registry.load(project("c")).unwrap();
    assert_eq!(evicted, [Eviction::OverBudget("b".into())]);
    // Growth from writes counts once the next eviction pass runs.
    let a = registry.get("a").unwrap();
    let props = text(&"x".repeat(size));
    a.apply(&[create("big", NodeKind::Text, "f1", props)])
        .unwrap();
    clock.advance(Duration::from_secs(1));
    registry.get("c").unwrap();
    assert_eq!(registry.evict(), [Eviction::OverBudget("a".into())]);
    // A project larger than the whole budget still loads; others make room.
    let (registry, _) = self::registry(3600, 1);
    registry.load(project("x")).unwrap();
    let (_, evicted) = registry.load(project("y")).unwrap();
    assert_eq!(evicted, [Eviction::OverBudget("x".into())]);
    assert_eq!(registry.list().len(), 1);
}

#[test]
fn projects_lock_independently() {
    let (registry, _) = registry(60, usize::MAX);
    let registry = Arc::new(registry);
    registry.load(project("a")).unwrap();
    registry.load(project("b")).unwrap();
    let a = registry.get("a").unwrap();
    let reading_a = a.read();
    let (done, finished) = mpsc::channel();
    let other = registry.clone();
    let writer = std::thread::spawn(move || {
        let b = other.get("b").unwrap();
        let delta = b.apply(&[delete("t2")]).unwrap();
        done.send(delta.revision).unwrap();
    });
    let revision = finished.recv_timeout(Duration::from_secs(5));
    assert_eq!(revision, Ok(2), "writer on b blocked by a reader of a");
    assert_eq!(reading_a.revision(), 1);
    drop(reading_a);
    writer.join().unwrap();
}

#[test]
fn readers_see_whole_batches() {
    let (registry, _) = registry(60, usize::MAX);
    let (handle, _) = registry.load(project("a")).unwrap();
    let writer = {
        let handle = handle.clone();
        std::thread::spawn(move || {
            for i in 0..200 {
                let ops = [
                    create(&format!("x{i}"), NodeKind::Text, "f1", text("x")),
                    create(&format!("y{i}"), NodeKind::Text, "f1", text("x")),
                ];
                handle.apply(&ops).unwrap();
            }
        })
    };
    let readers: Vec<_> = (0..4)
        .map(|_| {
            let handle = handle.clone();
            std::thread::spawn(move || {
                for _ in 0..200 {
                    let state = handle.read();
                    let added = state.len() - 18;
                    assert_eq!(added as u64, 2 * (state.revision() - 1));
                }
            })
        })
        .collect();
    writer.join().unwrap();
    readers.into_iter().for_each(|r| r.join().unwrap());
    assert_eq!(handle.read().revision(), 201);
}
