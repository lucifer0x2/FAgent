//! Frame `get_design_context` latency, end to end through `reads::read`: today's
//! path (database snapshot only) cold and warm, and memory on first touch
//! (load) and warm. Medians over repeated reads; responses are compared too.
use super::Pair;
use crate::state::AppState;
use fagent_storage::Snapshot;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Instant;

const REPEATS: usize = 15;

/// The root frame with the median subtree size: a normal frame, not the widest.
pub(super) fn normal_root(snapshot: &Snapshot, roots: &[String]) -> String {
    let parent: HashMap<&str, &str> = snapshot
        .nodes
        .iter()
        .filter_map(|n| Some((n.id.as_str(), n.parent_id.as_deref()?)))
        .collect();
    let mut size: HashMap<&str, usize> = HashMap::new();
    for node in &snapshot.nodes {
        let mut id = node.id.as_str();
        while let Some(&up) = parent.get(id) {
            if roots.iter().any(|r| r == id) {
                *size.entry(id).or_default() += 1;
                break;
            }
            id = up;
        }
    }
    let mut ranked: Vec<_> = roots
        .iter()
        .map(|r| (size.get(r.as_str()).copied().unwrap_or(0), r))
        .collect();
    ranked.sort();
    ranked[ranked.len() / 2].1.clone()
}

/// One timed context read, on the database snapshot only when `database`.
async fn timed(state: &AppState, args: &Value, database: bool) -> u64 {
    let start = Instant::now();
    let value = match database {
        true => crate::reads::read_database(state, "get_design_context", args.clone()).await,
        false => crate::reads::read(state, "get_design_context", args.clone()).await,
    };
    assert!(value.is_ok(), "context read failed");
    start.elapsed().as_micros() as u64
}

async fn micros(state: &AppState, args: &Value) -> u64 {
    timed(state, args, false).await
}

async fn today(state: &AppState, args: &Value) -> u64 {
    timed(state, args, true).await
}

async fn warm(state: &AppState, args: &Value, database: bool) -> u64 {
    let mut all = Vec::with_capacity(REPEATS);
    for _ in 0..REPEATS {
        all.push(timed(state, args, database).await);
    }
    all.sort_unstable();
    all[REPEATS / 2]
}

/// Timings in µs for one frame, then one compared read.
pub(super) async fn frame_context(pair: &mut Pair, frame: &str) -> Value {
    let args = json!({"projectId": pair.project, "frameId": frame});
    let today_cold = today(&pair.off, &args).await;
    let today_warm = warm(&pair.off, &args, true).await;
    let memory_first = micros(&pair.on, &args).await;
    let memory_warm = warm(&pair.on, &args, false).await;
    let response = pair
        .one("get_design_context", json!({"frameId": frame}))
        .await;
    json!({
        "frameNodes": response["nodes"].as_array().map_or(0, Vec::len),
        "todayColdUs": today_cold, "todayWarmUs": today_warm,
        "memoryFirstTouchUs": memory_first, "memoryWarmUs": memory_warm,
    })
}

fn median(mut values: Vec<u64>) -> u64 {
    values.sort_unstable();
    values[values.len() / 2]
}

/// First read after each of a few small commits to the frame, each state reading
/// after its own write (a database read with new revision indexes, vs memory
/// published by the commit). With one shared database the states take turns, each after a warm-up
/// commit so memory does not start behind the other state's writes.
pub(super) async fn after_writes(pair: &mut Pair, frame: &str, shared_db: bool) -> Value {
    let args = json!({"projectId": pair.project, "frameId": frame});
    let id = pair.project.clone();
    let mut revision = pair
        .off
        .db(move |s| s.project_meta(&id))
        .await
        .unwrap()
        .revision;
    let turns: &[Option<bool>] = if shared_db {
        &[Some(false), Some(true)]
    } else {
        &[None]
    };
    let (mut today, mut memory) = (Vec::new(), Vec::new());
    for &turn in turns {
        pair.shared = turn;
        for i in 0..8 {
            let name = format!("timing {turn:?} {i}");
            let ops = json!([{"op":"update_node","id":frame,"props":{"name":name}}]);
            revision = pair
                .commit(
                    revision,
                    &format!("timing-{}-{i}", turn.map_or(2, u8::from)),
                    ops,
                )
                .await;
            let (off, on) = (turn != Some(true), turn != Some(false));
            if off && i > 0 {
                today.push(self::today(&pair.off, &args).await);
            }
            if on && i > 0 {
                memory.push(micros(&pair.on, &args).await);
            } else if on {
                micros(&pair.on, &args).await;
            }
        }
    }
    pair.one("get_design_context", json!({"frameId": frame}))
        .await;
    json!({"todayAfterWriteUs": median(today), "memoryAfterWriteUs": median(memory)})
}
