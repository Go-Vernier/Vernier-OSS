//! Stage 2: static dependency mapping.
//!
//! `facts` turns one file into language-neutral facts. Matchers turn facts
//! into candidates naming what they found. The resolver turns candidates
//! into edges between discovered services.
pub mod config;
pub mod facts;
pub mod matchers;
pub mod resolve;
pub mod symbols;

use std::collections::BTreeSet;
use std::path::Path;

use indexmap::IndexMap;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::fs::{FileIndex, read_text};
use crate::model::{Edge, EdgeType, Evidence, Service, ServiceRole};
use config::ConfigIndex;
use facts::{Extraction, Part};
use matchers::FileContext;
use resolve::{Joins, Resolver, Unresolved};
use symbols::Symbols;

/// What a matcher found, before it is placed on a service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A full URL as written, scheme included; `jdbc:mysql://...` counts.
    Url(String),
    /// `payment:50051`
    HostPort(String),
    /// A bare host from an environment default or a hostname position.
    Host(String),
    /// A plain string literal equal to an infrastructure service's name:
    /// weak, so Uncertain.
    BareName(String),
    EnvVar {
        name: String,
        default: Option<String>,
    },
    Template(Vec<Part>),
    ProtoService(String),
    /// A message topic, queue, exchange or event type, with the part the
    /// mentioning code plays.
    Topic {
        key: String,
        role: TopicRole,
    },
    /// A message broker known through the client library a file imports;
    /// `how` is the detail, e.g. `imports pika (rabbitmq client)`.
    Broker {
        family: String,
        how: String,
    },
    /// A database key another service may share: `host/dbname`, or a named
    /// resource such as `orderingdb`.
    Database(String),
    /// A package, module or artifact as imported or declared; `how` is the
    /// detail, e.g. `import @acme/shared/utils`, `dependency @acme/shared`.
    Package {
        name: String,
        how: String,
    },
    /// A repository-relative path another manifest refers to, already
    /// normalised: `src/EventBus/EventBus.csproj`, `services/core-rs`.
    PackagePath {
        path: String,
        how: String,
    },
}

/// Which side of a topic a mention is on. `Unknown` is a declaration or a
/// binding: `queue_declare("orders")`, `QueueBind(...)`, an SQS ARN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopicRole {
    Producer,
    Consumer,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub target: Target,
    /// The grpc matcher says Grpc; the http matcher leaves None so the
    /// resolver types the edge by its target.
    pub kind_hint: Option<EdgeType>,
    pub evidence: Evidence,
}

/// What the mapping stage read and what it could not place, so the report
/// never looks more complete than it is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MappingStats {
    pub files_scanned: usize,
    /// Files not read, by extension or reason (`large`).
    pub files_skipped: IndexMap<String, usize>,
    /// Files under no discovered service root.
    pub files_outside_services: usize,
    /// Candidates that matched no discovered service.
    pub unresolved: usize,
    /// Distinct unresolved targets, sorted, at most 50.
    pub unresolved_targets: Vec<String>,
    /// Which parser handled each language label.
    pub parsers: IndexMap<String, facts::Parser>,
}

pub struct MapResult {
    pub edges: Vec<Edge>,
    pub stats: MappingStats,
}

const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_EVIDENCE: usize = 25;
const MAX_UNRESOLVED_LISTED: usize = 50;

type EdgeKey = (String, String, EdgeType);

/// An edge under construction: its evidence keeps the confidence rank of
/// the candidate that produced it, so the strongest reason is printed first.
struct Merged {
    edge: Edge,
    evidence: Vec<(u8, Evidence)>,
}

/// Stage 2 over a discovered repository.
pub fn run(root: &Path, index: &FileIndex, services: &[Service]) -> MapResult {
    let config = ConfigIndex::build(root, index, services);
    let mut stats = MappingStats::default();

    let to_scan = partition_files(root, index, services, &mut stats);
    let extractions = extract_all(root, &to_scan);
    stats.files_scanned = extractions.len();
    for (_, _, ex) in &extractions {
        stats
            .parsers
            .entry(ex.language.clone())
            .or_insert(ex.parser);
    }
    stats.parsers.sort_keys();

    let symbols = symbols::build(&extractions);
    let joins = Joins {
        proto_owner: matchers::grpc::proto_owners(&extractions, &config, services),
        topics: matchers::event::topic_index(&extractions, &config, &symbols),
        databases: matchers::database::database_index(&extractions, &config, &symbols),
    };
    let resolver = Resolver::new(services, &config, joins);
    let outcomes = collect_outcomes(&extractions, &config, &symbols, &resolver);
    let edges = merge_edges(outcomes, &mut stats);
    MapResult { edges, stats }
}

/// The code service whose root is the longest prefix of `path`. A root of
/// `.` owns everything.
pub fn owner_of_path<'a>(services: &'a [Service], path: &str) -> Option<&'a str> {
    let mut owners: Vec<(&str, &str)> = services
        .iter()
        .filter(|s| s.role == ServiceRole::Code)
        .filter_map(|s| s.root.as_deref().map(|r| (s.name.as_str(), r)))
        .collect();
    owners.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(b.0)));
    owners
        .iter()
        .find(|(_, r)| {
            *r == "." || path == *r || (path.starts_with(*r) && path[r.len()..].starts_with('/'))
        })
        .map(|(name, _)| *name)
}

