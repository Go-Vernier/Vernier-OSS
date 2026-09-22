//! Strategy 3: monorepo conventions. A child directory of a conventional
//! parent that holds its own manifest (package.json, go.mod, pom.xml,
//! requirements.txt, ...) is a service. Children without one (docs, shared
//! config) are not.
use std::path::Path;

use indexmap::IndexMap;

use super::build_service;
use super::language::describe_directory;
use crate::fs::{FileIndex, is_dir};
use crate::model::{DiscoveryStrategy, Evidence, Service, ServiceSource};

/// The spec names services/, apps/ and packages/; src/ is added because the
/// reference corpus (Google's microservices-demo, the OpenTelemetry demo)
/// keeps one service per directory under src/.
pub const MONOREPO_PARENTS: [&str; 5] = ["services", "apps", "packages", "microservices", "src"];

pub fn discover_from_monorepo(root: &Path, _index: &FileIndex) -> Vec<Service> {
    let mut found: IndexMap<String, Service> = IndexMap::new();
    for parent in MONOREPO_PARENTS {
        let parent_abs = root.join(parent);
        if !is_dir(&parent_abs) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&parent_abs) else {
            continue;
        };
        let mut children: Vec<String> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with('.') && n != "node_modules")
            .collect();
        children.sort();

        for child in children {
            let described = describe_directory(&parent_abs.join(&child));
            let Some(manifest) = described.manifest.clone() else {
                continue;
            };
            // First parent wins on a name clash (apps/web vs packages/web).
            if found.contains_key(&child) {
                continue;
            }
            let service = build_service(
                child.clone(),
                Some(format!("{parent}/{child}")),
                Some(described),
                ServiceSource::Strategy(DiscoveryStrategy::Monorepo),
                Evidence {
                    file: format!("{parent}/{child}/{}", manifest.file),
                    line: None,
                    detail: Some(manifest.file),
                },
                None,
            );
            found.insert(child, service);
        }
    }
    found.into_values().collect()
}
