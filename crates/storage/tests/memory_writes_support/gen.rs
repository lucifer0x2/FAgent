//! Deterministic random batches from several agents with stale bases (Task28 I3).
use serde_json::{json, Value};

/// xorshift64*: deterministic and dependency-free.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    pub fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
    fn pick<'a>(&mut self, items: &'a [String]) -> &'a str {
        &items[self.below(items.len())]
    }
}

pub fn seed() -> Value {
    let frame = |id: &str, parent: &str, props: Value| json!({"op":"create_node","id":id,"kind":"frame","parentId":parent,"props":props});
    json!([
        {"op":"create_node","id":"pg","kind":"page","parentId":null,"props":{"name":"Home"}},
        {"op":"create_node","id":"pg2","kind":"page","parentId":null,"props":{"name":"Two"}},
        frame("f0", "pg", json!({"width":400,"height":400})),
        frame("f1", "pg", json!({"width":400,"height":400})),
        frame("f2", "pg2", json!({"width":400,"height":400})),
        frame("master", "pg2", json!({"width":40,"height":40,"component":{"name":"Card"}})),
        {"op":"create_node","id":"inst","kind":"instance","parentId":"f0","props":{"componentId":"master"}},
        {"op":"create_node","id":"r0","kind":"rectangle","parentId":"f0","props":{"width":10,"height":10}},
        {"op":"upsert_token","token":{"id":"t0","name":"T0","type":"color","value":"#112233"}}
    ])
}

/// Ids an agent may name: seeded, created (possibly deleted since) and invented.
pub struct Ids {
    pub nodes: Vec<String>,
    pub containers: Vec<String>,
    pub notes: Vec<String>,
    live: Vec<String>,
    live_containers: Vec<String>,
    live_notes: Vec<String>,
    next: usize,
}

impl Default for Ids {
    fn default() -> Self {
        let own = |ids: &[&str]| ids.iter().map(|s| s.to_string()).collect();
        Self {
            nodes: own(&["f0", "f1", "f2", "r0", "inst", "master", "ghost"]),
            containers: own(&["f0", "f1", "f2", "pg", "pg2"]),
            notes: vec![],
            live: vec![],
            live_containers: vec![],
            live_notes: vec![],
            next: 0,
        }
    }
}

impl Ids {
    /// Ids that exist now (from a snapshot), so most batches are valid; the pool of
    /// stale and invented ids stays in use for a few operations.
    pub fn refresh(&mut self, snapshot: &fagent_storage::Snapshot) {
        let live = |kinds: &[&str]| -> Vec<String> {
            snapshot
                .nodes
                .iter()
                .filter(|n| kinds.is_empty() || kinds.contains(&n.kind.as_str()))
                .map(|n| n.id.clone())
                .collect()
        };
        self.live = live(&["frame", "rectangle", "instance"]);
        self.live_containers = live(&["frame", "page"]);
        self.live_notes = snapshot.notes.iter().map(|n| n.id.clone()).collect();
    }

    fn node(&self, rng: &mut Rng) -> String {
        match rng.chance(88) && !self.live.is_empty() {
            true => rng.pick(&self.live).to_owned(),
            false => rng.pick(&self.nodes).to_owned(),
        }
    }

    fn container(&self, rng: &mut Rng) -> String {
        match rng.chance(90) && !self.live_containers.is_empty() {
            true => rng.pick(&self.live_containers).to_owned(),
            false => rng.pick(&self.containers).to_owned(),
        }
    }

    fn fresh(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}{}", self.next)
    }
}

fn props(rng: &mut Rng) -> Value {
    let mut out = serde_json::Map::new();
    for key in ["name", "width", "height", "opacity", "x"] {
        if rng.chance(40) {
            let value = match key {
                "name" => json!(format!("n{}", rng.below(5))),
                "opacity" => json!(rng.below(11) as f64 / 10.0),
                _ => json!(rng.below(300) + 1),
            };
            out.insert(key.into(), value);
        }
    }
    if out.is_empty() {
        out.insert("name".into(), json!("x"));
    }
    Value::Object(out)
}

/// One operation; some are invalid on purpose (missing ids, cycles, bad indexes,
/// components in use, unknown props) so rejections are compared too.
pub fn operation(rng: &mut Rng, ids: &mut Ids) -> Value {
    let index = |rng: &mut Rng| match rng.chance(30) {
        true => json!(rng.below(5)),
        false => Value::Null,
    };
    match rng.below(14) {
        0..=2 => {
            let id = ids.fresh("n");
            let frame = rng.chance(35);
            let parent = ids.container(rng);
            ids.nodes.push(id.clone());
            if frame {
                ids.containers.push(id.clone());
            }
            let kind = if frame { "frame" } else { "rectangle" };
            let mut op = json!({"op":"create_node","id":id,"kind":kind,"parentId":parent,"props":{"width":rng.below(90)+1,"height":12}});
            let at = index(rng);
            if !at.is_null() {
                op["index"] = at;
            }
            op
        }
        3..=6 => {
            json!({"op":"update_node","id":ids.node(rng),"props":props(rng),"merge":rng.chance(30)})
        }
        7 => {
            let mut op =
                json!({"op":"reparent_node","id":ids.node(rng),"parentId":ids.container(rng)});
            let at = index(rng);
            if !at.is_null() {
                op["index"] = at;
            }
            op
        }
        8 => json!({"op":"reorder_node","id":ids.node(rng),"index":rng.below(4)}),
        9 if rng.chance(50) => json!({"op":"delete_subtree","id":ids.node(rng)}),
        9 => json!({"op":"update_node","id":ids.node(rng),"props":props(rng)}),
        10 => {
            json!({"op":"upsert_token","token":{"id":format!("t{}", rng.below(3)),"name":"T","type":"color","value":format!("#{:06x}", rng.below(0xffffff))}})
        }
        11 => {
            let id = ids.fresh("note");
            ids.notes.push(id.clone());
            json!({"op":"create_note","id":id,"nodeId":ids.node(rng),"text":"why"})
        }
        12 if !ids.live_notes.is_empty() => {
            let note = rng.pick(&ids.live_notes).to_owned();
            match rng.chance(60) {
                true => json!({"op":"update_note","id":note,"text":format!("t{}", rng.below(9))}),
                false => json!({"op":"delete_note","id":note}),
            }
        }
        12 => json!({"op":"update_node","id":"r0","props":{"bogus":1}}),
        13 if rng.chance(70) => {
            json!({"op":"update_node","id":ids.node(rng),"props":props(rng),"merge":true})
        }
        _ => {
            json!({"op":"create_node","id":ids.fresh("i"),"kind":"instance","parentId":ids.container(rng),"props":{"componentId":"master"}})
        }
    }
}
