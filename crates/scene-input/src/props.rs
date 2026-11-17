//! Borrowed typed access to common authored properties. JSON remains lossless
//! storage; these accessors neither normalize values nor supply validation defaults.
use serde_json::Value;

#[derive(Clone, Copy)]
pub struct NodeProps<'a>(&'a Value);

impl<'a> NodeProps<'a> {
    pub fn new(value: &'a Value) -> Self {
        Self(value)
    }

    pub fn component_id(self) -> Option<&'a str> {
        self.0.get("componentId").and_then(Value::as_str)
    }

    pub fn component(self) -> Option<&'a Value> {
        self.0.get("component")
    }

    pub fn name(self) -> Option<&'a str> {
        self.0.get("name").and_then(Value::as_str)
    }

    pub fn text(self) -> Option<&'a str> {
        self.0.get("text").and_then(Value::as_str)
    }

    pub fn link_targets(self) -> impl Iterator<Item = &'a str> {
        self.0
            .get("links")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|link| link.get("to").and_then(Value::as_str))
    }
}

impl crate::Node {
    pub fn typed_props(&self) -> NodeProps<'_> {
        NodeProps::new(&self.props)
    }
}
