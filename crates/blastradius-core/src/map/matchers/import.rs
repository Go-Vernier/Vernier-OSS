//! Cross-service imports: a package another discovered service declares,
//! imported in code or listed as a dependency; a project or path reference
//! from one manifest to another service's directory. These are the most
//! reliable edges there are, so they are Static. An import that matches no
//! discovered package is an external library and is dropped without a trace.
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::{FileContext, Matcher};
use crate::fs::{normalize, posix};
use crate::map::facts::Fact;
use crate::map::{Candidate, Target};
use crate::model::{EdgeType, Evidence};

pub struct Import;

/// npm, composer, `PyPI` and Cargo names: `@acme/shared`, `vendor/pkg`, `core-rs`.
static PACKAGE_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(@[a-z0-9][\w.-]*/)?[a-z0-9][\w.-]*$").unwrap());

fn evidence(ctx: &FileContext<'_>, line: u32, detail: &str) -> Evidence {
    Evidence {
        file: ctx.file.to_string(),
        line: Some(line),
        detail: Some(detail.to_string()),
    }
}

fn package(ctx: &FileContext<'_>, name: &str, how: &str, line: u32) -> Candidate {
    Candidate {
        target: Target::Package {
            name: name.to_string(),
            how: how.to_string(),
        },
        kind_hint: Some(EdgeType::Import),
        evidence: evidence(ctx, line, how),
    }
}

/// `..\EventBus\EventBus.csproj` next to `src/Basket.API/Basket.API.csproj`
/// -> `src/EventBus/EventBus.csproj`.
fn resolve_relative(file: &str, relative: &str) -> String {
    let dir = file.rsplit_once('/').map_or("", |(d, _)| d);
    let joined = Path::new(dir).join(relative.replace('\\', "/"));
    let clean = posix(&normalize(&joined));
    clean.trim_start_matches("./").to_string()
}

fn path_candidate(ctx: &FileContext<'_>, relative: &str, how: &str, line: u32) -> Candidate {
    Candidate {
        target: Target::PackagePath {
            path: resolve_relative(ctx.file, relative),
            how: how.to_string(),
        },
        kind_hint: Some(EdgeType::Import),
        evidence: evidence(ctx, line, how),
    }
}

impl Matcher for Import {
    fn name(&self) -> &'static str {
        "import"
    }

    fn candidates(&self, ctx: &FileContext<'_>) -> Vec<Candidate> {
        let mut out = Vec::new();
        let basename = ctx
            .file
            .rsplit('/')
            .next()
            .unwrap_or(ctx.file)
            .to_lowercase();
        let is_project = basename.ends_with(".csproj") || basename.ends_with(".fsproj");
        for fact in ctx.facts {
            match fact {
                Fact::Import { path, line } => {
                    let p = path.trim();
                    if p.is_empty() || p.starts_with(['.', '/']) || p.contains("://") {
                        continue;
                    }
                    out.push(package(ctx, p, &format!("import {p}"), *line));
                }
                Fact::Str { value, line } => {
                    let v = value.trim();
                    let lower = v.to_lowercase();
                    if is_project && (lower.ends_with(".csproj") || lower.ends_with(".fsproj")) {
                        out.push(path_candidate(
                            ctx,
                            v,
                            &format!("ProjectReference {v}"),
                            *line,
                        ));
                    } else if basename == "cargo.toml"
                        && (v.starts_with("./") || v.starts_with("../"))
                    {
                        out.push(path_candidate(ctx, v, &format!("path {v}"), *line));
                    } else if matches!(basename.as_str(), "package.json" | "composer.json")
                        && PACKAGE_NAME.is_match(v)
                    {
                        out.push(package(ctx, v, &format!("dependency {v}"), *line));
                    } else if basename == "pyproject.toml" {
                        let name = v
                            .split([' ', '<', '>', '=', '!', '~', ';', '['])
                            .next()
                            .unwrap_or(v);
                        if PACKAGE_NAME.is_match(name) {
                            out.push(package(ctx, name, &format!("dependency {v}"), *line));
                        }
                    }
                }
                Fact::Setting { key, value, line }
                    if basename == "pom.xml" && (key == "artifactId" || key == "module") =>
                {
                    out.push(package(ctx, value, &format!("{key} {value}"), *line));
                }
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_project_paths_resolve_against_the_manifest_directory() {
        assert_eq!(
            resolve_relative(
                "src/Basket.API/Basket.API.csproj",
                "..\\EventBus\\EventBus.csproj"
            ),
            "src/EventBus/EventBus.csproj"
        );
        assert_eq!(
            resolve_relative(
                "src/HybridApp/HybridApp.csproj",
                "..\\..\\src\\WebAppComponents\\WebAppComponents.csproj"
            ),
            "src/WebAppComponents/WebAppComponents.csproj"
        );
        assert_eq!(
            resolve_relative("services/indexer/Cargo.toml", "../core-rs"),
            "services/core-rs"
        );
        assert_eq!(
            resolve_relative("Cargo.toml", "./crates/core"),
            "crates/core"
        );
    }
}
