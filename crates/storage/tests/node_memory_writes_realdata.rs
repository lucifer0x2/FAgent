//! Task28 I3 real-data differential: two restores of the same native backup (never
//! the live cluster), one committing today's way and one through memory. Batches
//! built from real node ids go to both with stale, current, strict and scoped bases;
//! results and stored rows must be identical. Prints counts only.
//!
//! `WRITES_REALDATA_URLS='<url off>|<url on>' cargo test --release -p fagent-storage \
//!   --test node_memory_writes_realdata -- --ignored --nocapture`
//! Optional: `WRITES_REALDATA_BATCHES` per project (default 150).
mod memory_writes_support;
mod shadow_realdata;

use fagent_storage::*;
use memory_writes_support::{rows, Pair};
use serde_json::json;
use shadow_realdata::ops::{batch, Rng};
use std::sync::Arc;

fn storage(url: &str, on: bool) -> Storage {
    let mut config = PostgresConfig::new(url);
    let (shadow, reads, writes) = match on {
        true => (ShadowMode::Strict, ReadsMode::On, WritesMode::Strict),
        false => (ShadowMode::Off, ReadsMode::Off, WritesMode::Off),
    };
    config.node_memory_shadow = shadow;
    config.node_memory_reads = reads;
    config.node_memory_writes = writes;
    Storage::from_shared(Arc::new(PostgresBackend::with_config(config).unwrap()))
}

#[test]
#[ignore = "requires two disposable restores of a native backup"]
fn realdata_memory_writes_match_todays_path() {
    let urls = std::env::var("WRITES_REALDATA_URLS").expect("WRITES_REALDATA_URLS");
    let (off_url, on_url) = urls.split_once('|').unwrap();
    assert!(!urls.contains("2703"), "never the live cluster");
    let per_project = std::env::var("WRITES_REALDATA_BATCHES").map_or(150, |v| v.parse().unwrap());
    let projects = storage(off_url, false).list_projects().unwrap();
    let start = node_memory_shadow_stats();
    let (mut committed, mut rejected, mut concurrent) = (0, 0, 0);
    for meta in &projects {
        let (off, on) = (off_url.to_owned(), on_url.to_owned());
        let id = meta.project_id.clone();
        let pair = Pair {
            off: storage(off_url, false),
            on: storage(on_url, true),
            project: id.clone(),
            rows: Box::new(move |on_side| rows::postgres(if on_side { &on } else { &off }, &id)),
        };
        pair.assert_rows(0);
        let mut rng = Rng(meta.revision + 17);
        let mut history = vec![meta.revision];
        for step in 0..per_project {
            let snapshot = pair
                .off
                .open_project(&pair.project)
                .unwrap()
                .snapshot()
                .unwrap();
            let Some(b) = batch(&snapshot, step, step, &mut rng) else {
                continue;
            };
            // Mostly stale bases: an agent that read a few commits ago.
            let back = rng.below(4).min(history.len() - 1);
            let base = history[history.len() - 1 - back];
            let mut request = json!({"projectId":pair.project,"expectedRevision":base,
                "idempotencyKey":format!("i3-real-{step}-{}", b.label),"operations":b.ops});
            if rng.below(5) == 0 {
                request["strict"] = json!(true);
            }
            if let Some(pages) = b.pages {
                request["pageIds"] = json!(pages);
            }
            let result = pair.both(&request, false);
            match result.get("error") {
                Some(_) => rejected += 1,
                None => {
                    committed += 1;
                    concurrent += usize::from(result.get("concurrent").is_some());
                    history.push(result["committedRevision"].as_u64().unwrap());
                }
            }
            if step % 50 == 49 {
                pair.assert_rows(step);
            }
        }
        pair.assert_rows(per_project);
    }
    let end = node_memory_shadow_stats();
    println!(
        "writes-realdata projects={} committed={committed} rejected={rejected} concurrent={concurrent} memory_bases={} misses={} fallbacks={} mismatches={}",
        projects.len(),
        end.write_hits - start.write_hits,
        end.write_misses - start.write_misses,
        end.write_fallbacks - start.write_fallbacks,
        end.mismatches - start.mismatches,
    );
    assert!(committed > 0);
}
