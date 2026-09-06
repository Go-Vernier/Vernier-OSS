//! Strategy 4: workspace configuration lists member directories explicitly:
//! package.json `workspaces`, pnpm-workspace.yaml `packages`, Cargo
//! `[workspace] members`, and Nx `project.json` files. turbo.json rides on
//! package.json workspaces, so it needs no handling of its own.
use std::path::Path;

use indexmap::IndexMap;

use super::build_service;
use super::language::describe_directory;
use crate::fs::{FileIndex, read_json, read_text};
use crate::model::{DiscoveryStrategy, Evidence, Service, ServiceSource};
use crate::yaml::{Node, parse_documents};

/// A member pattern and the file that declared it.
struct Declared {
    pattern: String,
    file: String,
}

pub fn discover_from_workspace(root: &Path, index: &FileIndex) -> Vec<Service> {
    let mut patterns: Vec<Declared> = Vec::new();

    if let Some(pkg) = read_json(&root.join("package.json")) {
        let list = match pkg.get("workspaces") {
            Some(serde_json::Value::Array(list)) => list.clone(),
            Some(serde_json::Value::Object(obj)) => obj
                .get("packages")
                .and_then(|p| p.as_array())
                .cloned()
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        patterns.extend(list.iter().filter_map(|p| p.as_str()).map(|p| Declared {
            pattern: p.to_string(),
            file: "package.json".to_string(),
        }));
    }

    if let Some(text) = read_text(&root.join("pnpm-workspace.yaml")) {
        if let Some(packages) = parse_documents(&text)
            .first()
            .and_then(|d| d.get("packages"))
        {
            patterns.extend(
                packages
                    .items()
                    .iter()
                    .filter_map(Node::as_str)
                    .map(|p| Declared {
                        pattern: p.to_string(),
                        file: "pnpm-workspace.yaml".to_string(),
                    }),
            );
        }
    }

    if let Some(text) = read_text(&root.join("Cargo.toml")) {
        patterns.extend(cargo_members(&text).into_iter().map(|m| Declared {
            pattern: m,
            file: "Cargo.toml".to_string(),
        }));
    }

    // directory -> which declaration listed it
    let mut dirs: IndexMap<String, Declared> = IndexMap::new();
    for project in index.files_matching(&["**/project.json"], &[]) {
        if let Some((dir, _)) = project.rsplit_once('/') {
            dirs.insert(
                dir.to_string(),
                Declared {
                    pattern: project.clone(),
                    file: project.clone(),
                },
            );
        }
    }

    let exclude: Vec<&str> = patterns
        .iter()
        .filter_map(|p| p.pattern.strip_prefix('!'))
        .collect();
    for declared in patterns.iter().filter(|p| !p.pattern.starts_with('!')) {
        for dir in index.dirs_matching(&[declared.pattern.as_str()], &exclude) {
            if dir != "." && !dirs.contains_key(&dir) {
                dirs.insert(
                    dir,
                    Declared {
                        pattern: declared.pattern.clone(),
                        file: declared.file.clone(),
                    },
                );
            }
        }
    }

    let mut ordered: Vec<(&String, &Declared)> = dirs.iter().collect();
    ordered.sort_by(|a, b| a.0.cmp(b.0));

    let mut found: IndexMap<String, Service> = IndexMap::new();
    for (dir, source) in ordered {
        let described = describe_directory(&root.join(dir));
        if described.manifest.is_none() {
            continue;
        }
        let name = dir.rsplit('/').next().unwrap_or(dir).to_string();
        if found.contains_key(&name) {
            continue;
        }
        let service = build_service(
            name.clone(),
            Some(dir.clone()),
            Some(described),
            ServiceSource::Strategy(DiscoveryStrategy::Workspace),
            Evidence {
                file: source.file.clone(),
                line: None,
                detail: Some(source.pattern.clone()),
            },
            None,
        );
        found.insert(name, service);
    }
    found.into_values().collect()
}

/// `[workspace] members = ["a", "crates/*"]`.
pub fn cargo_members(toml_text: &str) -> Vec<String> {
    toml::from_str::<toml::Table>(toml_text)
        .ok()
        .and_then(|table| {
            let members = table
                .get("workspace")?
                .as_table()?
                .get("members")?
                .as_array()?;
            Some(
                members
                    .iter()
                    .filter_map(|m| m.as_str().map(str::to_string))
                    .collect(),
            )
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses_cargo_workspace_members() {
        let toml = "[package]\nname = \"root\"\n\n[workspace]\nmembers = [\n  \"crates/api\", # the api\n  'crates/worker',\n  \"tools/*\"\n]\n";
        assert_eq!(
            cargo_members(toml),
            vec!["crates/api", "crates/worker", "tools/*"]
        );
        assert_eq!(cargo_members("[package]\nname = 'x'"), Vec::<String>::new());
        assert_eq!(cargo_members("not = [valid"), Vec::<String>::new());
    }
}
