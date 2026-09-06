//! Strategy 1: docker-compose. Each key under `services:` is a service.
//! `${VAR}` references are resolved from the `.env` beside the compose file.
//!
//! Its directory, in order of preference:
//! - the directory of `build.dockerfile` when that is a subdirectory of the
//!   build context (monorepos build every service from the repository root
//!   with `context: .` and `dockerfile: src/<name>/Dockerfile`)
//! - when the context is the repository root, a directory matching the
//!   service name (the Dockerfile variable could not be resolved)
//! - `build.context` (or a string `build:`)
//! - for `image:`-only services, a directory matching the service name or
//!   the image name; otherwise the service is infrastructure the repository
//!   runs but does not build
use std::path::Path;

use indexmap::IndexMap;

use super::build_service;
use super::directories::{DirectoryIndex, image_basename};
use super::env::{interpolate, load_compose_env};
use super::language::describe_directory;
use crate::fs::{FileIndex, is_dir, is_file, normalize, read_text, rel};
use crate::model::{DiscoveryStrategy, Evidence, Service, ServiceRole, ServiceSource};
use crate::yaml::{Node, parse_documents};

pub const COMPOSE_PATTERNS: [&str; 2] = ["**/docker-compose*.{yml,yaml}", "**/compose.{yml,yaml}"];

pub fn discover_from_compose(root: &Path, index: &FileIndex) -> Vec<Service> {
    let files = index.files_matching(&COMPOSE_PATTERNS, &[]);
    if files.is_empty() {
        return Vec::new();
    }
    let dirs = DirectoryIndex::build(index);
    let mut found: IndexMap<String, Service> = IndexMap::new();

    for file in files {
        let Some(text) = read_text(&root.join(&file)) else {
            continue;
        };
        let docs = parse_documents(&text);
        let Some(services) = docs.first().and_then(|d| d.get("services")) else {
            continue;
        };
        if !services.is_map() {
            continue;
        }
        let compose_dir = root
            .join(&file)
            .parent()
            .map_or_else(|| root.to_path_buf(), Path::to_path_buf);
        let env = load_compose_env(&compose_dir);

        for (key, def) in services.entries() {
            let Some(name) = key.as_scalar_string() else {
                continue;
            };
            let service =
                describe_service(root, &compose_dir, &dirs, &file, key.line, &name, def, &env);
            let replace = match found.get(&name) {
                None => true,
                Some(prev) => {
                    prev.role == ServiceRole::Infrastructure && service.role == ServiceRole::Code
                }
            };
            if replace {
                found.insert(name, service);
            }
        }
    }
    found.into_values().collect()
}

#[allow(clippy::too_many_arguments)]
fn describe_service(
    root: &Path,
    compose_dir: &Path,
    dirs: &DirectoryIndex<'_>,
    file: &str,
    line: u32,
    name: &str,
    def: &Node,
    env: &super::env::Env,
) -> Service {
    let image = def
        .get("image")
        .and_then(Node::as_scalar_string)
        .map(|v| interpolate(&v, env));

    let mut context: Option<String> = None;
    let mut dockerfile: Option<String> = None;
    if let Some(build) = def.get("build") {
        if let Some(text) = build.as_scalar_string() {
            context = Some(interpolate(&text, env));
        } else if build.is_map() {
            context = build
                .get("context")
                .and_then(Node::as_scalar_string)
                .map(|v| interpolate(&v, env));
            dockerfile = build
                .get("dockerfile")
                .and_then(Node::as_scalar_string)
                .map(|v| interpolate(&v, env));
            if context.is_none() && dockerfile.is_some() {
                context = Some(".".to_string());
            }
        }
    }

    let mut service_root: Option<String> = None;
    let detail: String;
    if let Some(context) = &context {
        let context_abs = normalize(&compose_dir.join(context));
        let mut chosen = format!("build: {context}");
        if let Some(dockerfile) = &dockerfile {
            let dockerfile_abs = normalize(&context_abs.join(dockerfile));
            if let Some(dockerfile_dir) = dockerfile_abs.parent() {
                if dockerfile_dir != context_abs && is_file(&dockerfile_abs) {
                    service_root = rel(root, dockerfile_dir);
                    chosen = format!("build: {context}, dockerfile: {dockerfile}");
                }
            }
        }
        if service_root.is_none() && context_abs == root {
            if let Some(matched) = dirs.matching(&[Some(name)]) {
                if matched != "." {
                    chosen = format!("build: {context}, matched directory {matched}");
                    service_root = Some(matched);
                }
            }
        }
        if service_root.is_none() && is_dir(&context_abs) {
            service_root = rel(root, &context_abs);
        }
        detail = chosen;
    } else {
        service_root = dirs.matching(&[Some(name), image_basename(image.as_deref()).as_deref()]);
        detail = image
            .as_ref()
            .map_or_else(|| "service".to_string(), |i| format!("image: {i}"));
    }

    let described = service_root
        .as_ref()
        .map(|r| describe_directory(&root.join(r)));
    build_service(
        name.to_string(),
        service_root,
        described,
        ServiceSource::Strategy(DiscoveryStrategy::DockerCompose),
        Evidence {
            file: file.to_string(),
            line: Some(line),
            detail: Some(detail),
        },
        image,
    )
}
