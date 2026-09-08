//! What the repository's configuration says about each service's
//! environment, plus the gRPC services its `.proto` files define. This is
//! how an environment variable read in code becomes a hostname: the compose
//! file, the Kubernetes manifest, a `ConfigMap` or a dotenv file said so.
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use indexmap::IndexMap;
use regex::Regex;

use crate::discover::compose::COMPOSE_PATTERNS;
use crate::discover::env::{Env, interpolate, load_compose_env, parse_dotenv_lines};
use crate::fs::{FileIndex, read_text, rel};
use crate::model::{Evidence, Service, ServiceRole};
use crate::yaml::{Node, parse_documents};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvValue {
    pub value: String,
    pub evidence: Evidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtoService {
    pub name: String,
    pub package: Option<String>,
    pub evidence: Evidence,
}

/// A name a service can be imported by: its package.json name, Go module
/// path, Cargo or Maven artifact, .csproj stem, or Python directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    pub service: String,
    pub evidence: Evidence,
}

#[derive(Debug, Default)]
pub struct ConfigIndex {
    /// service name -> VAR -> value, from compose `environment` and
    /// `env_file`, and Kubernetes container `env`.
    per_service: HashMap<String, IndexMap<String, EnvValue>>,
    /// `ConfigMap` data and dotenv files at the root. First writer wins,
    /// except `.env` which overrides `.env.example`.
    global: IndexMap<String, EnvValue>,
    pub protos: Vec<ProtoService>,
    /// Names of discovered infrastructure services.
    pub infrastructure_names: Vec<String>,
    /// Names of every discovered service, for the bare-name rule.
    pub service_names: Vec<String>,
    /// Package/module names each service is importable by, kept empty
    /// until a later stage fills it in.
    pub packages: Vec<Package>,
}

const WORKLOAD_KINDS: &[&str] = &[
    "Deployment",
    "StatefulSet",
    "DaemonSet",
    "Job",
    "CronJob",
    "Rollout",
    "Pod",
];

