//! A YAML tree that remembers the line of every node, so evidence can point
//! at the exact declaration. Built on libyaml-safer, a port of libyaml, so
//! it accepts what Docker Compose and Kubernetes accept: the yaml-rust
//! family rejects a flow sequence whose closing bracket sits at the key's
//! indentation, and the OpenTelemetry demo's compose file does exactly that.
//! Anchors, aliases and `<<` merge keys resolve; explicit keys beat merged
//! ones. Parsing stops at the first error: documents completed before it are
//! kept, later documents in the same file are lost.
use std::collections::HashMap;

use libyaml_safer::{EventData, Parser, ScalarStyle};

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// 1-based line of the node's first token.
    pub line: u32,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Null,
    Bool(bool),
    /// Numbers keep their source text, so `image: 3.7` renders as written.
    Number(String),
    Str(String),
    Seq(Vec<Node>),
    Map(Vec<(Node, Node)>),
}

impl Node {
    /// Value for a string key in a mapping; the first match wins.
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.entries()
            .iter()
            .find(|(k, _)| k.as_str() == Some(key))
            .map(|(_, v)| v)
    }

    /// Only genuine strings. Numbers and booleans are not coerced.
    pub fn as_str(&self) -> Option<&str> {
        match &self.kind {
            Kind::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Any scalar rendered as text: strings, numbers, booleans.
    pub fn as_scalar_string(&self) -> Option<String> {
        match &self.kind {
            Kind::Str(s) | Kind::Number(s) => Some(s.clone()),
            Kind::Bool(b) => Some(b.to_string()),
            Kind::Null | Kind::Seq(_) | Kind::Map(_) => None,
        }
    }

    /// Mapping entries in source order; empty for anything else.
    pub fn entries(&self) -> &[(Node, Node)] {
        match &self.kind {
            Kind::Map(entries) => entries,
            _ => &[],
        }
    }

    /// Sequence items; empty for anything else.
    pub fn items(&self) -> &[Node] {
        match &self.kind {
            Kind::Seq(items) => items,
            _ => &[],
        }
    }

    pub fn is_map(&self) -> bool {
        matches!(self.kind, Kind::Map(_))
    }
}

/// Every document that parsed.
pub fn parse_documents(text: &str) -> Vec<Node> {
    // libyaml-safer 0.3 panics on a block scalar that runs to the end of the
    // input without a final newline (sock-shop's Prometheus ConfigMaps).
    // A trailing newline never changes what YAML means, so add one; the
    // unwind boundary is the backstop for inputs nobody has seen yet.
    let mut owned;
    let text = if text.ends_with('\n') {
        text
    } else {
        owned = String::with_capacity(text.len() + 1);
        owned.push_str(text);
        owned.push('\n');
        &owned
    };
    std::panic::catch_unwind(|| load(text)).unwrap_or_default()
}

fn load(text: &str) -> Vec<Node> {
    let mut input = text.as_bytes();
    let mut parser = Parser::new();
    parser.set_input_string(&mut input);
    let mut loader = Loader::default();
    // On error the documents finished so far are what we have.
    while let Ok(event) = parser.parse() {
        let line = u32::try_from(event.start_mark.line + 1).unwrap_or(u32::MAX);
        if loader.on_event(event.data, line) {
            break;
        }
    }
    loader.docs
}

enum Entry {
    Explicit(Node, Node),
    Merged(Vec<(Node, Node)>),
}

enum Frame {
    Seq {
        line: u32,
        items: Vec<Node>,
        anchor: Option<String>,
    },
    Map {
        line: u32,
        entries: Vec<Entry>,
        pending_key: Option<Node>,
        anchor: Option<String>,
    },
}

impl Frame {
    fn finish(self) -> (Node, Option<String>) {
        match self {
            Self::Seq {
                line,
                items,
                anchor,
            } => (
                Node {
                    line,
                    kind: Kind::Seq(items),
                },
                anchor,
            ),
            Self::Map {
                line,
                entries,
                anchor,
                ..
            } => (
                Node {
                    line,
                    kind: Kind::Map(flatten_entries(entries)),
                },
                anchor,
            ),
        }
    }
}

/// Merged entries land where the `<<` key stood, except keys the mapping
/// also states explicitly anywhere: those keep the explicit value.
fn flatten_entries(entries: Vec<Entry>) -> Vec<(Node, Node)> {
    let explicit: Vec<Option<String>> = entries
        .iter()
        .filter_map(|e| match e {
            Entry::Explicit(k, _) => Some(k.as_scalar_string()),
            Entry::Merged(_) => None,
        })
        .collect();
    let mut out: Vec<(Node, Node)> = Vec::new();
    for entry in entries {
        match entry {
            Entry::Explicit(k, v) => out.push((k, v)),
            Entry::Merged(merged) => {
                for (k, v) in merged {
                    let key = k.as_scalar_string();
                    let stated = explicit.contains(&key)
                        || out
                            .iter()
                            .any(|(existing, _)| existing.as_scalar_string() == key);
                    if !stated {
                        out.push((k, v));
                    }
                }
            }
        }
    }
    out
}

#[derive(Default)]
struct Loader {
    docs: Vec<Node>,
    stack: Vec<Frame>,
    anchors: HashMap<String, Node>,
    root: Option<Node>,
}

impl Loader {
    fn remember(&mut self, anchor: Option<String>, node: &Node) {
        if let Some(anchor) = anchor {
            self.anchors.insert(anchor, node.clone());
        }
    }

    fn push_node(&mut self, node: Node) {
        match self.stack.last_mut() {
            None => self.root = Some(node),
            Some(Frame::Seq { items, .. }) => items.push(node),
            Some(Frame::Map {
                entries,
                pending_key,
                ..
            }) => match pending_key.take() {
                None => *pending_key = Some(node),
                Some(key) if key.as_str() == Some("<<") => {
                    entries.push(Entry::Merged(merge_source(node)));
                }
                Some(key) => entries.push(Entry::Explicit(key, node)),
            },
        }
    }

    /// Returns true at the end of the stream.
    fn on_event(&mut self, event: EventData, line: u32) -> bool {
        match event {
            EventData::StreamEnd => return true,
            EventData::StreamStart { .. } => {}
            EventData::DocumentStart { .. } => self.root = None,
            EventData::DocumentEnd { .. } => {
                if let Some(root) = self.root.take() {
                    self.docs.push(root);
                }
            }
            EventData::Scalar {
                anchor,
                tag,
                value,
                style,
                ..
            } => {
                let node = Node {
                    line,
                    kind: scalar_kind(value, style, tag.as_deref()),
                };
                self.remember(anchor, &node);
                self.push_node(node);
            }
            EventData::SequenceStart { anchor, .. } => self.stack.push(Frame::Seq {
                line,
                items: Vec::new(),
                anchor,
            }),
            EventData::MappingStart { anchor, .. } => self.stack.push(Frame::Map {
                line,
                entries: Vec::new(),
                pending_key: None,
                anchor,
            }),
            EventData::SequenceEnd | EventData::MappingEnd => {
                if let Some(frame) = self.stack.pop() {
                    let (node, anchor) = frame.finish();
                    self.remember(anchor, &node);
                    self.push_node(node);
                }
            }
            EventData::Alias { anchor } => {
                let node = self.anchors.get(&anchor).cloned().unwrap_or(Node {
                    line,
                    kind: Kind::Null,
                });
                self.push_node(node);
            }
        }
        false
    }
}

/// The entries a `<<` value contributes: a mapping, or a sequence of mappings
/// where earlier ones win.
fn merge_source(node: Node) -> Vec<(Node, Node)> {
    match node.kind {
        Kind::Map(entries) => entries,
        Kind::Seq(items) => {
            let mut out: Vec<(Node, Node)> = Vec::new();
            for item in items {
                if let Kind::Map(entries) = item.kind {
                    for (k, v) in entries {
                        let key = k.as_scalar_string();
                        if !out.iter().any(|(e, _)| e.as_scalar_string() == key) {
                            out.push((k, v));
                        }
                    }
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

/// Tags look like `tag:yaml.org,2002:str`; only the last segment matters.
fn scalar_kind(value: String, style: ScalarStyle, tag: Option<&str>) -> Kind {
    if let Some(tag) = tag {
        let suffix = tag.rsplit(':').next().unwrap_or(tag);
        return match suffix {
            "int" | "float" => Kind::Number(value),
            "bool" => Kind::Bool(matches!(value.as_str(), "true" | "True" | "TRUE")),
            "null" => Kind::Null,
            _ => Kind::Str(value),
        };
    }
    if style != ScalarStyle::Plain {
        return Kind::Str(value);
    }
    match value.as_str() {
        "" | "~" | "null" | "Null" | "NULL" => Kind::Null,
        "true" | "True" | "TRUE" => Kind::Bool(true),
        "false" | "False" | "FALSE" => Kind::Bool(false),
        _ if looks_numeric(&value) => Kind::Number(value),
        _ => Kind::Str(value),
    }
}

/// Decimal integers and floats, with an optional exponent. Nothing else is
/// a number, so a service called `inf` stays a string.
fn looks_numeric(s: &str) -> bool {
    let body = s.strip_prefix(['-', '+']).unwrap_or(s);
    let (mantissa, exponent) = match body.split_once(['e', 'E']) {
        Some((m, e)) => (m, Some(e)),
        None => (body, None),
    };
    let digits_ok = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok = match mantissa.split_once('.') {
        Some((int, frac)) => {
            (int.is_empty() || digits_ok(int))
                && (frac.is_empty() || digits_ok(frac))
                && !(int.is_empty() && frac.is_empty())
        }
        None => digits_ok(mantissa),
    };
    let exponent_ok = exponent.is_none_or(|e| digits_ok(e.strip_prefix(['-', '+']).unwrap_or(e)));
    mantissa_ok && exponent_ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn keys_carry_their_line() {
        let docs =
            parse_documents("services:\n  checkout:\n    build: ./c\n  orders:\n    image: x\n");
        let services = docs[0].get("services").unwrap();
        let names: Vec<(&str, u32)> = services
            .entries()
            .iter()
            .map(|(k, _)| (k.as_str().unwrap(), k.line))
            .collect();
        assert_eq!(names, vec![("checkout", 2), ("orders", 4)]);
        assert_eq!(docs[0].line, 1);
    }

    #[test]
    fn multiple_documents_and_document_line() {
        let docs = parse_documents("---\napiVersion: v1\nkind: Service\n---\n\nkind: Deployment\n");
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].get("kind").unwrap().as_str(), Some("Service"));
        assert_eq!(docs[0].line, 2);
        assert_eq!(docs[1].line, 6);
    }

    #[test]
    fn anchors_aliases_and_merge_keys_resolve() {
        let text = "x-env: &env\n  A: '1'\n  B: '2'\nservices:\n  a:\n    environment:\n      <<: *env\n      C: '3'\n  b:\n    environment: *env\n";
        let docs = parse_documents(text);
        let a = docs[0]
            .get("services")
            .unwrap()
            .get("a")
            .unwrap()
            .get("environment")
            .unwrap();
        let keys: Vec<&str> = a
            .entries()
            .iter()
            .map(|(k, _)| k.as_str().unwrap())
            .collect();
        assert_eq!(keys, vec!["A", "B", "C"]);
        let b = docs[0]
            .get("services")
            .unwrap()
            .get("b")
            .unwrap()
            .get("environment")
            .unwrap();
        assert_eq!(b.get("B").unwrap().as_str(), Some("2"));
    }

    #[test]
    fn block_scalar_at_end_of_input_without_newline_parses() {
        let docs = parse_documents("data:\n  rules: |-\n    a: 1\n    b: 2");
        assert_eq!(
            docs[0].get("data").unwrap().get("rules").unwrap().as_str(),
            Some("a: 1\nb: 2")
        );
        let docs = parse_documents("data:\n  cfg: |\n    x");
        assert_eq!(
            docs[0].get("data").unwrap().get("cfg").unwrap().as_str(),
            Some("x\n")
        );
    }

    #[test]
    fn flow_sequence_closed_at_key_indentation_parses() {
        // Docker Compose accepts this; the yaml-rust family does not.
        let docs = parse_documents(
            "svc:\n  command: [\n    \"start\",\n    \"--uri\"\n  ]\n  ports:\n    - \"80\"\n",
        );
        let svc = docs[0].get("svc").unwrap();
        assert_eq!(svc.get("command").unwrap().items().len(), 2);
        assert_eq!(svc.get("ports").unwrap().items()[0].as_str(), Some("80"));
    }

    #[test]
    fn broken_yaml_yields_no_documents_and_scalars_render() {
        assert!(parse_documents("a: [unclosed").is_empty());
        let docs = parse_documents("image: 3.7\nport: 8080\nflag: true\nnothing: ~\n");
        assert_eq!(
            docs[0].get("image").unwrap().as_scalar_string(),
            Some("3.7".into())
        );
        assert_eq!(docs[0].get("port").unwrap().as_str(), None);
        assert_eq!(
            docs[0].get("port").unwrap().as_scalar_string(),
            Some("8080".into())
        );
        assert_eq!(docs[0].get("flag").unwrap().kind, Kind::Bool(true));
        assert_eq!(docs[0].get("nothing").unwrap().kind, Kind::Null);
        assert!(!docs[0].get("port").unwrap().is_map());
        assert!(docs[0].is_map());
    }
}
