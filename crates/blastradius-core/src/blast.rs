//! Stage 4: the blast radius. Given a change, which services can it reach?
//!
//! A change is a set of files. Each file belongs to the service whose root
//! holds it; those services seed a walk that follows dependency edges
//! outward to a bounded depth. Every reached service carries the depth it
//! was reached at, the weakest confidence on the path that reached it, and
//! that path. Everything else is "not in the computed blast radius": the
//! tool never says a service cannot be affected.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::discover::compare_service_names;
use crate::graph::BlastGraph;
use crate::model::{Confidence, EdgeType, ServiceRole};

/// The wording is fixed: the tool reports what it did not find, never what
/// cannot happen.
pub const NOT_REACHED: &str =
    "Not in the computed blast radius - no static or observed runtime path found";

/// Hops the walk follows unless `--depth` says otherwise.
pub const DEFAULT_DEPTH: usize = 3;

/// Where the changed files came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Pr,
    /// One commit, when a history has no pull request markers.
    Commit,
    Diff,
    Files,
}

/// The change under analysis: its files and where they were read from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub kind: ChangeKind,
    /// `#481`, the diff range, or `3 files`.
    pub reference: String,
    /// Which rule found a pull request: `merge commit 7d13248`, `ref refs/pull/481/head at ab12cd3`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub how: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Committer date, `YYYY-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// Relative to the analysed root, POSIX separators, in git's order.
    pub files: Vec<String>,
    /// Files git listed that lie outside the analysed root.
    pub outside_root: usize,
}

impl Change {
    /// Files named on the command line. `./` prefixes are dropped; nothing is
    /// checked against the file system, so a deleted file still counts.
    pub fn from_files(files: &[String]) -> Self {
        let files: Vec<String> = files.iter().map(|f| clean(f)).collect();
        let noun = if files.len() == 1 { "file" } else { "files" };
        Self {
            kind: ChangeKind::Files,
            reference: format!("{} {noun}", files.len()),
            how: None,
            title: None,
            date: None,
            files,
            outside_root: 0,
        }
    }
}

fn clean(path: &str) -> String {
    let mut p = path.replace('\\', "/");
    while let Some(rest) = p.strip_prefix("./") {
        p = rest.to_string();
    }
    p.trim_end_matches('/').to_string()
}

/// Why one service reaches another, from the reached service's side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Relation {
    /// `to` calls `from` over HTTP or gRPC.
    Calls,
    /// `to` imports a package `from` provides.
    Imports,
    /// `to` reads or writes a database `from` also uses.
    SharesDatabase,
    /// `to` consumes events `from` publishes.
    Consumes,
    /// `to` talks to the same broker `from` publishes to; the topic is unknown.
    SharesBroker,
}

impl Relation {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Calls => "calls",
            Self::Imports => "imports",
            Self::SharesDatabase => "shares-database",
            Self::Consumes => "consumes",
            Self::SharesBroker => "shares-broker",
        }
    }
}

/// One step of the walk: `from` is already in the radius, `to` is what it
/// reaches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hop {
    pub from: String,
    pub to: String,
    pub relation: Relation,
    #[serde(rename = "type")]
    pub edge_type: EdgeType,
    pub confidence: Confidence,
    /// The broker, for `shares-broker`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    /// Call count when the edge was observed in production.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calls: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reached {
    pub service: String,
    pub depth: usize,
    /// The weakest hop on the path below.
    pub confidence: Confidence,
    /// From a changed service outward.
    pub path: Vec<Hop>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Changed {
    pub service: String,
    pub files: Vec<String>,
}

/// Infrastructure on the path: a broker a changed service publishes to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Touched {
    pub service: String,
    /// The changed service that publishes to it.
    pub via: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByConfidence {
    pub observed: usize,
    #[serde(rename = "static")]
    pub static_: usize,
    pub inferred: usize,
    pub uncertain: usize,
}

