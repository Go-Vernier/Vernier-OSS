//! Generated gRPC stubs on the client side, server registrations on the
//! owning side. The service that registers a proto service owns it; when
//! nothing registers it, the service whose name normalises to the proto
//! service's name does.
use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use super::{FileContext, Matcher};
use crate::discover::directories::normalise;
use crate::map::config::ConfigIndex;
use crate::map::facts::{Arg, Extraction, Fact};
use crate::map::{Candidate, Target};
use crate::model::{EdgeType, Evidence, Service};

pub struct Grpc;

static CLIENT_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"New(\w+)Client$",
        r"^new\s+(?:[\w.]+\.)?(\w+)Client$",
        r"(\w+)Grpc\.new\w*Stub$",
        r"(\w+)Stub$",
        r"AddGrpcClient<(?:[\w.]+\.)?(\w+)Client>",
        r"(\w+)Client\.create$",
    ]
    .iter()
    .map(|p| Regex::new(p).unwrap())
    .collect()
});

static SERVER_CALL_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"Register(\w+)Server$",
        r"add_(\w+)Servicer_to_server$",
        r"(\w+)Grpc\.bindService$",
    ]
    .iter()
    .map(|p| Regex::new(p).unwrap())
    .collect()
});

static SERVER_EXTENDS_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"(\w+)Grpc\.\w*ImplBase$",
        r"(?:[\w.]+\.)?(\w+)\.\w+Base$",
        r"^(\w+)Servicer$",
        r"(\w+)ServiceServer$",
    ]
    .iter()
    .map(|p| Regex::new(p).unwrap())
    .collect()
});

static ADD_SERVICE_ARG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.(\w+)\.service\b").unwrap());

const NOISE: &[&str] = &[
    "Health",
    "Reflection",
    "Grpc",
    "Channel",
    "Http",
    "Web",
    "Rest",
    "Api",
    "Service",
    "Base",
    "Redis",
    "Mongo",
    "Sql",
    "Db",
    "Kafka",
    "Amqp",
    "Client",
    "Test",
    "Mock",
    "Fake",
];

fn candidate_name(captured: &str) -> Option<String> {
    let name = captured.trim();
    if name.len() < 3 || NOISE.contains(&name) {
        return None;
    }
    Some(name.to_string())
}

/// Proto service names a client stub construction refers to.
pub fn client_names(callee: &str) -> Vec<String> {
    let mut out = Vec::new();
    for re in CLIENT_PATTERNS.iter() {
        if let Some(caps) = re.captures(callee) {
            if let Some(name) = candidate_name(&caps[1]) {
                if !out.contains(&name) {
                    out.push(name);
                }
            }
        }
    }
    out
}

/// Proto service names a file registers a server for.
pub fn server_names(fact: &Fact) -> Vec<String> {
    let mut out = Vec::new();
    match fact {
        Fact::Call { callee, args, .. } => {
            for re in SERVER_CALL_PATTERNS.iter() {
                if let Some(caps) = re.captures(callee) {
                    out.extend(candidate_name(&caps[1]));
                }
            }
            if callee.ends_with("addService") {
                for arg in args {
                    if let Arg::Other(text) = arg {
                        if let Some(caps) = ADD_SERVICE_ARG.captures(text) {
                            out.extend(candidate_name(&caps[1]));
                        }
                    }
                }
            }
        }
        Fact::Extends { name, .. } => {
            for re in SERVER_EXTENDS_PATTERNS.iter() {
                if let Some(caps) = re.captures(name) {
                    out.extend(candidate_name(&caps[1]));
                    break;
                }
            }
        }
        _ => {}
    }
    out
}

/// proto service name -> owning service. Registration wins; otherwise the
/// discovered service whose normalised name equals the proto's. Client
/// names that normalise to a discovered service are owned by it too, so a
/// stub resolves even when the repository has no `.proto` file.
pub fn proto_owners(
    extractions: &[(String, String, Extraction)],
    config: &ConfigIndex,
    services: &[Service],
) -> HashMap<String, String> {
    let mut owners: HashMap<String, String> = HashMap::new();
    for (service, _, ex) in extractions {
        for fact in &ex.facts {
            for name in server_names(fact) {
                owners.entry(name).or_insert_with(|| service.clone());
            }
        }
    }
    let by_normalised: HashMap<String, &str> = services
        .iter()
        .map(|s| (normalise(&s.name), s.name.as_str()))
        .filter(|(n, _)| !n.is_empty())
        .collect();
    let mut names: Vec<String> = config.protos.iter().map(|p| p.name.clone()).collect();
    for (_, _, ex) in extractions {
        for fact in &ex.facts {
            if let Fact::Call { callee, .. } = fact {
                names.extend(client_names(callee));
            }
        }
    }
    for name in names {
        if owners.contains_key(&name) {
            continue;
        }
        if let Some(owner) = by_normalised.get(&normalise(&name)) {
            owners.insert(name, (*owner).to_string());
        }
    }
    owners
}

impl Matcher for Grpc {
    fn name(&self) -> &'static str {
        "grpc"
    }

    fn candidates(&self, ctx: &FileContext<'_>) -> Vec<Candidate> {
        let mut out = Vec::new();
        for fact in ctx.facts {
            let Fact::Call { callee, line, .. } = fact else {
                continue;
            };
            for name in client_names(callee) {
                out.push(Candidate {
                    target: Target::ProtoService(name),
                    kind_hint: Some(EdgeType::Grpc),
                    evidence: Evidence {
                        file: ctx.file.to_string(),
                        line: Some(*line),
                        detail: Some(callee.clone()),
                    },
                });
            }
        }
        out
    }
}
