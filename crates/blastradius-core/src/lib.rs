//! Blast Radius: which services can this change reach?
//!
//! Stage 1, discovery, finds the service boundaries in a repository. Later
//! stages add static edges, the runtime join and the blast radius itself.
//! Every service and edge carries evidence and a confidence label. The JSON
//! printed by [`Analysis::to_json`] is the contract other tools consume.
pub mod analyze;
pub mod discover;
pub mod fs;
pub mod graph;
pub mod map;
pub mod model;
pub mod report;
pub mod yaml;

pub use analyze::{
    Analysis, AnalysisJson, AnalyzeError, Discovery, Runtime, analyze, repository_name,
};
pub use graph::{BlastGraph, GraphError};
pub use map::MappingStats;
pub use model::*;
pub use report::format_repo_report;
