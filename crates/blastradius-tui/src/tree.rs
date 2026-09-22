//! The walk as a tree: each changed service is a root, and every reached
//! service hangs off the path that reached it. The engine keeps one path per
//! reached service; the tree is the union of those paths.
use blastradius::{Blast, Hop};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A changed service.
    Root,
    /// The end of a reached service's own path.
    Reached(usize),
    /// A service on another service's path, reported elsewhere in the tree
    /// by a path of its own.
    Via,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Box-drawing lead: `├── `, `│   └── `.
    pub prefix: String,
    pub service: String,
    /// The hop that led here; None for a root.
    pub hop: Option<Hop>,
    pub kind: Kind,
}

struct Node {
    service: String,
    hop: Option<Hop>,
    kind: Kind,
    children: Vec<Node>,
}

impl Node {
    fn child(&mut self, hop: &Hop) -> &mut Node {
        let at = if let Some(at) = self.children.iter().position(|c| c.service == hop.to) {
            at
        } else {
            self.children.push(Node {
                service: hop.to.clone(),
                hop: Some(hop.clone()),
                kind: Kind::Via,
                children: Vec::new(),
            });
            self.children.len() - 1
        };
        &mut self.children[at]
    }
}

/// Changed services in the report's order, each followed by what it reaches,
/// in the order the engine sorts reached services (depth, confidence, name).
pub fn rows(b: &Blast) -> Vec<Row> {
    let mut roots: Vec<Node> = b
        .changed
        .iter()
        .map(|c| Node {
            service: c.service.clone(),
            hop: None,
            kind: Kind::Root,
            children: Vec::new(),
        })
        .collect();
    for (i, r) in b.reached.iter().enumerate() {
        let Some(first) = r.path.first() else {
            continue;
        };
        let Some(root) = roots.iter_mut().find(|n| n.service == first.from) else {
            continue;
        };
        let mut node = root;
        for hop in &r.path {
            node = node.child(hop);
        }
        node.kind = Kind::Reached(i);
    }
    let mut out = Vec::new();
    for root in roots {
        out.push(Row {
            prefix: String::new(),
            service: root.service,
            hop: None,
            kind: Kind::Root,
        });
        flatten(&root.children, "", &mut out);
    }
    out
}

fn flatten(nodes: &[Node], lead: &str, out: &mut Vec<Row>) {
    for (i, node) in nodes.iter().enumerate() {
        let last = i + 1 == nodes.len();
        out.push(Row {
            prefix: format!("{lead}{}", if last { "└── " } else { "├── " }),
            service: node.service.clone(),
            hop: node.hop.clone(),
            kind: node.kind,
        });
        let next = format!("{lead}{}", if last { "    " } else { "│   " });
        flatten(&node.children, &next, out);
    }
}
