//! Runs every built stage over a repository and returns the graph.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::blast::Blast;
use crate::discover::{DiscoveryAttempt, discover_services};
use crate::fs::{FileIndex, is_dir};
use crate::graph::BlastGraph;
use crate::history::History;
use crate::map::{self, MappingStats};
use crate::model::{DiscoveryStrategy, Edge, RuntimeSource, Service};

#[derive(Debug, Error)]
pub enum AnalyzeError {
    #[error("not a directory: {0}")]
    NotADirectory(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Discovery {
    pub strategy: Option<DiscoveryStrategy>,
    pub attempted: Vec<DiscoveryAttempt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMapping {
    pub runtime: String,
    pub service: Option<String>,
    pub how: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeServices {
    pub runtime: usize,
    pub matched: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEdges {
    pub observed: usize,
    pub runtime_only: usize,
    pub skipped: usize,
}

/// Stage 3 fills this in. Without a runtime source it serialises as
/// `{ "connected": false }`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Runtime {
    pub connected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<RuntimeSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub services: Option<RuntimeServices>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mapping: Vec<RuntimeMapping>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unmatched: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edges: Option<RuntimeEdges>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Analysis {
    /// `owner/repo` from the git remote, or the directory name.
    pub repository: String,
    /// Absolute path that was analysed.
    pub root: PathBuf,
    pub discovery: Discovery,
    pub graph: BlastGraph,
    pub mapping: MappingStats,
    pub runtime: Runtime,
    /// Stage 4 fills these in when a change or a history was asked for.
    pub blast: Option<Blast>,
    pub history: Option<History>,
}

/// The JSON contract. Field order is part of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisJson {
    pub repository: String,
    pub root: String,
    pub discovery: Discovery,
    pub services: Vec<Service>,
    pub edges: Vec<Edge>,
    pub mapping: MappingStats,
    pub runtime: Runtime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blast: Option<Blast>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history: Option<History>,
}

impl Analysis {
    pub fn to_json(&self) -> AnalysisJson {
        AnalysisJson {
            repository: self.repository.clone(),
            root: self.root.display().to_string(),
            discovery: self.discovery.clone(),
            services: self.graph.services(),
            edges: self.graph.edges().to_vec(),
            mapping: self.mapping.clone(),
            runtime: self.runtime.clone(),
            blast: self.blast.clone(),
            history: self.history.clone(),
        }
    }
}

pub fn analyze(root: &Path) -> Result<Analysis, AnalyzeError> {
    if !is_dir(root) {
        return Err(AnalyzeError::NotADirectory(root.display().to_string()));
    }
    let root = root.canonicalize()?;
    let index = FileIndex::build(&root);
    let discovery = discover_services(&root, &index);

    let mapped = map::run(&root, &index, &discovery.services);

    let mut graph = BlastGraph::new();
    for service in discovery.services {
        graph.add_service(service);
    }
    for edge in mapped.edges {
        graph
            .add_edge(edge)
            .expect("mapping only produces edges between discovered services");
    }

    Ok(Analysis {
        repository: repository_name(&root),
        root,
        discovery: Discovery {
            strategy: discovery.strategy,
            attempted: discovery.attempted,
        },
        graph,
        mapping: mapped.stats,
        runtime: Runtime::default(),
        blast: None,
        history: None,
    })
}

static REMOTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[:/]([^/:\s]+)/([^/\s]+?)(?:\.git)?/?$").unwrap());

/// `owner/repo` from the origin remote when there is one, else the
/// directory's name. The only subprocess the engine runs.
pub fn repository_name(root: &Path) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["remote", "get-url", "origin"])
        .output();
    if let Ok(output) = output {
        if output.status.success() {
            let url = String::from_utf8_lossy(&output.stdout);
            if let Some(caps) = REMOTE.captures(url.trim()) {
                return format!("{}/{}", &caps[1], &caps[2]);
            }
        }
    }
    root.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}
