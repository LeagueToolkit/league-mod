//! Structural discovery retains duplicate keys in refused manifest and source files.

use serde::{
    Deserialize, Deserializer,
    de::{MapAccess, SeqAccess, Visitor},
};

pub(crate) enum Node {
    Map(Vec<(Node, Node)>),
    Sequence(Vec<Node>),
    Text(String),
    Scalar,
}

impl Node {
    fn fields(&self, name: &str) -> impl Iterator<Item = &Node> {
        let entries = match self {
            Self::Map(entries) => entries.as_slice(),
            _ => &[],
        };
        entries.iter().filter_map(move |(key, value)| {
            matches!(key, Self::Text(key) if key == name).then_some(value)
        })
    }

    /// The text items of every `overrides` list of a body: the compact body's own and each
    /// edit's under the `edits` key.
    fn body_overrides(&self, paths: &mut Vec<String>) {
        self.texts("overrides", paths);
        for edits in self.fields("edits") {
            if let Self::Sequence(edits) = edits {
                for edit in edits {
                    edit.texts("overrides", paths);
                }
            }
        }
    }

    /// The text items of every list under `name`, appended to `paths`.
    fn texts(&self, name: &str, paths: &mut Vec<String>) {
        for list in self.fields(name) {
            if let Self::Sequence(items) = list {
                paths.extend(items.iter().filter_map(|item| match item {
                    Self::Text(path) => Some(path.clone()),
                    _ => None,
                }));
            }
        }
    }

    /// Every override path of a manifest or source file, as spelled, in document order.
    pub(crate) fn overrides(&self) -> Vec<String> {
        let mut paths = Vec::new();
        self.body_overrides(&mut paths);
        for modules in self.fields("modules") {
            if let Self::Sequence(modules) = modules {
                for module in modules {
                    module.body_overrides(&mut paths);
                }
            }
        }
        paths
    }

    pub(crate) fn sources(&self) -> Vec<String> {
        let mut paths = Vec::new();
        for modules in self.fields("modules") {
            if let Self::Sequence(modules) = modules {
                for module in modules {
                    for source in module.fields("source") {
                        if let Self::Text(path) = source {
                            paths.push(path.clone());
                        }
                    }
                }
            }
        }
        paths
    }
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct NodeVisitor;
        impl<'de> Visitor<'de> for NodeVisitor {
            type Value = Node;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a declaration value")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
                let mut entries = Vec::new();
                while let Some(pair) = map.next_entry()? {
                    entries.push(pair);
                }
                Ok(Node::Map(entries))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Node, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element()? {
                    values.push(value);
                }
                Ok(Node::Sequence(values))
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Node, E> {
                Ok(Node::Text(value.to_owned()))
            }
            fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Node, E> {
                Ok(Node::Text(value))
            }
            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Node, E> {
                Ok(Node::Scalar)
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Node, E> {
                Ok(Node::Scalar)
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Node, E> {
                Ok(Node::Scalar)
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Node, E> {
                Ok(Node::Scalar)
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Node, E> {
                Ok(Node::Scalar)
            }
        }
        deserializer.deserialize_any(NodeVisitor)
    }
}
