//! Random note, token and style operations for [`Gen`](super::random_ops::Gen).
use super::random_ops::{Gen, Pool};
use fagent_node_memory::Operation;
use fagent_scene_input::{Style, Token};
use serde_json::{json, Value};

impl Gen<'_> {
    pub(super) fn note(&mut self, pool: &mut Pool) -> Operation {
        self.counter += 1;
        let existing = match pool.notes.is_empty() || self.rng.chance(10) {
            true => "nomissing".to_owned(),
            false => pool.notes[self.rng.below(pool.notes.len())].clone(),
        };
        let text = ["hi", ""][usize::from(self.rng.chance(5))].to_owned();
        match self.rng.below(5) {
            0 | 1 => {
                let id = match self.rng.chance(5) {
                    true => self.pick(pool),
                    false => format!("note{}", self.counter),
                };
                pool.notes.push(id.clone());
                Operation::CreateNote {
                    id,
                    node_id: self.pick(pool),
                    text,
                    author: self.rng.chance(50).then(|| "me".into()),
                }
            }
            2 => Operation::ReplyNote {
                id: existing,
                text,
                author: None,
            },
            3 => Operation::UpdateNote {
                id: existing,
                text: self.rng.chance(30).then_some(text),
                resolved: self.rng.chance(30).then_some(true),
                node_id: self.rng.chance(30).then(|| self.pick(pool)),
            },
            _ => Operation::DeleteNote { id: existing },
        }
    }

    pub(super) fn resource(&mut self) -> Operation {
        let id = format!("res{}", self.rng.below(3));
        let value: Value = json!(format!("#00000{}", self.rng.below(9)));
        match self.rng.chance(50) {
            true => Operation::UpsertToken {
                token: Token {
                    id,
                    name: "Color".into(),
                    token_type: "color".into(),
                    value,
                },
            },
            false => Operation::UpsertStyle {
                style: Style {
                    id,
                    name: "Fill".into(),
                    kind: "fill".into(),
                    value,
                },
            },
        }
    }
}
