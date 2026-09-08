//! The build spec's merge rules. Static and runtime agree: Observed, with
//! the count. Runtime alone: a new Observed edge, since static analysis
//! missed an async or dynamic call. Static alone: untouched, never dropped.
use std::collections::HashMap;

use crate::graph::BlastGraph;
use crate::model::{Confidence, Edge, EdgeType, Evidence, Observed};

use super::matching::Mapping;
use super::{RuntimeCall, RuntimeGraph, RuntimeKind};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MergeCounts {
    /// Edges carrying `observed` after the merge.
    pub observed: usize,
    /// Calls that confirmed at least one static edge.
    pub confirmed: usize,
    /// Edges added because no static edge matched.
    pub runtime_only: usize,
    /// Calls with an unmatched or ignored end.
    pub skipped: usize,
}

/// A runtime call of this kind confirms a static edge of this type.
fn compatible(kind: RuntimeKind, ty: EdgeType) -> bool {
    match kind {
        RuntimeKind::Unknown | RuntimeKind::Http | RuntimeKind::Grpc => {
            matches!(ty, EdgeType::Http | EdgeType::Grpc)
        }
        RuntimeKind::Event => ty == EdgeType::Event,
        RuntimeKind::Database => ty == EdgeType::Database,
    }
}

fn edge_type_for(kind: RuntimeKind) -> EdgeType {
    match kind {
        RuntimeKind::Grpc => EdgeType::Grpc,
        RuntimeKind::Event => EdgeType::Event,
        RuntimeKind::Database => EdgeType::Database,
        RuntimeKind::Http | RuntimeKind::Unknown => EdgeType::Http,
    }
}

fn detail(calls: Option<u64>, method: &str) -> String {
    match calls {
        Some(n) => format!("{n} calls ({method})"),
        None => format!("observed ({method})"),
    }
}

fn add_calls(existing: Option<u64>, more: Option<u64>) -> Option<u64> {
    match (existing, more) {
        (Some(a), Some(b)) => Some(a + b),
        (a, None) => a,
        (None, b) => b,
    }
}

pub fn apply(graph: &mut BlastGraph, runtime: &RuntimeGraph, mapping: &[Mapping]) -> MergeCounts {
    let resolved: HashMap<&str, Option<&str>> = mapping
        .iter()
        .map(|m| (m.runtime.as_str(), m.service.as_deref()))
        .collect();
    let mut counts = MergeCounts::default();
    let mut added: Vec<Edge> = Vec::new();
    for call in &runtime.calls {
        let (Some(Some(a)), Some(Some(b))) = (
            resolved.get(call.client.as_str()),
            resolved.get(call.server.as_str()),
        ) else {
            counts.skipped += 1;
            continue;
        };
        if a == b {
            continue;
        }
        let evidence = Evidence {
            file: runtime.input.clone(),
            line: None,
            detail: Some(detail(call.calls, runtime.method)),
        };
        let mut matched_static = false;
        for edge in graph
            .edges_mut()
            .iter_mut()
            .filter(|e| e.source == *a && e.target == *b && compatible(call.kind, e.edge_type))
        {
            matched_static = true;
            confirm(edge, call, runtime, &evidence);
        }
        if let Some(edge) = added
            .iter_mut()
            .find(|e| e.source == *a && e.target == *b && compatible(call.kind, e.edge_type))
        {
            // A second runtime call on a pair this join already added.
            confirm(edge, call, runtime, &evidence);
            continue;
        }
        if matched_static {
            counts.confirmed += 1;
            continue;
        }
        added.push(Edge {
            source: (*a).to_string(),
            target: (*b).to_string(),
            edge_type: edge_type_for(call.kind),
            confidence: Confidence::Observed,
            evidence: vec![evidence],
            observed: Some(Observed {
                calls: call.calls,
                source: runtime.source,
            }),
        });
        counts.runtime_only += 1;
    }
    for edge in added {
        graph
            .add_edge(edge)
            .expect("runtime edges join two discovered services");
    }
    graph.sort_edges();
    counts.observed = graph
        .edges()
        .iter()
        .filter(|e| e.observed.is_some())
        .count();
    counts
}

/// Promotes one edge: Observed, counts summed, one runtime evidence entry
/// whose detail carries the running total.
fn confirm(edge: &mut Edge, call: &RuntimeCall, runtime: &RuntimeGraph, evidence: &Evidence) {
    edge.confidence = Confidence::Observed;
    let total = add_calls(edge.observed.as_ref().and_then(|o| o.calls), call.calls);
    edge.observed = Some(Observed {
        calls: total,
        source: runtime.source,
    });
    let text = detail(total, runtime.method);
    match edge
        .evidence
        .iter_mut()
        .find(|v| v.file == runtime.input && v.line.is_none())
    {
        Some(existing) => existing.detail = Some(text),
        None => edge.evidence.push(Evidence {
            detail: Some(text),
            ..evidence.clone()
        }),
    }
}
