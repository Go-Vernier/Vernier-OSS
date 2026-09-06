//! What language a directory holds and where it starts. Cheap checks only:
//! manifest file names, a handful of conventional entry points. Nothing is
//! parsed beyond `package.json`.
use std::path::Path;

use crate::fs::{is_file, posix, read_json};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Manifest file relative to the directory.
    pub file: String,
    pub language: Option<String>,
}

/// Checked in order; the first hit decides the language.
const MANIFESTS: &[(&str, &str)] = &[
    ("package.json", "javascript"),
    ("go.mod", "go"),
    ("pom.xml", "java"),
    ("build.gradle", "java"),
    ("build.gradle.kts", "kotlin"),
    ("requirements.txt", "python"),
    ("pyproject.toml", "python"),
    ("Pipfile", "python"),
    ("Cargo.toml", "rust"),
    ("Gemfile", "ruby"),
    ("composer.json", "php"),
    ("mix.exs", "elixir"),
];

/// Does this directory hold a buildable service? Answers with the manifest
/// that says so. A bare Dockerfile counts: it is a service boundary even when
/// we cannot name the language.
pub fn detect_manifest(dir_abs: &Path) -> Option<Manifest> {
    for (file, language) in MANIFESTS {
        if !is_file(&dir_abs.join(file)) {
            continue;
        }
        let language = if *file == "package.json" && is_file(&dir_abs.join("tsconfig.json")) {
            "typescript"
        } else {
            language
        };
        return Some(Manifest {
            file: (*file).to_string(),
            language: Some(language.to_string()),
        });
    }
    if let Some(project) = first_project_file(dir_abs) {
        let language = if project.ends_with(".fsproj") {
            "fsharp"
        } else {
            "csharp"
        };
        return Some(Manifest {
            file: project,
            language: Some(language.to_string()),
        });
    }
    if is_file(&dir_abs.join("Dockerfile")) {
        return Some(Manifest {
            file: "Dockerfile".to_string(),
            language: None,
        });
    }
    None
}

/// The first `*.csproj` or `*.fsproj` in the directory, by name.
fn first_project_file(dir_abs: &Path) -> Option<String> {
    let mut projects: Vec<String> = std::fs::read_dir(dir_abs)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".csproj") || n.ends_with(".fsproj"))
        .collect();
    projects.sort();
    projects.into_iter().next()
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DirectoryDescription {
    pub manifest: Option<Manifest>,
    pub language: Option<String>,
    pub entry_points: Vec<String>,
    pub package_name: Option<String>,
}

pub fn describe_directory(dir_abs: &Path) -> DirectoryDescription {
    let manifest = detect_manifest(dir_abs);
    let language = manifest.as_ref().and_then(|m| m.language.clone());
    let entry_points = detect_entry_points(dir_abs, language.as_deref());
    let package_name = match &manifest {
        Some(m) if m.file == "package.json" => read_json(&dir_abs.join("package.json"))
            .and_then(|pkg| pkg.get("name")?.as_str().map(str::to_string)),
        _ => None,
    };
    DirectoryDescription {
        manifest,
        language,
        entry_points,
        package_name,
    }
}

/// Best-effort entry points, relative to the directory, sorted.
pub fn detect_entry_points(dir_abs: &Path, language: Option<&str>) -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();
    match language {
        Some("javascript" | "typescript") => {
            if let Some(pkg) = read_json(&dir_abs.join("package.json")) {
                if let Some(main) = pkg.get("main").and_then(|v| v.as_str()) {
                    candidates.push(main.to_string());
                }
                match pkg.get("bin") {
                    Some(serde_json::Value::String(bin)) => candidates.push(bin.clone()),
                    Some(serde_json::Value::Object(bins)) => {
                        candidates
                            .extend(bins.values().filter_map(|v| v.as_str()).map(str::to_string));
                    }
                    _ => {}
                }
            }
            candidates.extend(
                [
                    "src/index.ts",
                    "src/main.ts",
                    "src/server.ts",
                    "src/app.ts",
                    "src/index.js",
                    "src/main.js",
                    "src/server.js",
                    "src/app.js",
                    "index.js",
                    "server.js",
                    "app.js",
                    "main.js",
                ]
                .map(str::to_string),
            );
        }
        Some("go") => candidates.push("main.go".to_string()),
        Some("python") => candidates.extend(
            [
                "main.py",
                "app.py",
                "manage.py",
                "server.py",
                "__main__.py",
                "wsgi.py",
                "asgi.py",
            ]
            .map(str::to_string),
        ),
        Some("rust") => candidates.push("src/main.rs".to_string()),
        Some("ruby") => candidates.extend(["config.ru", "app.rb"].map(str::to_string)),
        Some("php") => candidates.extend(["public/index.php", "index.php"].map(str::to_string)),
        _ => {}
    }

    let mut found: Vec<String> = Vec::new();
    for candidate in candidates {
        let clean = posix(&crate::fs::normalize(Path::new(&candidate)));
        let clean = clean.strip_prefix("./").unwrap_or(&clean).to_string();
        if !clean.is_empty() && is_file(&dir_abs.join(&clean)) && !found.contains(&clean) {
            found.push(clean);
        }
    }
    if language == Some("go") {
        if let Ok(entries) = std::fs::read_dir(dir_abs.join("cmd")) {
            for entry in entries.flatten() {
                let main = entry.path().join("main.go");
                if is_file(&main) {
                    found.push(format!(
                        "cmd/{}/main.go",
                        entry.file_name().to_string_lossy()
                    ));
                }
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn fixture(p: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures")
            .join(p)
    }

    #[test]
    fn manifest_order_and_typescript_upgrade() {
        let m = detect_manifest(&fixture("compose-app/services/checkout")).unwrap();
        assert_eq!(
            (m.file.as_str(), m.language.as_deref()),
            ("package.json", Some("typescript"))
        );
        let m = detect_manifest(&fixture("compose-app/services/orders")).unwrap();
        assert_eq!(
            (m.file.as_str(), m.language.as_deref()),
            ("go.mod", Some("go"))
        );
        let m = detect_manifest(&fixture("k8s-app/src/cartservice")).unwrap();
        assert_eq!(
            (m.file.as_str(), m.language.as_deref()),
            ("cartservice.csproj", Some("csharp"))
        );
        assert!(detect_manifest(&fixture("monorepo-app/services/docs-only")).is_none());
        assert!(detect_manifest(&fixture("missing-dir")).is_none());
    }

    #[test]
    fn describes_entry_points_and_package_name() {
        let d = describe_directory(&fixture("compose-app/services/checkout"));
        assert_eq!(d.package_name.as_deref(), Some("@acme/checkout"));
        assert!(d.entry_points.contains(&"src/index.ts".to_string()));
        let d = describe_directory(&fixture("compose-app/services/orders"));
        assert_eq!(d.entry_points, vec!["main.go"]);
        let d = describe_directory(&fixture("monorepo-app/services/worker"));
        assert_eq!(
            (d.language.as_deref(), d.entry_points),
            (Some("python"), vec!["main.py".to_string()])
        );
        let d = describe_directory(&fixture("monorepo-app/services/docs-only"));
        assert!(d.manifest.is_none() && d.language.is_none() && d.entry_points.is_empty());
    }
}