/// Files worth reading, each with the service that owns it. Longest root
/// wins, so nested services own their own files.
fn partition_files(
    root: &Path,
    index: &FileIndex,
    services: &[Service],
    stats: &mut MappingStats,
) -> Vec<(String, String)> {
    let owner_of = |file: &str| owner_of_path(services, file);

    let mut to_scan: Vec<(String, String)> = Vec::new();
    for file in index.files() {
        let Some(service) = owner_of(file) else {
            stats.files_outside_services += 1;
            continue;
        };
        if facts::language_for(file).is_none() {
            let ext = file
                .rsplit_once('.')
                .map_or("(none)", |(_, e)| e)
                .to_lowercase();
            *stats.files_skipped.entry(ext).or_insert(0) += 1;
            continue;
        }
        if std::fs::metadata(root.join(file)).is_ok_and(|m| m.len() > MAX_FILE_BYTES) {
            *stats.files_skipped.entry("large".to_string()).or_insert(0) += 1;
            continue;
        }
        to_scan.push((service.to_string(), file.clone()));
    }
    to_scan
}

fn extract_all(root: &Path, to_scan: &[(String, String)]) -> Vec<(String, String, Extraction)> {
    let mut extractions: Vec<(String, String, Extraction)> = to_scan
        .par_iter()
        .filter_map(|(service, file)| {
            let text = read_text(&root.join(file))?;
            let extraction = facts::extract(file, &text)?;
            Some((service.clone(), file.clone(), extraction))
        })
        .collect();
    extractions.sort_by(|a, b| a.1.cmp(&b.1));
    extractions
}

fn collect_outcomes(
    extractions: &[(String, String, Extraction)],
    config: &ConfigIndex,
    symbols: &Symbols,
    resolver: &Resolver<'_>,
) -> Vec<Result<Edge, Unresolved>> {
    let matchers = matchers::all();
    extractions
        .par_iter()
        .flat_map_iter(|(service, file, ex)| {
            let ctx = FileContext {
                service,
                file,
                facts: &ex.facts,
                config,
                symbols,
            };
            let mut results = Vec::new();
            for matcher in &matchers {
                for candidate in matcher.candidates(&ctx) {
                    for outcome in resolver.resolve_all(service, &candidate) {
                        results.push(outcome.map(|resolved| Edge {
                            source: resolved.source.clone().unwrap_or_else(|| service.clone()),
                            target: resolved.target,
                            edge_type: resolved.edge_type,
                            confidence: resolved.confidence,
                            evidence: vec![Evidence {
                                file: candidate.evidence.file.clone(),
                                line: candidate.evidence.line,
                                detail: Some(resolved.detail),
                            }],
                            observed: None,
                        }));
                    }
                }
            }
            results
        })
        .collect()
}

/// Merge by (source, target, type): highest confidence wins, evidence is
/// the union ordered strongest first, then by file and line.
fn merge_edges(outcomes: Vec<Result<Edge, Unresolved>>, stats: &mut MappingStats) -> Vec<Edge> {
    let mut merged: IndexMap<EdgeKey, Merged> = IndexMap::new();
    let mut unresolved_targets: BTreeSet<String> = BTreeSet::new();
    for outcome in outcomes {
        match outcome {
            Ok(mut edge) => {
                let key = (edge.source.clone(), edge.target.clone(), edge.edge_type);
                let rank = edge.confidence.rank();
                let evidence: Vec<(u8, Evidence)> = std::mem::take(&mut edge.evidence)
                    .into_iter()
                    .map(|e| (rank, e))
                    .collect();
                match merged.get_mut(&key) {
                    None => {
                        merged.insert(key, Merged { edge, evidence });
                    }
                    Some(existing) => {
                        if edge.confidence.rank() > existing.edge.confidence.rank() {
                            existing.edge.confidence = edge.confidence;
                        }
                        for item in evidence {
                            if !existing.evidence.iter().any(|(_, e)| *e == item.1) {
                                existing.evidence.push(item);
                            }
                        }
                    }
                }
            }
            Err(Unresolved::SelfEdge | Unresolved::Ignored) => {}
            Err(Unresolved::Unknown(name)) => {
                if is_reportable(&name) {
                    stats.unresolved += 1;
                    unresolved_targets.insert(name);
                }
            }
        }
    }

    // A URL on a pair that also has a gRPC stub is the stub's address.
    let grpc_pairs: Vec<(String, String)> = merged
        .keys()
        .filter(|(_, _, t)| *t == EdgeType::Grpc)
        .map(|(s, t, _)| (s.clone(), t.clone()))
        .collect();
    for (source, target) in grpc_pairs {
        let http_key = (source.clone(), target.clone(), EdgeType::Http);
        if let Some(http) = merged.shift_remove(&http_key) {
            if let Some(grpc) = merged.get_mut(&(source, target, EdgeType::Grpc)) {
                for item in http.evidence {
                    if !grpc.evidence.iter().any(|(_, e)| *e == item.1) {
                        grpc.evidence.push(item);
                    }
                }
            }
        }
    }

    let mut edges: Vec<Edge> = Vec::with_capacity(merged.len());
    for Merged {
        mut edge,
        mut evidence,
    } in merged.into_values()
    {
        evidence.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.file.cmp(&b.1.file))
                .then_with(|| a.1.line.cmp(&b.1.line))
        });
        edge.evidence = evidence
            .into_iter()
            .map(|(_, e)| e)
            .take(MAX_EVIDENCE)
            .collect();
        edges.push(edge);
    }
    edges.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then_with(|| a.target.cmp(&b.target))
            .then_with(|| a.edge_type.cmp(&b.edge_type))
    });
    stats.unresolved_targets = unresolved_targets
        .into_iter()
        .take(MAX_UNRESOLVED_LISTED)
        .collect();
    edges
}

/// Unresolved names worth listing: hostnames and variable names, not
/// format placeholders, punctuation or bare numbers.
fn is_reportable(name: &str) -> bool {
    let mut chars = name.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    first_ok
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | ':'))
        && name.len() >= 2
        && name != "_"
}
