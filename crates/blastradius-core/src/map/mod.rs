//! Stage 2: static dependency mapping.
//!
//! `facts` turns one file into language-neutral facts. Matchers turn facts
//! into candidates naming what they found. The resolver turns candidates
//! into edges between discovered services.
pub mod config;
pub mod facts;
pub mod matchers;
pub mod resolve;

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
use resolve::{Resolver, Unresolved};

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

    let owners_map = matchers::grpc::proto_owners(&extractions, &config, services);
    let resolver = Resolver::new(services, &config, owners_map);
    let outcomes = collect_outcomes(&extractions, &config, &resolver);
    let edges = merge_edges(outcomes, &mut stats);
    MapResult { edges, stats }
}

/// Files worth reading, each with the service that owns it. Longest root
/// wins, so nested services own their own files.
fn partition_files(
    root: &Path,
    index: &FileIndex,
    services: &[Service],
    stats: &mut MappingStats,
) -> Vec<(String, String)> {
    let mut owners: Vec<(&str, &str)> = services
        .iter()
        .filter(|s| s.role == ServiceRole::Code)
        .filter_map(|s| s.root.as_deref().map(|r| (s.name.as_str(), r)))
        .collect();
    owners.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(b.0)));
    let owner_of = |file: &str| -> Option<&str> {
        owners
            .iter()
            .find(|(_, r)| *r == "." || file.starts_with(*r) && file[r.len()..].starts_with('/'))
            .map(|(name, _)| *name)
    };

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
            };
            let mut results = Vec::new();
            for matcher in &matchers {
                for candidate in matcher.candidates(&ctx) {
                    results.push(resolver.resolve(service, &candidate).map(|resolved| Edge {
                        source: service.clone(),
                        target: resolved.target,
                        edge_type: resolved.edge_type,
                        confidence: resolved.confidence,
                        evidence: vec![Evidence {
                            file: candidate.evidence.file.clone(),
                            line: candidate.evidence.line,
                            detail: Some(resolved.detail),
                        }],
                    }));
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
            Err(Unresolved::SelfEdge) => {}
            Err(Unresolved::Unknown(name)) => {
                stats.unresolved += 1;
                unresolved_targets.insert(name);
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
