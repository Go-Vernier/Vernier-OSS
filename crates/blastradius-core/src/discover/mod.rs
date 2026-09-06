//! Stage 1: service discovery.
//!
//! Strategies are tried in order and the first that finds more than one
//! service with code in the repository wins. When none does, the repository
//! is reported as one service, keeping whatever infrastructure was declared
//! so a deploy-only repository is described, not hidden. Boundaries are
//! never invented.
pub mod compose;
pub mod directories;
pub mod env;
pub mod kubernetes;
pub mod language;
pub mod monorepo;
pub mod workspace;

use std::cmp::Ordering;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::fs::FileIndex;
use crate::model::{DiscoveryStrategy, Evidence, Service, ServiceRole, ServiceSource};
use language::DirectoryDescription;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryAttempt {
    pub strategy: DiscoveryStrategy,
    /// Services with code in the repository that this strategy found.
    pub services: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryResult {
    pub services: Vec<Service>,
    /// The strategy that found more than one service, or None: the honest
    /// single-service fallback.
    pub strategy: Option<DiscoveryStrategy>,
    pub attempted: Vec<DiscoveryAttempt>,
}

type Strategy = fn(&Path, &FileIndex) -> Vec<Service>;

const STRATEGIES: &[(DiscoveryStrategy, Strategy)] = &[
    (
        DiscoveryStrategy::DockerCompose,
        compose::discover_from_compose,
    ),
    (
        DiscoveryStrategy::Kubernetes,
        kubernetes::discover_from_kubernetes,
    ),
    (
        DiscoveryStrategy::Monorepo,
        monorepo::discover_from_monorepo,
    ),
    (
        DiscoveryStrategy::Workspace,
        workspace::discover_from_workspace,
    ),
];

pub fn discover_services(root: &Path, index: &FileIndex) -> DiscoveryResult {
    let mut attempted = Vec::new();
    let mut fallback: Vec<Service> = Vec::new();

    for (strategy, run) in STRATEGIES {
        let mut services = run(root, index);
        let code = code_count(&services);
        attempted.push(DiscoveryAttempt {
            strategy: *strategy,
            services: code,
        });
        if code > 1 {
            sort_services(&mut services);
            return DiscoveryResult {
                services,
                strategy: Some(*strategy),
                attempted,
            };
        }
        let better = code > code_count(&fallback)
            || (code == code_count(&fallback) && services.len() > fallback.len());
        if better {
            fallback = services;
        }
    }

    if code_count(&fallback) == 0 {
        let described = language::describe_directory(root);
        if let Some(manifest) = described.manifest.clone() {
            let name = root
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            fallback.push(build_service(
                name,
                Some(".".to_string()),
                Some(described),
                ServiceSource::Root,
                Evidence {
                    file: manifest.file.clone(),
                    line: None,
                    detail: Some(manifest.file),
                },
                None,
            ));
        }
    }
    sort_services(&mut fallback);
    DiscoveryResult {
        services: fallback,
        strategy: None,
        attempted,
    }
}

/// Code first, then by name, case-insensitive, then byte order.
pub fn sort_services(services: &mut [Service]) {
    services.sort_by(|a, b| {
        role_rank(a.role)
            .cmp(&role_rank(b.role))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
}

pub fn compare_service_names(a: &str, b: &str) -> Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(b))
}

fn role_rank(role: ServiceRole) -> u8 {
    match role {
        ServiceRole::Code => 0,
        ServiceRole::Infrastructure => 1,
    }
}

pub fn code_count(services: &[Service]) -> usize {
    services
        .iter()
        .filter(|s| s.role == ServiceRole::Code)
        .count()
}

/// A service with code when `root` names a directory, infrastructure when it
/// does not.
pub(crate) fn build_service(
    name: String,
    root: Option<String>,
    described: Option<DirectoryDescription>,
    discovered_by: ServiceSource,
    evidence: Evidence,
    image: Option<String>,
) -> Service {
    let described = described.unwrap_or_default();
    let role = if root.is_some() {
        ServiceRole::Code
    } else {
        ServiceRole::Infrastructure
    };
    Service {
        name,
        root,
        language: described.language,
        entry_points: described.entry_points,
        role,
        discovered_by,
        evidence,
        image,
        package_name: described.package_name,
    }
}