impl ByConfidence {
    pub fn count(reached: &[Reached]) -> Self {
        let mut by = Self::default();
        for r in reached {
            match r.confidence {
                Confidence::Observed => by.observed += 1,
                Confidence::Static => by.static_ += 1,
                Confidence::Inferred => by.inferred += 1,
                Confidence::Uncertain => by.uncertain += 1,
            }
        }
        by
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub changed: usize,
    /// Code services reached; the changed ones are not counted.
    pub reached: usize,
    pub by_confidence: ByConfidence,
}

/// The walk's result, independent of where the seeds came from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Radius {
    pub reached: Vec<Reached>,
    pub infrastructure: Vec<Touched>,
    pub not_reached: Vec<String>,
}

/// The blast radius of one change. Field names are the JSON contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Blast {
    pub change: Change,
    pub depth: usize,
    pub changed: Vec<Changed>,
    /// Changed files no service owns. They seed nothing.
    pub unowned: Vec<String>,
    /// By depth, then confidence (strongest first), then name.
    pub reached: Vec<Reached>,
    pub infrastructure: Vec<Touched>,
    /// Code services neither changed nor reached, in service order.
    pub not_reached: Vec<String>,
    pub summary: Summary,
}

/// The services that own `file`: the longest root that holds it wins, and
/// every service declaring that root owns it. The root `.` (the
/// single-service fallback) owns everything but loses to any other root.
pub fn owners(graph: &BlastGraph, file: &str) -> Vec<String> {
    let file = clean(file);
    let mut best_len: Option<usize> = None;
    let mut best: Vec<String> = Vec::new();
    for service in graph.services() {
        let Some(root) = service.root.as_deref() else {
            continue;
        };
        let len = if root == "." {
            0
        } else if file == root || file.starts_with(&format!("{root}/")) {
            root.len()
        } else {
            continue;
        };
        match best_len {
            Some(current) if current > len => {}
            Some(current) if current == len => best.push(service.name.clone()),
            _ => {
                best_len = Some(len);
                best = vec![service.name.clone()];
            }
        }
    }
    best.sort_by(|a, b| compare_service_names(a, b));
    best
}

/// Groups the changed files by owning service. Files nobody owns are
/// returned second, in their original order.
pub fn seeds(graph: &BlastGraph, files: &[String]) -> (Vec<Changed>, Vec<String>) {
    let mut by_service: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut unowned: Vec<String> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for file in files {
        let file = clean(file);
        if !seen.insert(file.clone()) {
            continue;
        }
        let owners = owners(graph, &file);
        if owners.is_empty() {
            unowned.push(file);
            continue;
        }
        for owner in owners {
            by_service.entry(owner).or_default().push(file.clone());
        }
    }
    let mut changed: Vec<Changed> = by_service
        .into_iter()
        .map(|(service, files)| Changed { service, files })
        .collect();
    changed.sort_by(|a, b| compare_service_names(&a.service, &b.service));
    (changed, unowned)
}

struct Label {
    confidence: Confidence,
    depth: usize,
    path: Vec<Hop>,
}

struct State {
    node: String,
    confidence: Confidence,
    depth: usize,
    path: Vec<Hop>,
}

fn weaker(a: Confidence, b: Confidence) -> Confidence {
    if a.rank() <= b.rank() { a } else { b }
}

