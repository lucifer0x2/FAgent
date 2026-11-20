//! `check_token_write` against `validate_tokens` after the same write, on generated
//! valid token sets: same acceptance and the same error (code, field and message).
use super::super::{validate_tokens, Candidate};
use super::check_token_write;
use crate::{Snapshot, Token};
use serde_json::{json, Value};

/// A small deterministic generator (64-bit LCG).
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n
    }
}

fn empty() -> Candidate {
    let snapshot: Snapshot = serde_json::from_value(json!({
        "schemaVersion":4,"projectId":"p","slug":"p","name":"p","revision":1,
        "retention":{"oldestCommitRevision":0,"idempotencyPolicy":"bounded"},
        "tokens":[],"styles":[],"assets":[],"fonts":[],"notes":[],"nodes":[]
    }))
    .unwrap();
    Candidate::from_snapshot(&snapshot)
}

fn id(i: usize) -> String {
    format!("t{i:02}")
}

/// A branch value: mostly an alias (in a base, of one of the three tokens below, so
/// chains grow long; in a write, of any id), else a literal.
fn branch(rng: &mut Rng, i: usize, kind: &str, upward: bool) -> Value {
    if rng.below(10) < 7 && (upward || i > 0) {
        let target = match upward {
            true => rng.below(48),
            false => i - 1 - rng.below(i.min(3)),
        };
        return json!({ "aliasId": id(target) });
    }
    match kind {
        "color" => json!("#102030"),
        _ => json!(4),
    }
}

fn token(rng: &mut Rng, i: usize, kind: &str, upward: bool) -> Token {
    let value = match rng.below(6) {
        0 => {
            let mut themes = serde_json::Map::new();
            let names = match rng.below(12) {
                0 => 20,
                _ => 1 + rng.below(3),
            };
            let offset = rng.below(30);
            for n in 0..names {
                themes.insert(format!("th{}", offset + n), branch(rng, i, kind, upward));
            }
            json!({"default": branch(rng, i, kind, upward), "themes": themes})
        }
        _ => branch(rng, i, kind, upward),
    };
    Token {
        id: id(i),
        name: format!("Token {i}"),
        token_type: kind.into(),
        value,
    }
}

fn kind(rng: &mut Rng) -> &'static str {
    match rng.below(8) {
        0 => "number",
        _ => "color",
    }
}

fn outcome(result: crate::Result<impl Sized>) -> Value {
    match result {
        Ok(_) => Value::Null,
        Err(error) => serde_json::to_value(error).unwrap(),
    }
}

#[test]
fn writes_match_validate_tokens() {
    let mut rng = Rng(7);
    let (mut accepted, mut rejected, mut graph) = (0, 0, 0);
    for _ in 0..300 {
        let mut base = empty();
        for i in 0..40 {
            let token = token(&mut rng, i, "color", false);
            base.tokens.insert(token.id.clone(), token);
            if validate_tokens(&base).is_err() {
                base.tokens.remove(&id(i));
            }
        }
        for _ in 0..20 {
            let i = rng.below(48);
            let kind = kind(&mut rng);
            let write = token(&mut rng, i, kind, true);
            let mut after = base.clone();
            let previous = after.tokens.insert(write.id.clone(), write.clone());
            let expected = outcome(validate_tokens(&after));
            let actual = outcome(check_token_write(&after, &write.id, previous.as_ref()));
            assert_eq!(actual, expected, "write {write:?}");
            match expected {
                Value::Null => accepted += 1,
                error => {
                    rejected += 1;
                    graph += usize::from(error["message"].as_str().unwrap().contains("alias "));
                }
            }
        }
    }
    assert!(
        accepted > 500 && rejected > 500 && graph > 100,
        "{accepted} {rejected} {graph}"
    );
}

fn put(candidate: &mut Candidate, i: usize, target: Option<usize>) -> Option<Token> {
    let token = Token {
        id: id(i),
        name: id(i),
        token_type: "color".into(),
        value: target.map_or(json!("#000000"), |t| json!({ "aliasId": id(t) })),
    };
    candidate.tokens.insert(token.id.clone(), token)
}

/// `t{i}` aliasing `t{i+1}` for `i` in `from..to`.
fn chain(candidate: &mut Candidate, from: usize, to: usize) {
    for i in from..to {
        put(candidate, i, Some(i + 1));
    }
}

/// `t{from}` rewritten to alias `t{to}`: the per-write outcome equals the full walk's.
fn rewrite(base: &Candidate, from: usize, to: usize) -> String {
    let mut after = base.clone();
    let previous = put(&mut after, from, Some(to));
    let expected = outcome(validate_tokens(&after));
    let actual = outcome(check_token_write(&after, &id(from), previous.as_ref()));
    assert_eq!(actual, expected);
    expected["message"].as_str().unwrap_or("").into()
}

/// Cycles and paths longer than 32 edges through the written token. The full walk
/// starts at the smallest id: from `t00` it walks 22 tokens into the cycle the write
/// closes and passes the depth limit before it meets the cycle, so that is the error.
#[test]
fn graph_faults_report_the_full_walks_error() {
    let mut base = empty();
    chain(&mut base, 0, 20);
    put(&mut base, 20, Some(60));
    chain(&mut base, 30, 50);
    put(&mut base, 50, Some(60));
    put(&mut base, 60, None);
    assert!(validate_tokens(&base).is_ok());
    assert!(rewrite(&base, 60, 30).contains("depth"));
    assert!(rewrite(&base, 50, 40).contains("cycle"));
    assert!(rewrite(&base, 60, 10).contains("cycle"));
    let mut tail = base.clone();
    chain(&mut tail, 61, 75);
    put(&mut tail, 75, None);
    assert!(rewrite(&tail, 60, 61).contains("depth"));
    assert!(rewrite(&tail, 60, 70).is_empty());
}
