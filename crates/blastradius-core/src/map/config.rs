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
            let Some((var, value)) = text.split_once('=') else {
                continue;
            };
            self.insert_service(
                service,
                var.trim().to_string(),
                EnvValue {
                    value: interpolate(value.trim(), env),
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
        for file in [".env.example", ".env"] {
            let Some(text) = read_text(&root.join(file)) else {
                continue;
            };
            for (var, value, line) in parse_dotenv_lines(&text) {
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
}