/// Walks outward from `seeds` for at most `max_depth` hops. Seeds are never
/// reported as reached. Unknown seed names are ignored.
pub fn radius(graph: &BlastGraph, seeds: &[String], max_depth: usize) -> Radius {
    let seed_set: BTreeSet<&str> = seeds
        .iter()
        .map(String::as_str)
        .filter(|s| graph.has_service(s))
        .collect();
    let mut labels: BTreeMap<String, Vec<Label>> = BTreeMap::new();
    let mut touched: BTreeMap<String, String> = BTreeMap::new();
    let mut queue: VecDeque<State> = seed_set
        .iter()
        .map(|s| State {
            node: (*s).to_string(),
            confidence: Confidence::Observed,
            depth: 0,
            path: Vec::new(),
        })
        .collect();

    while let Some(state) = queue.pop_front() {
        if state.depth >= max_depth {
            continue;
        }
        for hop in steps(graph, &state.node, state.depth == 0, &mut touched) {
            if seed_set.contains(hop.to.as_str()) {
                continue;
            }
            let confidence = weaker(state.confidence, hop.confidence);
            let depth = state.depth + 1;
            let known = labels.entry(hop.to.clone()).or_default();
            if known
                .iter()
                .any(|l| l.confidence.rank() >= confidence.rank() && l.depth <= depth)
            {
                continue;
            }
            known.retain(|l| !(l.confidence.rank() <= confidence.rank() && l.depth >= depth));
            let mut path = state.path.clone();
            path.push(hop.clone());
            known.push(Label {
                confidence,
                depth,
                path: path.clone(),
            });
            queue.push_back(State {
                node: hop.to,
                confidence,
                depth,
                path,
            });
        }
    }

    let mut reached: Vec<Reached> = labels
        .into_iter()
        .filter_map(|(service, labels)| {
            let best = labels.into_iter().max_by(|a, b| {
                a.confidence
                    .rank()
                    .cmp(&b.confidence.rank())
                    .then_with(|| b.depth.cmp(&a.depth))
            })?;
            Some(Reached {
                service,
                depth: best.depth,
                confidence: best.confidence,
                path: best.path,
            })
        })
        .collect();
    reached.sort_by(|a, b| {
        a.depth
            .cmp(&b.depth)
            .then_with(|| b.confidence.rank().cmp(&a.confidence.rank()))
            .then_with(|| compare_service_names(&a.service, &b.service))
    });
    let reached_names: BTreeSet<&str> = reached.iter().map(|r| r.service.as_str()).collect();
    let not_reached = graph
        .services()
        .into_iter()
        .filter(|s| s.role == ServiceRole::Code)
        .filter(|s| !seed_set.contains(s.name.as_str()) && !reached_names.contains(s.name.as_str()))
        .map(|s| s.name)
        .collect();
    Radius {
        reached,
        infrastructure: touched
            .into_iter()
            .map(|(service, via)| Touched { service, via })
            .collect(),
        not_reached,
    }
}

/// The hops the walk takes from `node`. Inbound `http`, `grpc`, `import` and
/// `database` edges reach their source: something that depends on `node`.
/// Outbound `event` edges reach their consumer. An outbound `event` edge
/// into a broker reaches every other client of that broker, as Uncertain,
/// and only from a changed service: the step has no topic evidence, and
/// chaining it would join the whole repository through one broker.
fn steps(
    graph: &BlastGraph,
    node: &str,
    from_seed: bool,
    touched: &mut BTreeMap<String, String>,
) -> Vec<Hop> {
    let is_code = |name: &str| {
        graph
            .service(name)
            .is_some_and(|s| s.role == ServiceRole::Code)
    };
    let mut hops: Vec<Hop> = Vec::new();
    for e in graph.inbound(node) {
        if e.edge_type == EdgeType::Event || e.source == node || !is_code(&e.source) {
            continue;
        }
        let relation = match e.edge_type {
            EdgeType::Http | EdgeType::Grpc => Relation::Calls,
            EdgeType::Import => Relation::Imports,
            EdgeType::Database => Relation::SharesDatabase,
            EdgeType::Event => unreachable!("event edges are skipped above"),
        };
        hops.push(Hop {
            from: node.to_string(),
            to: e.source.clone(),
            relation,
            edge_type: e.edge_type,
            confidence: e.confidence,
            via: None,
            calls: e.observed.as_ref().and_then(|o| o.calls),
        });
    }
    for e in graph.outbound(node) {
        if e.edge_type != EdgeType::Event || e.target == node {
            continue;
        }
        if is_code(&e.target) {
            hops.push(Hop {
                from: node.to_string(),
                to: e.target.clone(),
                relation: Relation::Consumes,
                edge_type: EdgeType::Event,
                confidence: e.confidence,
                via: None,
                calls: e.observed.as_ref().and_then(|o| o.calls),
            });
            continue;
        }
        if !from_seed {
            continue;
        }
        touched
            .entry(e.target.clone())
            .or_insert_with(|| node.to_string());
        for client in graph.inbound(&e.target) {
            if client.source == node || !is_code(&client.source) {
                continue;
            }
            hops.push(Hop {
                from: node.to_string(),
                to: client.source.clone(),
                relation: Relation::SharesBroker,
                edge_type: EdgeType::Event,
                confidence: Confidence::Uncertain,
                via: Some(e.target.clone()),
                calls: None,
            });
        }
    }
    hops
}