static HAS_KIND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^kind:\s*\S+").unwrap());
static PROTO_SERVICE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*service\s+(\w+)\s*\{").unwrap());
static PROTO_PACKAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*package\s+([\w.]+)\s*;").unwrap());
static GO_MODULE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^module\s+(\S+)").unwrap());
static POM_PARENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<parent>.*?</parent>").unwrap());
static POM_ARTIFACT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<artifactId>\s*([^<\s]+)\s*</artifactId>").unwrap());

impl ConfigIndex {
    pub fn build(root: &Path, index: &FileIndex, services: &[Service]) -> Self {
        let mut cfg = Self {
            infrastructure_names: services
                .iter()
                .filter(|s| s.role == ServiceRole::Infrastructure)
                .map(|s| s.name.clone())
                .collect(),
            service_names: services.iter().map(|s| s.name.clone()).collect(),
            ..Self::default()
        };
        cfg.read_compose(root, index);
        cfg.read_kubernetes(root, index);
        cfg.read_dotenv(root);
        cfg.read_protos(root, index);
        cfg.read_packages(root, services);
        cfg
    }

    /// The calling service's own configuration first, then the global pool.
    pub fn lookup(&self, service: &str, var: &str) -> Option<&EnvValue> {
        self.per_service
            .get(service)
            .and_then(|vars| vars.get(var))
            .or_else(|| self.global.get(var))
    }

    fn insert_service(&mut self, service: &str, var: String, value: EnvValue) {
        self.per_service
            .entry(service.to_string())
            .or_default()
            .entry(var)
            .or_insert(value);
    }

    fn read_compose(&mut self, root: &Path, index: &FileIndex) {
        for file in index.files_matching(&COMPOSE_PATTERNS, &[]) {
            let Some(text) = read_text(&root.join(&file)) else {
                continue;
            };
            let docs = parse_documents(&text);
            let Some(services) = docs.first().and_then(|d| d.get("services")) else {
                continue;
            };
            let compose_dir = root
                .join(&file)
                .parent()
                .map_or_else(|| root.to_path_buf(), Path::to_path_buf);
            let env = load_compose_env(&compose_dir);
            for (key, def) in services.entries() {
                let Some(name) = key.as_scalar_string() else {
                    continue;
                };
                if let Some(environment) = def.get("environment") {
                    self.read_environment_node(&name, environment, &file, &env);
                }
                if let Some(env_file) = def.get("env_file") {
                    let paths: Vec<String> = match env_file.as_scalar_string() {
                        Some(single) => vec![single],
                        None => env_file
                            .items()
                            .iter()
                            .filter_map(Node::as_scalar_string)
                            .collect(),
                    };
                    for path in paths {
                        let abs = crate::fs::normalize(&compose_dir.join(interpolate(&path, &env)));
                        let Some(relative) = rel(root, &abs) else {
                            continue;
                        };
                        let Some(dotenv) = read_text(&abs) else {
                            continue;
                        };
                        for (var, value, line) in parse_dotenv_lines(&dotenv) {
                            self.insert_service(
                                &name,
                                var,
                                EnvValue {
                                    value,
                                    evidence: Evidence {
                                        file: relative.clone(),
                                        line: Some(line),
                                        detail: None,
                                    },
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    /// `environment:` as a mapping or as a list of `KEY=value` strings.
    fn read_environment_node(&mut self, service: &str, node: &Node, file: &str, env: &Env) {
        if node.is_map() {
            for (k, v) in node.entries() {
                let (Some(var), Some(value)) = (k.as_scalar_string(), v.as_scalar_string()) else {
                    continue;
                };
                self.insert_service(
                    service,
                    var,
                    EnvValue {
                        value: interpolate(&value, env),
                        evidence: Evidence {
                            file: file.to_string(),
                            line: Some(k.line),
                            detail: None,
                        },
                    },
                );
            }
            return;
        }
        for item in node.items() {
            let Some(text) = item.as_scalar_string() else {
                continue;
            };
            let (var, value) = if let Some((var, value)) = text.split_once('=') {
                (var.trim().to_string(), interpolate(value.trim(), env))
            } else {
                let var = text.trim();
                let Some(value) = env.get(var) else {
                    continue;
                };
                (var.to_string(), value.clone())
            };
            self.insert_service(
                service,
                var,
                EnvValue {
                    value,
                    evidence: Evidence {
                        file: file.to_string(),
                        line: Some(item.line),
                        detail: None,
                    },
                },
            );
        }
    }

    fn read_kubernetes(&mut self, root: &Path, index: &FileIndex) {
        for file in index.files_matching(&["**/*.{yml,yaml}"], &COMPOSE_PATTERNS) {
            let Some(text) = read_text(&root.join(&file)) else {
                continue;
            };
            if !HAS_KIND.is_match(&text) || text.contains("{{") {
                continue;
            }
            for doc in parse_documents(&text) {
                let Some(kind) = doc.get("kind").and_then(Node::as_str) else {
                    continue;
                };
                let name = doc
                    .get("metadata")
                    .and_then(|m| m.get("name"))
                    .and_then(Node::as_scalar_string);
                if kind == "ConfigMap" {
                    if let Some(data) = doc.get("data") {
                        for (k, v) in data.entries() {
                            let (Some(var), Some(value)) =
                                (k.as_scalar_string(), v.as_scalar_string())
                            else {
                                continue;
                            };
                            self.global.entry(var).or_insert(EnvValue {
                                value,
                                evidence: Evidence {
                                    file: file.clone(),
                                    line: Some(k.line),
                                    detail: name.clone().map(|n| format!("ConfigMap {n}")),
                                },
                            });
                        }
                    }
                    continue;
                }
                if !WORKLOAD_KINDS.contains(&kind) {
                    continue;
                }
                let Some(name) = name else {
                    continue;
                };
                let mut found: Vec<(String, String, u32)> = Vec::new();
                collect_container_env(&doc, 0, &mut found);
                for (var, value, line) in found {
                    self.insert_service(
                        &name,
                        var,
                        EnvValue {
                            value,
                            evidence: Evidence {
                                file: file.clone(),
                                line: Some(line),
                                detail: None,
                            },
                        },
                    );
                }
            }
        }
    }

    fn read_dotenv(&mut self, root: &Path) {
        let mut seen = Env::new();
        for file in [".env.example", ".env"] {
            let Some(text) = read_text(&root.join(file)) else {
                continue;
            };
            for (var, value, line) in parse_dotenv_lines(&text) {
                let value = interpolate(&value, &seen);
                seen.insert(var.clone(), value.clone());
                self.global.insert(
                    var,
                    EnvValue {
                        value,
                        evidence: Evidence {
                            file: file.to_string(),
                            line: Some(line),
                            detail: None,
                        },
                    },
                );
            }
        }
    }

    fn read_protos(&mut self, root: &Path, index: &FileIndex) {
        for file in index.files_matching(&["**/*.proto"], &[]) {
            let Some(text) = read_text(&root.join(&file)) else {
                continue;
            };
            let package = PROTO_PACKAGE.captures(&text).map(|c| c[1].to_string());
            for caps in PROTO_SERVICE.captures_iter(&text) {
                let offset = caps.get(1).map_or(0, |m| m.start());
                let line =
                    u32::try_from(text[..offset].matches('\n').count() + 1).unwrap_or(u32::MAX);
                self.protos.push(ProtoService {
                    name: caps[1].to_string(),
                    package: package.clone(),
                    evidence: Evidence {
                        file: file.clone(),
                        line: Some(line),
                        detail: Some(format!("service {}", &caps[1])),
                    },
                });
            }
        }
    }

    /// The names each code service can be imported by, from its manifest.
    #[allow(clippy::too_many_lines)]
    fn read_packages(&mut self, root: &Path, services: &[Service]) {
        let mut discovered: Vec<Package> = Vec::new();
        for s in services.iter().filter(|s| s.role == ServiceRole::Code) {
            let Some(dir) = s.root.as_deref() else {
                continue;
            };
            let mut add = |name: String, file: String| {
                let name = name.trim().to_string();
                if name.is_empty()
                    || discovered
                        .iter()
                        .any(|p| p.name == name && p.service == s.name)
                {
                    return;
                }
                discovered.push(Package {
                    name,
                    service: s.name.clone(),
                    evidence: Evidence {
                        file,
                        line: None,
                        detail: None,
                    },
                });
            };
            let at = |file: &str| {
                if dir == "." {
                    file.to_string()
                } else {
                    format!("{dir}/{file}")
                }
            };
            if let Some(name) = &s.package_name {
                add(name.clone(), at("package.json"));
            }
            if let Some(pkg) = crate::fs::read_json(&root.join(dir).join("package.json")) {
                if let Some(name) = pkg.get("name").and_then(|v| v.as_str()) {
                    add(name.to_string(), at("package.json"));
                }
            }
            if let Some(text) = read_text(&root.join(dir).join("go.mod")) {
                if let Some(caps) = GO_MODULE.captures(&text) {
                    add(caps[1].to_string(), at("go.mod"));
                }
            }
            if let Some(text) = read_text(&root.join(dir).join("Cargo.toml")) {
                if let Some(name) = toml::from_str::<toml::Table>(&text).ok().and_then(|t| {
                    t.get("package")?
                        .as_table()?
                        .get("name")?
                        .as_str()
                        .map(str::to_string)
                }) {
                    add(name, at("Cargo.toml"));
                }
            }
            if let Some(text) = read_text(&root.join(dir).join("pom.xml")) {
                let own = POM_PARENT.replace(&text, "");
                if let Some(caps) = POM_ARTIFACT.captures(&own) {
                    add(caps[1].to_string(), at("pom.xml"));
                }
            }
            if let Some(pkg) = crate::fs::read_json(&root.join(dir).join("composer.json")) {
                if let Some(name) = pkg.get("name").and_then(|v| v.as_str()) {
                    add(name.to_string(), at("composer.json"));
                }
            }
            if let Some(text) = read_text(&root.join(dir).join("pyproject.toml")) {
                if let Ok(t) = toml::from_str::<toml::Table>(&text) {
                    let name = t
                        .get("project")
                        .and_then(|p| p.get("name"))
                        .or_else(|| t.get("tool")?.get("poetry")?.get("name"))
                        .and_then(|v| v.as_str());
                    if let Some(name) = name {
                        add(name.to_string(), at("pyproject.toml"));
                    }
                }
            }
            if let Ok(entries) = std::fs::read_dir(root.join(dir)) {
                let mut projects: Vec<String> = entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.ends_with(".csproj") || n.ends_with(".fsproj"))
                    .collect();
                projects.sort();
                for project in projects {
                    let stem = project
                        .rsplit_once('.')
                        .map_or(project.as_str(), |(s, _)| s)
                        .to_string();
                    add(stem, at(&project));
                }
            }
            if s.language.as_deref() == Some("python") {
                let base = dir.rsplit('/').next().unwrap_or(dir).to_string();
                let evidence_file = if root.join(dir).join("requirements.txt").is_file() {
                    at("requirements.txt")
                } else if root.join(dir).join("pyproject.toml").is_file() {
                    at("pyproject.toml")
                } else {
                    dir.to_string()
                };
                add(base, evidence_file);
            }
        }
        self.packages.extend(discovered);
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn insert_for_test(
        &mut self,
        service: &str,
        var: &str,
        value: &str,
        file: &str,
        line: u32,
    ) {
        self.insert_service(
            service,
            var.to_string(),
            EnvValue {
                value: value.to_string(),
                evidence: Evidence {
                    file: file.to_string(),
                    line: Some(line),
                    detail: None,
                },
            },
        );
    }
}

/// Every `env` entry with a literal `value` under any `containers` list.
fn collect_container_env(node: &Node, depth: usize, out: &mut Vec<(String, String, u32)>) {
    if depth > 10 || !node.is_map() {
        return;
    }
    for key in ["containers", "initContainers"] {
        if let Some(containers) = node.get(key) {
            for container in containers.items() {
                let Some(env) = container.get("env") else {
                    continue;
                };
                for item in env.items() {
                    let (Some(var), Some(value)) = (
                        item.get("name").and_then(Node::as_scalar_string),
                        item.get("value").and_then(Node::as_scalar_string),
                    ) else {
                        continue;
                    };
                    out.push((var, value, item.line));
                }
            }
        }
    }
    for (_, value) in node.entries() {
        if value.is_map() {
            collect_container_env(value, depth + 1, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures")
            .join(name)
            .canonicalize()
            .unwrap()
    }

    fn build(name: &str) -> ConfigIndex {
        let root = fixture(name);
        let index = FileIndex::build(&root);
        let services = crate::discover::discover_services(&root, &index).services;
        ConfigIndex::build(&root, &index, &services)
    }

    #[test]
    fn compose_environment_per_service_and_dotenv_global() {
        let cfg = build("edges-http-app");
        let v = cfg.lookup("web", "CATALOGUE_HOST").unwrap();
        assert_eq!(v.value, "catalogue");
        assert_eq!(
            (v.evidence.file.as_str(), v.evidence.line),
            ("docker-compose.yml", Some(5))
        );
        assert_eq!(
            cfg.lookup("cart", "REDIS_HOST").map(|v| v.value.as_str()),
            Some("redis")
        );
        assert_eq!(
            cfg.lookup("cart", "REDIS_HOST")
                .and_then(|v| v.evidence.line),
            Some(10)
        );
        assert_eq!(
            cfg.lookup("nobody", "GLOBAL_THING")
                .map(|v| v.value.as_str()),
            Some("from-dotenv")
        );
        assert!(cfg.lookup("web", "MISSING").is_none());
        assert_eq!(
            cfg.infrastructure_names,
            vec!["mongodb", "rabbitmq", "redis"]
        );
    }

    #[test]
    fn kubernetes_env_configmaps_and_protos() {
        let cfg = build("edges-grpc-app");
        assert_eq!(
            cfg.lookup("frontend", "CART_SERVICE_ADDR")
                .map(|v| v.value.as_str()),
            Some("cartservice:7070")
        );
        assert_eq!(
            cfg.lookup("frontend", "FROM_CONFIGMAP")
                .map(|v| v.value.as_str()),
            Some("emailservice:5000")
        );
        assert_eq!(
            cfg.lookup("checkoutservice", "EMAIL_SERVICE_ADDR")
                .map(|v| v.value.as_str()),
            Some("emailservice:5000")
        );
        let names: Vec<&str> = cfg.protos.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["CartService", "EmailService", "ShippingService"]
        );
        assert_eq!(cfg.protos[0].evidence.file, "protos/demo.proto");
        assert_eq!(cfg.protos[0].evidence.line, Some(5));
        assert_eq!(cfg.protos[0].package.as_deref(), Some("hipstershop"));
    }

    #[test]
    fn dotenv_values_interpolate_and_bare_compose_keys_pass_through() {
        let cfg = build("edges-db-app");
        assert_eq!(
            cfg.lookup("cart", "VALKEY_ADDR").map(|v| v.value.as_str()),
            Some("valkey-cart:6379")
        );
        assert_eq!(
            cfg.lookup("cart", "VALKEY_ADDR")
                .map(|v| v.evidence.file.as_str()),
            Some("docker-compose.yml")
        );
        assert_eq!(
            cfg.lookup("reports", "DB_CONNECTION_STRING")
                .map(|v| v.value.as_str()),
            Some("postgres://app:secret@postgres/shop?sslmode=disable")
        );
        assert_eq!(
            cfg.lookup("nobody", "VALKEY_ADDR")
                .map(|v| v.value.as_str()),
            Some("valkey-cart:6379"),
            "dotenv values are global and interpolated against their own file"
        );
    }

    #[test]
    fn packages_are_read_from_every_manifest_kind() {
        let cfg = build("edges-import-app");
        let mut got: Vec<(&str, &str)> = cfg
            .packages
            .iter()
            .map(|p| (p.name.as_str(), p.service.as_str()))
            .collect();
        got.sort_unstable();
        assert_eq!(
            got,
            vec![
                ("@acme/shared", "shared"),
                ("Basket.API", "basket-api"),
                ("EventBus", "eventbus"),
                ("common", "common"),
                ("core-rs", "core-rs"),
                ("github.com/acme/demo/services/cart", "cart"),
                ("github.com/acme/demo/services/checkout", "checkout"),
                ("indexer", "indexer"),
                ("orders", "orders"),
                ("shared_py", "shared_py"),
                ("web", "web"),
                ("worker", "worker"),
            ]
        );
        let p = cfg.packages.iter().find(|p| p.name == "orders").unwrap();
        assert_eq!(p.evidence.file, "services/orders/pom.xml");
    }
}
