//! Strategy 2: Kubernetes manifests. Any `kind: Deployment` (or other
//! workload) or `kind: Service` is a service. The manifest names it and its
//! image; the repository directory is found by matching the name and the
//! image against directory names, since manifests rarely say where their
//! code lives. Helm and other Go templates are not YAML until rendered, so
//! they are skipped, not misread.
use std::path::Path;
use std::sync::LazyLock;

use indexmap::IndexMap;
use regex::Regex;

use super::build_service;
use super::compose::COMPOSE_PATTERNS;
use super::directories::{DirectoryIndex, image_basename};
use super::language::describe_directory;
use crate::fs::{FileIndex, read_text};
use crate::model::{DiscoveryStrategy, Evidence, Service, ServiceRole, ServiceSource};
use crate::yaml::{Node, parse_documents};

const WORKLOAD_KINDS: &[&str] = &[
    "Deployment",
    "StatefulSet",
    "DaemonSet",
    "Job",
    "CronJob",
    "Rollout",
];

static HAS_KIND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^kind:\s*\S+").unwrap());

pub fn discover_from_kubernetes(root: &Path, index: &FileIndex) -> Vec<Service> {
    let files = index.files_matching(&["**/*.{yml,yaml}"], &COMPOSE_PATTERNS);
    let dirs = DirectoryIndex::build(index);
    let mut found: IndexMap<String, (Service, bool)> = IndexMap::new();

    for file in files {
        let Some(text) = read_text(&root.join(&file)) else {
            continue;
        };
        if !HAS_KIND.is_match(&text) || text.contains("{{") {
            continue;
        }
        for doc in parse_documents(&text) {
            if !doc.is_map() {
                continue;
            }
            let Some(kind) = doc.get("kind").and_then(Node::as_str) else {
                continue;
            };
            let workload = WORKLOAD_KINDS.contains(&kind);
            if !workload && kind != "Service" {
                continue;
            }
            let Some(name) = doc
                .get("metadata")
                .and_then(|m| m.get("name"))
                .and_then(Node::as_scalar_string)
            else {
                continue;
            };
            let image = if workload { first_image(&doc, 0) } else { None };
            let service_root =
                dirs.matching(&[Some(&name), image_basename(image.as_deref()).as_deref()]);
            let described = service_root
                .as_ref()
                .map(|r| describe_directory(&root.join(r)));
            let detail = image
                .as_ref()
                .map_or_else(|| kind.to_string(), |i| format!("{kind}, image: {i}"));
            let service = build_service(
                name.clone(),
                service_root,
                described,
                ServiceSource::Strategy(DiscoveryStrategy::Kubernetes),
                Evidence {
                    file: file.clone(),
                    line: Some(doc.line),
                    detail: Some(detail),
                },
                image,
            );
            // A workload beats a Service object of the same name (it carries
            // the image); anything with code beats infrastructure.
            let better = match found.get(&name) {
                None => true,
                Some((prev, prev_workload)) => {
                    (workload && !prev_workload)
                        || (prev.role == ServiceRole::Infrastructure
                            && service.role == ServiceRole::Code)
                }
            };
            if better {
                found.insert(name, (service, workload));
            }
        }
    }
    found.into_values().map(|(service, _)| service).collect()
}

/// The first container image declared anywhere under this object.
fn first_image(node: &Node, depth: usize) -> Option<String> {
    if depth > 8 || !node.is_map() {
        return None;
    }
    if let Some(containers) = node.get("containers") {
        for container in containers.items() {
            if let Some(image) = container.get("image").and_then(Node::as_scalar_string) {
                return Some(image);
            }
        }
    }
    node.entries()
        .iter()
        .filter(|(_, value)| value.is_map())
        .find_map(|(_, value)| first_image(value, depth + 1))
}
