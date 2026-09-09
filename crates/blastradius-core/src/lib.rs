//! Vernier: which services can this change reach?
//!
//! Stage 1, discovery, finds the service boundaries in a repository. Stage 2
//! maps static edges, stage 3 joins runtime data, and stage 4 walks the graph
//! from a change to report its blast radius.
//! Every service and edge carries evidence and a confidence label. The JSON
//! printed by [`Analysis::to_json`] is the contract other tools consume.
pub mod analyze;
pub mod blast;
pub mod config;
pub mod discover;
pub mod fs;
pub mod git;
pub mod graph;
pub mod history;
pub mod html;
pub mod map;
pub mod model;
pub mod report;
pub mod runtime;
pub mod yaml;

pub use analyze::{
    Analysis, AnalysisJson, AnalyzeError, Discovery, Runtime, RuntimeEdges, RuntimeMapping,
    RuntimeServices, analyze, repository_name,
};
pub use blast::{Blast, Change, ChangeKind, Changed, Hop, Reached, Relation, Touched};
pub use config::{Config, ConfigError, RuntimeConfig};
pub use git::GitError;
pub use graph::{BlastGraph, GraphError};
pub use history::History;
pub use map::MappingStats;
pub use model::*;
pub use report::{format_change_report, format_repo_report, format_report};
pub use runtime::{RuntimeCall, RuntimeError, RuntimeGraph, RuntimeInput, RuntimeKind};
