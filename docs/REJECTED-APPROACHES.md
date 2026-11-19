# Rejected approaches

Check this list before adding a dependency, engine or storage design: each entry was tried, measured or evaluated and then removed, rejected or shelved.

## Rendering and export

- **tiny-skia render backend (`crates/render`, `RENDER_BACKEND=rust`)** — pure-Rust PNG export (HarfRust text + tiny-skia raster) built as a spike from 2026-10-29 to replace Node + Chromium. Removed 2026-11-17: `vello_cpu` replaced it and shares the encoder with the viewer's Vello engine. Gone from every `Cargo.toml`; do not reinstall it.
- **Rust render engine compiled to WASM for the viewer (former Tasks 09, 11-13)** — Task 09 (2026-11-04) matched native pixels byte for byte and stayed under the size gate, but missed the latency gates (1440x900 frame p50 624 ms against 200 ms). The viewer and default-switch tasks were never built. The Vello WASM engine (`?renderer=vello`, now the default) replaced the idea.
- **Rust driving Chromium over CDP** — considered 2026-10-29 as a way to drop Node. Rejected: not pure Rust and still needs Chromium. Never built.
- **Skia C++ (`skia-safe`), Cairo/Pango, Python rasterisers** — listed in the 2026-10-29 export study and rejected as not pure Rust. Never built; no benchmark exists.
- **`vello_gpu` for native server export** — rejected 2026-11-17: raster is under 40% of a job (at most 51 ms on the largest frame), GPU readback and device startup would cost more, and GPU float output breaks deterministic cache identity. `vello_gpu` runs only in the browser viewer.
- **Graphite (node-graph editor) as renderer or dependency** — source review only, 2026-11-04; nothing was built. Rejected: alpha, no CPU raster path, headless export needs a GPU adapter, no auto-layout, and its text line breaking does not match Chrome. Only the Vello library was kept from that review.
- **RustyBuzz / `ttf-parser`** — rejected as the text stack in the 2026-10-29 study (upstream archived or in maintenance mode); HarfRust is used instead (`crates/text`).

## Storage and realtime

- **Redis as hot read cache (bitcode snapshots)** — built and benchmarked 2026-11-02/03 (Tasks 17, 19), opt-in via `REDIS_URL`. Removed from the runtime 2026-11-03: the user switched to an in-process cache while the Redis 8 trial was still unfinished, and Redis did not reliably speed up writes (slower than delta-only at 100 nodes). An in-process Moka cache (TTL 1 hour, PostgreSQL only) replaced it, itself removed in Task 41 (see below). No crate depends on Redis today.
- **Redis as primary store with PostgreSQL write-behind (Task 18)** — shelved. Written down as a research backlog item on 2026-11-02 and never started; it needs a defined ACK, durability and recovery contract first.
- **Per-node delta writes with a single writer (Task 17 spike)** — measured 2026-11-02/03 in `spikes/realtime-db`. It reduced full-project loading for small edits but regressed under other loads, so it stayed a spike. The runtime backends keep their own revision, idempotency and history rules.
- **Node-memory shadow mirror and compare in production (Task 28 `SHADOW`)** — built 2026-11-16 to mirror every commit into RAM and compare it with the database path. Removed 2026-11-18 (Task 41 phase 1): node memory is the only path, so a second copy only cost RAM and commit time; parity lives in test oracles (`crates/node-memory/tests/differential.rs`).
- **Opt-in node-memory flags and modes (`FAGENT_NODE_MEMORY_SHADOW/READS/WRITES`, `off`/`on`/`strict`)** — Task 28, 2026-11-16. Removed 2026-11-18 (Task 41 phase 1): memory is always on; every mode doubled the paths to test and none was meant to stay. Do not reintroduce a switch.
- **Whole-project validation per commit (incremental `validate_after`/`validate_after_indexed`, the `classify::supports` classifier and its full fallback)** — Tasks 25 and 28. Even "incremental", it rebuilt indexes, scanned notes/navigation/triggers and expanded every instance on each commit (~210 ms at 200k nodes). Removed 2026-11-19 (Task 41 phases 3–4): per-op checks plus end-of-batch delta rules (`scene-input` `validate::delta`); `Candidate::validate` runs once per load (admission).
- **Latest-revision storage snapshot cache (PostgreSQL Moka `SnapshotCache`, TTL 1 hour) and the `RevisionIndexes` Moka cache (`IndexCache`)** — Tasks 19 and 25, replacing the Redis cache above. Removed 2026-11-19 (Task 41 phase 4): node memory serves latest-revision full and scoped reads, so a second typed copy per project was redundant; revision indexes are built per database read. The server's HTTP response cache (encoded bytes, Moka) stays.

## Still in use, being phased out

- **Chromium/Playwright export worker** — not rejected yet: still the default export backend (`FAGENT_RENDER_BACKEND=chromium`, Node worker + `renderer/` Canvas2D) and the pixel reference; the `vello` build has no Node or Chromium at runtime (Task 34). Do not extend it with new features.
- **Konva/Canvas2D viewer drawing** — the fallback since Vello became the default on 2026-11-17; used for `?renderer=canvas2d` and server scenes. Do not build new viewer features on it.