/// The blast radius of `change`: its files mapped to services, then the walk.
pub fn of_change(graph: &BlastGraph, change: Change, depth: usize) -> Blast {
    let (changed, unowned) = seeds(graph, &change.files);
    let seed_names: Vec<String> = changed.iter().map(|c| c.service.clone()).collect();
    let r = radius(graph, &seed_names, depth);
    let summary = Summary {
        changed: changed.len(),
        reached: r.reached.len(),
        by_confidence: ByConfidence::count(&r.reached),
    };
    Blast {
        change,
        depth,
        changed,
        unowned,
        reached: r.reached,
        infrastructure: r.infrastructure,
        not_reached: r.not_reached,
        summary,
    }
}

/// The code service whose change reaches the most services, and how many.
/// Ties go to the first name. None when there is no code service.
pub fn widest(graph: &BlastGraph, depth: usize) -> Option<(String, usize)> {
    graph
        .services()
        .into_iter()
        .filter(|s| s.role == ServiceRole::Code)
        .map(|s| {
            let count = radius(graph, std::slice::from_ref(&s.name), depth)
                .reached
                .len();
            (s.name, count)
        })
        .max_by(|a, b| {
            a.1.cmp(&b.1)
                .then_with(|| compare_service_names(&b.0, &a.0))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        DiscoveryStrategy, Edge, Evidence, Observed, RuntimeSource, Service, ServiceSource,
    };
    use pretty_assertions::assert_eq;

    fn service(name: &str, root: Option<&str>) -> Service {
        Service {
            name: name.into(),
            root: root.map(str::to_string),
            language: None,
            entry_points: vec![],
            role: if root.is_some() {
                ServiceRole::Code
            } else {
                ServiceRole::Infrastructure
            },
            discovered_by: ServiceSource::Strategy(DiscoveryStrategy::DockerCompose),
            evidence: Evidence {
                file: "docker-compose.yml".into(),
                line: None,
                detail: None,
            },
            image: None,
            package_name: None,
        }
    }

    fn edge(s: &str, t: &str, ty: EdgeType, c: Confidence) -> Edge {
        Edge {
            source: s.into(),
            target: t.into(),
            edge_type: ty,
            confidence: c,
            evidence: vec![],
            observed: None,
        }
    }

    /// web -> checkout (http, uncertain); checkout -> payment (http, observed 120);
    /// orders -> payment (grpc, static); reports -> orders (import, static);
    /// audit <-> payment (database, inferred); payment -> dispatch (event, inferred);
    /// payment -> rabbitmq, dispatch -> rabbitmq, mailer -> rabbitmq (event);
    /// payment -> redis (database); lonely has no edges.
    fn graph() -> BlastGraph {
        let mut g = BlastGraph::new();
        for (name, root) in [
            ("web", "web"),
            ("checkout", "services/checkout"),
            ("payment", "services/payment"),
            ("orders", "services/orders"),
            ("reports", "services/reports"),
            ("audit", "services/audit"),
            ("dispatch", "dispatch"),
            ("mailer", "mailer"),
            ("lonely", "lonely"),
        ] {
            g.add_service(service(name, Some(root)));
        }
        g.add_service(service("rabbitmq", None));
        g.add_service(service("redis", None));
        let mut observed = edge("checkout", "payment", EdgeType::Http, Confidence::Observed);
        observed.observed = Some(Observed {
            calls: Some(120),
            source: RuntimeSource::Otel,
        });
        for e in [
            edge("web", "checkout", EdgeType::Http, Confidence::Uncertain),
            observed,
            edge("orders", "payment", EdgeType::Grpc, Confidence::Static),
            edge("reports", "orders", EdgeType::Import, Confidence::Static),
            edge("audit", "payment", EdgeType::Database, Confidence::Inferred),
            edge("payment", "audit", EdgeType::Database, Confidence::Inferred),
            edge("payment", "dispatch", EdgeType::Event, Confidence::Inferred),
            edge("payment", "rabbitmq", EdgeType::Event, Confidence::Static),
            edge(
                "dispatch",
                "rabbitmq",
                EdgeType::Event,
                Confidence::Inferred,
            ),
            edge("mailer", "rabbitmq", EdgeType::Event, Confidence::Static),
            edge("payment", "redis", EdgeType::Database, Confidence::Static),
        ] {
            g.add_edge(e).unwrap();
        }
        g.sort_edges();
        g
    }

    fn names(reached: &[Reached]) -> Vec<(&str, usize, Confidence)> {
        reached
            .iter()
            .map(|r| (r.service.as_str(), r.depth, r.confidence))
            .collect()
    }

    #[test]
    fn owners_take_the_longest_root_and_share_it() {
        let mut g = graph();
        g.add_service(service("checkout-worker", Some("services/checkout")));
        g.add_service(service("checkout-lib", Some("services/checkout/lib")));
        assert_eq!(
            owners(&g, "services/checkout/src/index.ts"),
            vec!["checkout", "checkout-worker"]
        );
        assert_eq!(
            owners(&g, "./services/checkout/lib/util.ts"),
            vec!["checkout-lib"]
        );
        assert_eq!(
            owners(&g, "services/checkout"),
            vec!["checkout", "checkout-worker"]
        );
        assert_eq!(owners(&g, "services/checkouts/x.ts"), Vec::<String>::new());
        assert_eq!(owners(&g, "README.md"), Vec::<String>::new());
        assert_eq!(owners(&g, "rabbitmq/x"), Vec::<String>::new());
    }

    #[test]
    fn a_root_service_owns_everything_but_loses_to_any_other_root() {
        let mut g = BlastGraph::new();
        g.add_service(service("app", Some(".")));
        assert_eq!(owners(&g, "src/main.rs"), vec!["app"]);
        g.add_service(service("api", Some("api")));
        assert_eq!(owners(&g, "api/main.go"), vec!["api"]);
        assert_eq!(owners(&g, "docs/x.md"), vec!["app"]);
    }

    #[test]
    fn seeds_group_files_and_list_the_unowned_ones_once() {
        let files: Vec<String> = [
            "services/payment/pay.py",
            "README.md",
            "services/payment/pay.py",
            "web/index.html",
            "docs/a.md",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let (changed, unowned) = seeds(&graph(), &files);
        assert_eq!(
            changed,
            vec![
                Changed {
                    service: "payment".into(),
                    files: vec!["services/payment/pay.py".into()]
                },
                Changed {
                    service: "web".into(),
                    files: vec!["web/index.html".into()]
                },
            ]
        );
        assert_eq!(unowned, vec!["README.md", "docs/a.md"]);
    }

    #[test]
    fn the_walk_follows_every_rule_in_the_table() {
        let r = radius(&graph(), &["payment".to_string()], 3);
        assert_eq!(
            names(&r.reached),
            vec![
                ("checkout", 1, Confidence::Observed),
                ("orders", 1, Confidence::Static),
                ("audit", 1, Confidence::Inferred),
                ("dispatch", 1, Confidence::Inferred),
                ("mailer", 1, Confidence::Uncertain),
                ("reports", 2, Confidence::Static),
                ("web", 2, Confidence::Uncertain),
            ]
        );
        let by_name = |n: &str| r.reached.iter().find(|x| x.service == n).unwrap();
        // inbound http: checkout calls payment, with the observed count
        let checkout = by_name("checkout");
        assert_eq!(checkout.path.len(), 1);
        assert_eq!(checkout.path[0].relation, Relation::Calls);
        assert_eq!(checkout.path[0].calls, Some(120));
        assert_eq!(
            (checkout.path[0].from.as_str(), checkout.path[0].to.as_str()),
            ("payment", "checkout")
        );
        // inbound grpc, import at depth 2, shared database, consumer
        assert_eq!(by_name("orders").path[0].relation, Relation::Calls);
        assert_eq!(by_name("orders").path[0].edge_type, EdgeType::Grpc);
        assert_eq!(by_name("reports").path[1].relation, Relation::Imports);
        assert_eq!(by_name("audit").path[0].relation, Relation::SharesDatabase);
        assert_eq!(by_name("dispatch").path[0].relation, Relation::Consumes);
        // broker: mailer shares rabbitmq with payment, uncertain; dispatch keeps
        // its stronger consumer path
        let mailer = by_name("mailer");
        assert_eq!(mailer.path[0].relation, Relation::SharesBroker);
        assert_eq!(mailer.path[0].via.as_deref(), Some("rabbitmq"));
        assert_eq!(mailer.path[0].confidence, Confidence::Uncertain);
        assert_eq!(
            r.infrastructure,
            vec![Touched {
                service: "rabbitmq".into(),
                via: "payment".into()
            }]
        );
        // redis is a dependency of payment, not affected; lonely has no path
        assert_eq!(r.not_reached, vec!["lonely"]);
        assert!(!r.reached.iter().any(|x| x.service == "redis"));
    }

    #[test]
    fn steps_not_taken() {
        // a consumer changing does not reach its producer (inbound event)
        let mut g = BlastGraph::new();
        g.add_service(service("producer", Some("producer")));
        g.add_service(service("consumer", Some("consumer")));
        g.add_edge(edge(
            "producer",
            "consumer",
            EdgeType::Event,
            Confidence::Inferred,
        ))
        .unwrap();
        let r = radius(&g, &["consumer".to_string()], 3);
        assert_eq!(r.reached, vec![], "{:?}", names(&r.reached));
        assert_eq!(r.not_reached, vec!["producer"]);
        let r = radius(&g, &["producer".to_string()], 3);
        assert_eq!(
            names(&r.reached),
            vec![("consumer", 1, Confidence::Inferred)]
        );

        let g = graph();
        // a service's own dependencies are not affected (outbound http)
        let r = radius(&g, &["web".to_string()], 3);
        assert_eq!(r.reached, vec![]);
        assert_eq!(r.infrastructure, vec![]);
        // the broker step is taken from a changed service only: mailer is
        // changed -> payment and dispatch via rabbitmq, but from there the
        // broker is not walked again, so nothing beyond payment's own
        // callers appears through rabbitmq
        let r = radius(&g, &["mailer".to_string()], 3);
        assert_eq!(
            names(&r.reached),
            vec![
                ("dispatch", 1, Confidence::Uncertain),
                ("payment", 1, Confidence::Uncertain),
                ("audit", 2, Confidence::Uncertain),
                ("checkout", 2, Confidence::Uncertain),
                ("orders", 2, Confidence::Uncertain),
                ("reports", 3, Confidence::Uncertain),
                ("web", 3, Confidence::Uncertain),
            ]
        );
    }

    #[test]
    fn depth_cuts_the_walk_and_seeds_are_never_reached() {
        let g = graph();
        let r = radius(&g, &["payment".to_string()], 1);
        assert!(r.reached.iter().all(|x| x.depth == 1));
        assert!(r.not_reached.contains(&"reports".to_string()));
        assert!(r.not_reached.contains(&"web".to_string()));
        let r = radius(&g, &["payment".to_string()], 0);
        assert_eq!(r.reached, vec![]);
        assert_eq!(r.not_reached.len(), 8);
        let r = radius(&g, &["payment".to_string(), "checkout".to_string()], 3);
        assert!(!r.reached.iter().any(|x| x.service == "checkout"));
        assert!(!r.not_reached.contains(&"checkout".to_string()));
        // unknown seeds are ignored
        let r = radius(&g, &["nope".to_string()], 3);
        assert_eq!(r.reached, vec![]);
    }

    #[test]
    fn the_strongest_path_wins_over_the_shortest() {
        // a is changed. b calls a (uncertain). c calls a (static), b calls c (static):
        // b is reachable at depth 1 uncertain and depth 2 static; static wins.
        let mut g = BlastGraph::new();
        for n in ["a", "b", "c"] {
            g.add_service(service(n, Some(n)));
        }
        g.add_edge(edge("b", "a", EdgeType::Http, Confidence::Uncertain))
            .unwrap();
        g.add_edge(edge("c", "a", EdgeType::Http, Confidence::Static))
            .unwrap();
        g.add_edge(edge("b", "c", EdgeType::Http, Confidence::Static))
            .unwrap();
        let r = radius(&g, &["a".to_string()], 3);
        assert_eq!(
            names(&r.reached),
            vec![("c", 1, Confidence::Static), ("b", 2, Confidence::Static)]
        );
        assert_eq!(r.reached[1].path.len(), 2);
        // with depth 1 only the uncertain path exists
        let r = radius(&g, &["a".to_string()], 1);
        assert_eq!(
            names(&r.reached),
            vec![
                ("c", 1, Confidence::Static),
                ("b", 1, Confidence::Uncertain)
            ]
        );
    }

    #[test]
    fn of_change_and_widest() {
        let g = graph();
        let change = Change::from_files(&[
            "./services/payment/pay.py".to_string(),
            "README.md".to_string(),
        ]);
        assert_eq!(change.reference, "2 files");
        assert_eq!(change.files, vec!["services/payment/pay.py", "README.md"]);
        let b = of_change(&g, change, 3);
        assert_eq!(b.depth, 3);
        assert_eq!(b.changed[0].service, "payment");
        assert_eq!(b.unowned, vec!["README.md"]);
        assert_eq!(
            b.summary,
            Summary {
                changed: 1,
                reached: 7,
                by_confidence: ByConfidence {
                    observed: 1,
                    static_: 2,
                    inferred: 2,
                    uncertain: 2
                }
            }
        );
        let json = serde_json::to_value(&b).unwrap();
        assert_eq!(
            json["summary"]["byConfidence"],
            serde_json::json!({ "observed": 1, "static": 2, "inferred": 2, "uncertain": 2 })
        );
        assert_eq!(json["change"]["kind"], "files");
        assert_eq!(json["reached"][4]["path"][0]["relation"], "shares-broker");
        assert_eq!(json["reached"][4]["path"][0]["via"], "rabbitmq");
        assert!(json["reached"][0]["path"][0].get("via").is_none());
        assert_eq!(json["reached"][0]["path"][0]["calls"], 120);
        assert_eq!(json["notReached"], serde_json::json!(["lonely"]));
        // dispatch and payment both reach 7; the tie goes to the first name
        assert_eq!(widest(&g, 3), Some(("dispatch".into(), 7)));
        assert_eq!(widest(&BlastGraph::new(), 3), None);
        assert!(NOT_REACHED.starts_with("Not in the computed blast radius"));
    }
}
