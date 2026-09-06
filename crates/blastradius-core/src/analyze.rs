//! Runs every built stage over a repository and returns the graph.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::discover::{DiscoveryAttempt, discover_services};
use crate::fs::{FileIndex, is_dir};
use crate::graph::BlastGraph;
use crate::map::{self, MappingStats};
use crate::model::{DiscoveryStrategy, Edge, Service};

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

/// Stage 3 fills this in. Until then the report says so plainly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Runtime {
    pub connected: bool,
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
}

/// The JSON contract. Field order is part of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisJson {
    pub repository: String,
    pub root: String,
    pub discovery: Discovery,
    pub services: Vec<Service>,
    pub edges: Vec<Edge>,
    pub mapping: MappingStats,
    pub runtime: Runtime,
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
            runtime: self.runtime,
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
