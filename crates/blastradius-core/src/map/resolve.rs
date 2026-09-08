//! From a candidate to a discovered service, with a confidence that says
//! how the match was made. Recall over precision: anything that matches a
//! discovered service is kept; anything that matches nothing is reported as
//! unresolved, never guessed.
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use super::config::ConfigIndex;
use super::facts::{Part, env_default};
use super::{Candidate, Target, TopicRole};
use crate::discover::directories::{image_basename, normalise};
use crate::model::{Confidence, EdgeType, Service, ServiceRole};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub target: String,
    pub edge_type: EdgeType,
    pub confidence: Confidence,
    pub detail: String,
    /// When the edge starts somewhere other than the file's own service: a
    /// consumer's mention yields producer → consumer.
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unresolved {
    /// The candidate points back at its own service. Dropped, not counted.
    SelfEdge,
    /// Too weak to report even as unresolved: a plain word that happens to
    /// be a code service's name. Dropped, not counted.
    Ignored,
    /// Nothing discovered matches. Counted and listed.
    Unknown(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceKind {
    Datastore,
    Broker,
    Other,
}

const DATASTORES: &[&str] = &[
    "redis",
    "valkey",
    "memcached",
    "mongo",
    "mysql",
    "mariadb",
    "postgres",
    "pgsql",
    "cockroach",
    "cassandra",
    "scylla",
    "elasticsearch",
    "opensearch",
    "influxdb",
    "clickhouse",
    "dynamodb",
    "sqlserver",
    "mssql",
    "oracle",
    "neo4j",
    "couchbase",
    "couchdb",
    "etcd",
    "minio",
    "timescale",
];
const BROKERS: &[&str] = &[
    "rabbitmq",
    "kafka",
    "redpanda",
    "nats",
    "activemq",
    "artemis",
    "pulsar",
    "mosquitto",
    "emqx",
    "localstack",
    "sqs",
    "sns",
    "eventbus",
    "zookeeper",
    "servicebus",
];
const EVENT_SCHEMES: &[&str] = &[
    "amqp", "amqps", "kafka", "nats", "mqtt", "mqtts", "stomp", "sqs", "pulsar",
];
pub(crate) const DATABASE_SCHEMES: &[&str] = &[
    "mongodb",
    "mongodb+srv",
    "postgres",
    "postgresql",
    "mysql",
    "mariadb",
    "redis",
    "rediss",
    "memcached",
    "jdbc",
    "cassandra",
    "sqlserver",
    "mssql",
    "cockroachdb",
    "clickhouse",
    "neo4j",
    "bolt",
    "couchbase",
    "influxdb",
];

static URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([A-Za-z][A-Za-z0-9+.:-]*)://(?:[^/@\s]*@)?(\[[^\]]+\]|[^/:?#\s]+)(?::(\d+))?")
        .unwrap()
});
static HOST_PORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Za-z0-9][A-Za-z0-9.-]*):(\d{2,5})$").unwrap());
static HOSTNAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9.-]*$").unwrap());

const HOSTISH_SUFFIXES: &[&str] = &[
    "_CONNECTION_STRING",
    "_CONNECTIONSTRING",
    "_BOOTSTRAP_SERVERS",
    "_CONNECTION",
    "_BROKERS",
    "_DSN",
    "_SERVICE_HOST",
    "_SERVICE_ADDR",
    "_SERVICE_URL",
    "_BASE_URL",
    "_API_URL",
    "_HOST_NAME",
    "_HOSTNAME",
    "_HOST",
    "_URL",
    "_URI",
    "_ADDR",
    "_ADDRESS",
    "_ENDPOINT",
    "_SERVER",
    "_API",
    "_SERVICE",
    "_PORT",
    "_SVC",
];

/// `scheme://host`, also `jdbc:mysql://host`. Returns (scheme, host), both
/// lowercased, host without brackets.
pub fn parse_url(text: &str) -> Option<(String, String)> {
    let caps = URL.captures(text.trim())?;
    let scheme = caps[1].to_lowercase();
    let host = caps[2].trim_matches(['[', ']']).to_lowercase();
    if host.is_empty() {
        return None;
    }
    Some((scheme, host))
}

/// The host of a bare `host:port`.
pub fn host_port(text: &str) -> Option<String> {
    HOST_PORT.captures(text.trim()).map(|c| c[1].to_lowercase())
}

pub fn is_hostname(text: &str) -> bool {
    HOSTNAME.is_match(text) && text.contains(|c: char| c.is_ascii_alphabetic())
}

/// Does a variable name look like it holds a host or URL?
pub fn is_hostish_var(name: &str) -> bool {
    let upper = name.to_uppercase();
    HOSTISH_SUFFIXES.iter().any(|s| upper.ends_with(s))
}

/// Keys of settings that hold a host: the last dotted segment is a hostish
/// word, or the whole key is a hostish variable name.
const HOSTISH_KEYS: &[&str] = &[
    "host",
    "hostname",
    "hosts",
    "url",
    "uri",
    "addr",
    "address",
    "endpoint",
    "server",
    "servers",
    "brokers",
    "bootstrap-servers",
    "bootstrap_servers",
    "bootstrapservers",
    "nodes",
    "seeds",
    "contact-points",
    "contactpoints",
    "connection-string",
    "connectionstring",
    "connection_string",
    "dsn",
];

pub fn is_hostish_key(key: &str) -> bool {
    let key = key.trim();
    let last = key.rsplit('.').next().unwrap_or(key);
    let last = last.split('[').next().unwrap_or(last).to_lowercase();
    HOSTISH_KEYS.contains(&last.as_str()) || is_hostish_var(key)
}

static ADO_HOST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|;)\s*(?:server|host|data source|addr|address)\s*=\s*([^;,:\s]+)").unwrap()
});
static ADO_DATABASE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:^|;)\s*(?:database|initial catalog)\s*=\s*([^;\s]+)").unwrap()
});

/// ADO.NET style `Host=x;Database=y;...`: (host, database), lowercased, the
/// port after `,` or `:` removed. None unless both parts are present.
pub fn ado_connection(text: &str) -> Option<(String, String)> {
    let host = ADO_HOST.captures(text)?[1].trim().to_lowercase();
    let database = ADO_DATABASE.captures(text)?[1].trim().to_lowercase();
    if host.is_empty() || database.is_empty() || text.contains("://") {
        return None;
    }
    Some((host, database))
}

/// `PRODUCT_CATALOG_SERVICE_ADDR` -> `productcatalog`, `CATALOGUE_HOST` ->
/// `catalogue`: strip hostish suffixes, then the service-name normalisation.
pub fn normalise_var(name: &str) -> String {
    let mut upper = name.to_uppercase();
    for _ in 0..3 {
        let before = upper.len();
        for suffix in HOSTISH_SUFFIXES {
            if let Some(stripped) = upper.strip_suffix(suffix) {
                if !stripped.is_empty() {
                    upper = stripped.to_string();
                    break;
                }
            }
        }
        if upper.len() == before {
            break;
        }
    }
    normalise(&upper)
}

pub(crate) fn is_local(host: &str) -> bool {
    host == "localhost"
        || host == "0.0.0.0"
        || host == "::1"
        || host.starts_with("127.")
        || host.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// Drops self-edges; applies the matcher's type hint when it gave one.
fn finish(
    source: &str,
    target: &str,
    edge_type: EdgeType,
    confidence: Confidence,
    detail: String,
    hint: Option<EdgeType>,
) -> Result<Resolved, Unresolved> {
    if target == source {
        return Err(Unresolved::SelfEdge);
    }
    Ok(Resolved {
        target: target.to_string(),
        edge_type: hint.unwrap_or(edge_type),
        confidence,
        detail,
        source: None,
    })
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TopicSides {
    pub producers: BTreeSet<String>,
    pub consumers: BTreeSet<String>,
    pub unknown: BTreeSet<String>,
}

/// What the pre-pass over every file's facts learned about the repository as
/// a whole: who registers which proto service, who produces and consumes
/// which topic, who uses which database key.
#[derive(Debug, Default)]
pub struct Joins {
    pub proto_owner: HashMap<String, String>,
    pub topics: HashMap<String, TopicSides>,
    pub databases: HashMap<String, BTreeSet<String>>,
}

pub struct Resolver<'a> {
    services: &'a [Service],
    config: &'a ConfigIndex,
    by_name: HashMap<String, usize>,
    by_lower: HashMap<String, usize>,
    by_normalised: HashMap<String, usize>,
    joins: Joins,
    proto_owners: HashSet<String>,
}

impl<'a> Resolver<'a> {
    pub fn new(services: &'a [Service], config: &'a ConfigIndex, joins: Joins) -> Self {
        let mut by_name = HashMap::new();
        let mut by_lower = HashMap::new();
        let mut by_normalised = HashMap::new();
        for (i, s) in services.iter().enumerate() {
            by_name.entry(s.name.clone()).or_insert(i);
            by_lower.entry(s.name.to_lowercase()).or_insert(i);
            let n = normalise(&s.name);
            if !n.is_empty() {
                by_normalised.entry(n).or_insert(i);
            }
            if let Some(image) = image_basename(s.image.as_deref()) {
                let n = normalise(&image);
                if !n.is_empty() {
                    by_normalised.entry(n).or_insert(i);
                }
            }
        }
        let proto_owners = joins.proto_owner.values().cloned().collect();
        Self {
            services,
            config,
            by_name,
            by_lower,
            by_normalised,
            joins,
            proto_owners,
        }
    }

    /// Exact name, then the first DNS label, then the normalised name.
    pub fn service_for_host(&self, host: &str) -> Option<&str> {
        let host = host.trim().trim_end_matches('.').to_lowercase();
        if host.is_empty() || is_local(&host) {
            return None;
        }
        let name = |i: &usize| self.services[*i].name.as_str();
        if let Some(i) = self.by_name.get(&host).or_else(|| self.by_lower.get(&host)) {
            return Some(name(i));
        }
        let label = host.split('.').next().unwrap_or(&host);
        if let Some(i) = self.by_lower.get(label) {
            return Some(name(i));
        }
        let n = normalise(label);
        if n.len() < 3 {
            return None;
        }
        self.by_normalised.get(&n).map(name)
    }

    fn service_for_normalised(&self, normalised: &str) -> Option<&str> {
        if normalised.len() < 3 {
            return None;
        }
        self.by_normalised
            .get(normalised)
            .map(|i| self.services[*i].name.as_str())
    }

    fn kind_of(&self, service: &str) -> ServiceKind {
        let Some(&i) = self.by_name.get(service) else {
            return ServiceKind::Other;
        };
        let s = &self.services[i];
        let mut haystack = s.name.to_lowercase();
        if let Some(image) = image_basename(s.image.as_deref()) {
            haystack.push(' ');
            haystack.push_str(&image.to_lowercase());
        }
        if DATASTORES.iter().any(|k| haystack.contains(k)) {
            return ServiceKind::Datastore;
        }
        if BROKERS.iter().any(|k| haystack.contains(k)) {
            return ServiceKind::Broker;
        }
        ServiceKind::Other
    }

    /// Scheme first when it is specific, then what the target is, then the
    /// `gRPC` rule for scheme-less `host:port`, then http.
    pub fn classify(
        &self,
        target: &str,
        scheme: Option<&str>,
        scheme_less_host_port: bool,
    ) -> EdgeType {
        if let Some(scheme) = scheme {
            let first = scheme.split(':').next().unwrap_or(scheme);
            if EVENT_SCHEMES.contains(&first) {
                return EdgeType::Event;
            }
            if DATABASE_SCHEMES.contains(&first) || DATABASE_SCHEMES.contains(&scheme) {
                return EdgeType::Database;
            }
        }
        match self.kind_of(target) {
            ServiceKind::Datastore => EdgeType::Database,
            ServiceKind::Broker => EdgeType::Event,
            ServiceKind::Other => {
                if scheme_less_host_port && self.proto_owners.contains(target) {
                    EdgeType::Grpc
                } else {
                    EdgeType::Http
                }
            }
        }
    }

    /// A configured or literal value: URL, `host:port`, or a bare hostname.
    fn place_value(&self, value: &str) -> Option<(&str, EdgeType)> {
        let value = value.trim().trim_matches(['"', '\'']);
        if let Some((scheme, host)) = parse_url(value) {
            let target = self.service_for_host(&host)?;
            return Some((target, self.classify(target, Some(&scheme), false)));
        }
        if let Some(host) = host_port(value) {
            let target = self.service_for_host(&host)?;
            return Some((target, self.classify(target, None, true)));
        }
        if is_hostname(value) {
            let target = self.service_for_host(value)?;
            return Some((target, self.classify(target, None, false)));
        }
        None
    }

    pub fn resolve(&self, source: &str, candidate: &Candidate) -> Result<Resolved, Unresolved> {
        let hint = candidate.kind_hint;
        match &candidate.target {
            Target::Url(text) => {
                let (scheme, host) =
                    parse_url(text).ok_or_else(|| Unresolved::Unknown(text.clone()))?;
                let target = self
                    .service_for_host(&host)
                    .ok_or_else(|| Unresolved::Unknown(host.clone()))?;
                let ty = self.classify(target, Some(&scheme), false);
                finish(source, target, ty, Confidence::Static, text.clone(), hint)
            }
            Target::HostPort(text) => {
                let host = host_port(text).ok_or_else(|| Unresolved::Unknown(text.clone()))?;
                let target = self
                    .service_for_host(&host)
                    .ok_or_else(|| Unresolved::Unknown(host.clone()))?;
                let ty = self.classify(target, None, true);
                let detail = candidate
                    .evidence
                    .detail
                    .clone()
                    .unwrap_or_else(|| text.clone());
                finish(source, target, ty, Confidence::Static, detail, hint)
            }
            Target::Host(host) => {
                let target = self
                    .service_for_host(host)
                    .ok_or_else(|| Unresolved::Unknown(host.clone()))?;
                let ty = self.classify(target, None, false);
                let detail = candidate
                    .evidence
                    .detail
                    .clone()
                    .unwrap_or_else(|| host.clone());
                finish(source, target, ty, Confidence::Static, detail, hint)
            }
            Target::BareName(host) => {
                let target = self
                    .service_for_host(host)
                    .ok_or_else(|| Unresolved::Unknown(host.clone()))?;
                // A plain word naming a code service is not evidence of a call.
                if self.kind_of(target) == ServiceKind::Other {
                    return Err(Unresolved::Ignored);
                }
                let ty = self.classify(target, None, false);
                finish(
                    source,
                    target,
                    ty,
                    Confidence::Uncertain,
                    format!("\"{host}\""),
                    hint,
                )
            }
            Target::EnvVar { name, default } => {
                self.resolve_env(source, name, default.as_deref(), hint)
            }
            Target::Template(parts) => self.resolve_template(source, parts, hint),
            Target::ProtoService(name) => {
                let owner = self
                    .joins
                    .proto_owner
                    .get(name)
                    .ok_or_else(|| Unresolved::Unknown(name.clone()))?;
                finish(
                    source,
                    owner,
                    EdgeType::Grpc,
                    Confidence::Static,
                    format!("proto service {name}"),
                    hint,
                )
            }
            Target::Topic { .. } | Target::Broker { .. } | Target::Database(_) => self
                .resolve_all(source, candidate)
                .into_iter()
                .next()
                .unwrap_or(Err(Unresolved::Ignored)),
            Target::Package { name, how } => self.resolve_package(source, name, how, hint),
            Target::PackagePath { path, how } => {
                let owner = self.service_for_path(path).ok_or(Unresolved::Ignored)?;
                finish(
                    source,
                    owner,
                    EdgeType::Import,
                    Confidence::Static,
                    how.clone(),
                    hint,
                )
            }
        }
    }

    /// Every edge a candidate yields. Hosts, variables and packages give at
    /// most one; topics, brokers and shared databases fan out to every
    /// counterpart.
    pub fn resolve_all(
        &self,
        source: &str,
        candidate: &Candidate,
    ) -> Vec<Result<Resolved, Unresolved>> {
        match &candidate.target {
            Target::Topic { key, role } => self.resolve_topic(source, key, *role),
            Target::Broker { family, how } => self.resolve_broker(source, family, how),
            Target::Database(key) => self.resolve_database(source, key),
            _ => vec![self.resolve(source, candidate)],
        }
    }

    fn resolve_topic(
        &self,
        source: &str,
        key: &str,
        role: TopicRole,
    ) -> Vec<Result<Resolved, Unresolved>> {
        let Some(sides) = self.joins.topics.get(key) else {
            return vec![Err(Unresolved::Ignored)];
        };
        let inferred = |target: &str, from: Option<&str>, detail: String| Resolved {
            target: target.to_string(),
            edge_type: EdgeType::Event,
            confidence: Confidence::Inferred,
            detail,
            source: from.map(str::to_string),
        };
        let mut out = Vec::new();
        let others = |set: &BTreeSet<String>| -> Vec<String> {
            set.iter()
                .filter(|s| s.as_str() != source)
                .cloned()
                .collect()
        };
        match role {
            TopicRole::Producer => {
                for c in others(&sides.consumers) {
                    out.push(Ok(inferred(
                        &c,
                        None,
                        format!("publishes \"{key}\", consumed by {c}"),
                    )));
                }
                for u in others(&sides.unknown) {
                    out.push(Ok(inferred(
                        &u,
                        None,
                        format!("publishes \"{key}\", mentioned by {u}"),
                    )));
                }
            }
            TopicRole::Consumer => {
                for p in others(&sides.producers) {
                    out.push(Ok(inferred(
                        source,
                        Some(&p),
                        format!("consumes \"{key}\", published by {p}"),
                    )));
                }
                for u in others(&sides.unknown) {
                    out.push(Ok(inferred(
                        source,
                        Some(&u),
                        format!("consumes \"{key}\", mentioned by {u}"),
                    )));
                }
            }
            TopicRole::Unknown => {
                for p in others(&sides.producers) {
                    out.push(Ok(inferred(
                        source,
                        Some(&p),
                        format!("mentions \"{key}\", published by {p}"),
                    )));
                }
                for c in others(&sides.consumers) {
                    out.push(Ok(inferred(
                        &c,
                        None,
                        format!("mentions \"{key}\", consumed by {c}"),
                    )));
                }
            }
        }
        if out.is_empty() {
            return vec![if role == TopicRole::Unknown {
                Err(Unresolved::Ignored)
            } else {
                Err(Unresolved::Unknown(format!("topic:{key}")))
            }];
        }
        out
    }

    /// A broker family is named by an infrastructure service's name or
    /// image, or by a code service's image (a code service that builds a
    /// broker image is still the broker; one merely *named* like one, such
    /// as a `kafka-consumer` that imports the client library, is not).
    fn resolve_broker(
        &self,
        source: &str,
        family: &str,
        how: &str,
    ) -> Vec<Result<Resolved, Unresolved>> {
        let aliases: Vec<&str> = match family {
            "rabbitmq" => vec!["rabbitmq", "amqp"],
            "kafka" => vec!["kafka", "redpanda"],
            "mqtt" => vec!["mqtt", "mosquitto", "emqx", "hivemq"],
            "sqs" | "sns" => vec![family, "localstack"],
            "activemq" => vec!["activemq", "artemis"],
            other => vec![other],
        };
        let mut out = Vec::new();
        for s in self.services {
            let is_broker = match s.role {
                ServiceRole::Infrastructure => {
                    let mut haystack = s.name.to_lowercase();
                    if let Some(image) = image_basename(s.image.as_deref()) {
                        haystack.push(' ');
                        haystack.push_str(&image.to_lowercase());
                    }
                    aliases.iter().any(|a| haystack.contains(a))
                }
                ServiceRole::Code => image_basename(s.image.as_deref()).is_some_and(|image| {
                    let image = image.to_lowercase();
                    aliases.iter().any(|a| image.contains(a))
                }),
            };
            if is_broker {
                out.push(finish(
                    source,
                    &s.name,
                    EdgeType::Event,
                    Confidence::Inferred,
                    how.to_string(),
                    None,
                ));
            }
        }
        if out.is_empty() {
            return vec![Err(Unresolved::Ignored)];
        }
        out
    }

    fn resolve_database(&self, source: &str, key: &str) -> Vec<Result<Resolved, Unresolved>> {
        let out: Vec<Result<Resolved, Unresolved>> = self
            .joins
            .databases
            .get(key)
            .into_iter()
            .flatten()
            .filter(|s| s.as_str() != source)
            .map(|other| {
                Ok(Resolved {
                    target: other.clone(),
                    edge_type: EdgeType::Database,
                    confidence: Confidence::Inferred,
                    detail: format!("shared database {key} with {other}"),
                    source: None,
                })
            })
            .collect();
        if out.is_empty() {
            return vec![Err(Unresolved::Ignored)];
        }
        out
    }

    /// Exact package name first, then the longest declared name that is a
    /// prefix ending at `/`, `.` or `:` (`@acme/shared/utils`,
    /// `github.com/acme/demo/cart/genproto`, `Basket.API.Grpc`).
    fn resolve_package(
        &self,
        source: &str,
        name: &str,
        how: &str,
        hint: Option<EdgeType>,
    ) -> Result<Resolved, Unresolved> {
        let packages = &self.config.packages;
        let owner = packages
            .iter()
            .find(|p| p.name == name)
            .or_else(|| packages.iter().find(|p| p.name.eq_ignore_ascii_case(name)))
            .or_else(|| {
                packages
                    .iter()
                    .filter(|p| {
                        name.len() > p.name.len()
                            && name.starts_with(p.name.as_str())
                            && name[p.name.len()..].starts_with(['/', '.', ':'])
                    })
                    .max_by_key(|p| p.name.len())
            })
            .map(|p| p.service.as_str())
            .ok_or(Unresolved::Ignored)?;
        finish(
            source,
            owner,
            EdgeType::Import,
            Confidence::Static,
            how.to_string(),
            hint,
        )
    }

    pub fn service_for_path(&self, path: &str) -> Option<&str> {
        crate::map::owner_of_path(self.services, path)
    }

    fn resolve_env(
        &self,
        source: &str,
        name: &str,
        default: Option<&str>,
        hint: Option<EdgeType>,
    ) -> Result<Resolved, Unresolved> {
        if let Some(configured) = self.config.lookup(source, name) {
            if let Some((target, ty)) = self.place_value(&configured.value) {
                let at = match configured.evidence.line {
                    Some(line) => format!("{}:{line}", configured.evidence.file),
                    None => configured.evidence.file.clone(),
                };
                let detail = format!("{name}={} via {at}", configured.value);
                return finish(source, target, ty, Confidence::Static, detail, hint);
            }
        }
        if let Some(default) = default {
            if let Some((target, ty)) = self.place_value(default) {
                return finish(
                    source,
                    target,
                    ty,
                    Confidence::Static,
                    format!("{name} default {default}"),
                    hint,
                );
            }
        }
        if let Some(target) = self.service_for_normalised(&normalise_var(name)) {
            let ty = self.classify(target, None, false);
            return finish(
                source,
                target,
                ty,
                Confidence::Uncertain,
                format!("{name} matched by name"),
                hint,
            );
        }
        Err(Unresolved::Unknown(name.to_string()))
    }

    fn resolve_template(
        &self,
        source: &str,
        parts: &[Part],
        hint: Option<EdgeType>,
    ) -> Result<Resolved, Unresolved> {
        let rendered: String = parts
            .iter()
            .map(|p| match p {
                Part::Lit(s) => s.clone(),
                Part::Var(v) => format!("${{{v}}}"),
            })
            .collect();
        let mut first_unknown: Option<String> = None;
        let mut self_edge = false;
        let count = parts.len();
        for (i, part) in parts.iter().enumerate() {
            let attempt = match part {
                Part::Var(var) => {
                    let (name, default) = env_default(var);
                    let mut result = self.resolve_env(source, &name, default.as_deref(), hint);
                    if let Ok(r) = &mut result {
                        if r.confidence == Confidence::Static
                            && !r.detail.contains(" via ")
                            && !r.detail.contains(" default ")
                        {
                            r.confidence = Confidence::Uncertain;
                        }
                        r.detail = format!("{rendered} ({})", r.detail);
                    }
                    result
                }
                Part::Lit(lit) => {
                    let Some((scheme, host)) = parse_url(lit) else {
                        continue;
                    };
                    // The host is only complete when something follows it
                    // in the same literal, or the literal is the last part.
                    let after_host = lit.split_once("://").map_or("", |(_, rest)| rest);
                    let complete = after_host.len() > host.len() || i + 1 == count;
                    if !complete {
                        continue;
                    }
                    match self.service_for_host(&host) {
                        Some(target) => {
                            let ty = self.classify(target, Some(&scheme), false);
                            finish(
                                source,
                                target,
                                ty,
                                Confidence::Static,
                                rendered.clone(),
                                hint,
                            )
                        }
                        None => Err(Unresolved::Unknown(host)),
                    }
                }
            };
            match attempt {
                Ok(resolved) => return Ok(resolved),
                Err(Unresolved::SelfEdge) => self_edge = true,
                Err(Unresolved::Ignored) => {}
                Err(Unresolved::Unknown(name)) => {
                    if first_unknown.is_none() {
                        first_unknown = Some(name);
                    }
                }
            }
        }
        if self_edge {
            return Err(Unresolved::SelfEdge);
        }
        Err(Unresolved::Unknown(first_unknown.unwrap_or(rendered)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Target, TopicRole, owner_of_path};
    use crate::model::{DiscoveryStrategy, Evidence, ServiceRole, ServiceSource};
    use pretty_assertions::assert_eq;
    use std::collections::BTreeSet;

    fn svc(name: &str, image: Option<&str>) -> Service {
        Service {
            name: name.into(),
            root: image.is_none().then(|| name.to_string()),
            language: None,
            entry_points: vec![],
            role: if image.is_some() {
                ServiceRole::Infrastructure
            } else {
                ServiceRole::Code
            },
            discovered_by: ServiceSource::Strategy(DiscoveryStrategy::DockerCompose),
            evidence: Evidence {
                file: "docker-compose.yml".into(),
                line: None,
                detail: None,
            },
            image: image.map(str::to_string),
            package_name: None,
        }
    }
    fn services(code: &[&str], infra: &[(&str, &str)]) -> Vec<Service> {
        code.iter()
            .map(|n| svc(n, None))
            .chain(infra.iter().map(|(n, i)| svc(n, Some(i))))
            .collect()
    }
    fn ev() -> Evidence {
        Evidence {
            file: "x".into(),
            line: Some(1),
            detail: None,
        }
    }
    fn cand(target: Target) -> Candidate {
        Candidate {
            target,
            kind_hint: None,
            evidence: ev(),
        }
    }

    #[test]
    fn hosts_resolve_exact_label_and_normalised() {
        let s = services(
            &["cart", "productcatalogservice", "Basket.API"],
            &[("redis", "redis:7")],
        );
        let cfg = ConfigIndex::default();
        let r = Resolver::new(&s, &cfg, Joins::default());
        assert_eq!(r.service_for_host("cart"), Some("cart"));
        assert_eq!(
            r.service_for_host("cart.default.svc.cluster.local"),
            Some("cart")
        );
        assert_eq!(
            r.service_for_host("product-catalog-service"),
            Some("productcatalogservice")
        );
        assert_eq!(r.service_for_host("basket-api"), Some("Basket.API"));
        assert_eq!(r.service_for_host("localhost"), None);
        assert_eq!(r.service_for_host("127.0.0.1"), None);
        assert_eq!(r.service_for_host("paypal.com"), None);
        assert_eq!(r.service_for_host("redis"), Some("redis"));
    }

    #[test]
    fn urls_type_by_scheme_then_target() {
        let s = services(
            &["catalogue", "payment", "mysql"],
            &[("redis", "redis:7"), ("rabbitmq", "rabbitmq:3")],
        );
        let cfg = ConfigIndex::default();
        let r = Resolver::new(&s, &cfg, Joins::default());
        let ok = |t: Target| r.resolve("web", &cand(t)).unwrap();
        let c = ok(Target::Url("http://catalogue:8080/products".into()));
        assert_eq!(
            (c.target.as_str(), c.edge_type, c.confidence),
            ("catalogue", EdgeType::Http, Confidence::Static)
        );
        assert_eq!(c.detail, "http://catalogue:8080/products");
        assert_eq!(
            ok(Target::Url("redis://redis:6379/0".into())).edge_type,
            EdgeType::Database
        );
        assert_eq!(
            ok(Target::Url("amqp://guest:guest@rabbitmq:5672".into())).edge_type,
            EdgeType::Event
        );
        assert_eq!(
            ok(Target::Host("redis".into())).edge_type,
            EdgeType::Database
        );
        assert_eq!(
            ok(Target::Url("jdbc:mysql://mysql:3306/cities".into())).edge_type,
            EdgeType::Database
        );
        assert_eq!(
            ok(Target::Url("https+http://catalogue".into())).target,
            "catalogue"
        );
        assert_eq!(
            ok(Target::BareName("rabbitmq".into())).confidence,
            Confidence::Uncertain
        );
        assert_eq!(
            ok(Target::BareName("mysql".into())).edge_type,
            EdgeType::Database,
            "a code service named like a datastore still counts"
        );
        assert_eq!(
            r.resolve("web", &cand(Target::BareName("catalogue".into()))),
            Err(Unresolved::Ignored)
        );
        assert_eq!(
            r.resolve("web", &cand(Target::Url("http://example.com/x".into()))),
            Err(Unresolved::Unknown("example.com".into()))
        );
        assert_eq!(
            r.resolve(
                "catalogue",
                &cand(Target::Url("http://catalogue:8080".into()))
            ),
            Err(Unresolved::SelfEdge)
        );
    }

    #[test]
    fn env_vars_resolve_through_config_default_or_name() {
        let mut cfg = ConfigIndex::default();
        cfg.insert_for_test(
            "web",
            "CATALOGUE_HOST",
            "catalogue",
            "docker-compose.yml",
            5,
        );
        let s = services(&["catalogue", "user", "cart"], &[]);
        let r = Resolver::new(&s, &cfg, Joins::default());
        let c = r
            .resolve(
                "web",
                &cand(Target::EnvVar {
                    name: "CATALOGUE_HOST".into(),
                    default: None,
                }),
            )
            .unwrap();
        assert_eq!(
            (c.target.as_str(), c.confidence),
            ("catalogue", Confidence::Static)
        );
        assert_eq!(
            c.detail,
            "CATALOGUE_HOST=catalogue via docker-compose.yml:5"
        );
        let c = r
            .resolve(
                "web",
                &cand(Target::EnvVar {
                    name: "USER_HOST".into(),
                    default: Some("user".into()),
                }),
            )
            .unwrap();
        assert_eq!(
            (c.target.as_str(), c.confidence, c.detail.as_str()),
            ("user", Confidence::Static, "USER_HOST default user")
        );
        let c = r
            .resolve(
                "web",
                &cand(Target::EnvVar {
                    name: "CART_SERVICE_ADDR".into(),
                    default: None,
                }),
            )
            .unwrap();
        assert_eq!(
            (c.target.as_str(), c.confidence, c.detail.as_str()),
            (
                "cart",
                Confidence::Uncertain,
                "CART_SERVICE_ADDR matched by name"
            )
        );
        assert_eq!(
            r.resolve(
                "web",
                &cand(Target::EnvVar {
                    name: "AMQP_HOST".into(),
                    default: None
                })
            ),
            Err(Unresolved::Unknown("AMQP_HOST".into()))
        );
        assert_eq!(
            r.resolve(
                "cart",
                &cand(Target::EnvVar {
                    name: "CART_HOST".into(),
                    default: None
                })
            ),
            Err(Unresolved::SelfEdge)
        );
    }

    #[test]
    fn templates_and_protos() {
        let mut owner = HashMap::new();
        owner.insert("CartService".to_string(), "cart".to_string());
        let cfg = ConfigIndex::default();
        let s = services(&["cart", "user", "catalogue"], &[]);
        let r = Resolver::new(
            &s,
            &cfg,
            Joins {
                proto_owner: owner,
                ..Joins::default()
            },
        );
        let c = r
            .resolve(
                "web",
                &cand(Target::Template(vec![
                    Part::Lit("http://".into()),
                    Part::Var("USER_HOST".into()),
                    Part::Lit(":8080/".into()),
                ])),
            )
            .unwrap();
        assert_eq!(
            (c.target.as_str(), c.confidence, c.edge_type),
            ("user", Confidence::Uncertain, EdgeType::Http)
        );
        assert_eq!(
            c.detail,
            "http://${USER_HOST}:8080/ (USER_HOST matched by name)"
        );
        let c = r
            .resolve(
                "web",
                &cand(Target::Template(vec![
                    Part::Lit("http://catalogue:8080/product/".into()),
                    Part::Var("sku".into()),
                ])),
            )
            .unwrap();
        assert_eq!(
            (c.target.as_str(), c.confidence),
            ("catalogue", Confidence::Static)
        );
        assert_eq!(
            r.resolve(
                "web",
                &cand(Target::Template(vec![
                    Part::Lit("http://".into()),
                    Part::Var("host".into())
                ]))
            ),
            Err(Unresolved::Unknown("host".into()))
        );
        let c = r
            .resolve("web", &cand(Target::ProtoService("CartService".into())))
            .unwrap();
        assert_eq!(
            (
                c.target.as_str(),
                c.confidence,
                c.edge_type,
                c.detail.as_str()
            ),
            (
                "cart",
                Confidence::Static,
                EdgeType::Grpc,
                "proto service CartService"
            )
        );
        assert_eq!(
            r.resolve("web", &cand(Target::HostPort("cart:7070".into())))
                .unwrap()
                .edge_type,
            EdgeType::Grpc
        );
        assert_eq!(
            r.resolve("web", &cand(Target::HostPort("user:8080".into())))
                .unwrap()
                .edge_type,
            EdgeType::Http
        );
        assert_eq!(
            normalise_var("PRODUCT_CATALOG_SERVICE_ADDR"),
            "productcatalog"
        );
        assert_eq!(normalise_var("CATALOGUE_HOST"), "catalogue");
        assert_eq!(normalise_var("SPRING_DATASOURCE_URL"), "springdatasource");
        assert!(
            is_hostish_var("CART_HOST")
                && is_hostish_var("cart_service_addr")
                && !is_hostish_var("DEBUG")
        );
        assert_eq!(
            parse_url("jdbc:mysql://db:3306/x"),
            Some(("jdbc:mysql".into(), "db".into()))
        );
        assert_eq!(host_port("payment:50051"), Some("payment".into()));
        assert_eq!(host_port("http://x"), None);
    }

    #[test]
    #[allow(clippy::bool_comparison, clippy::manual_assert_eq)]
    fn hostish_keys_and_connection_strings() {
        assert!(is_hostish_key("spring.data.mongodb.host"));
        assert!(is_hostish_key("bootstrap-servers"));
        assert!(is_hostish_key("host"));
        assert!(is_hostish_key("spring.kafka.bootstrap-servers"));
        assert!(is_hostish_key("DB_CONNECTION_STRING"));
        assert!(is_hostish_key("eureka.client.serviceUrl.defaultZone") == false);
        assert!(!is_hostish_key("spring.data.mongodb.database"));
        assert!(!is_hostish_key("name") && !is_hostish_key("image"));
        assert!(is_hostish_var("DB_CONNECTION_STRING") && is_hostish_var("PDO_DSN"));
        assert_eq!(
            ado_connection("Host=localhost;Database=LedgerDB;Username=postgres;Password=x"),
            Some(("localhost".into(), "ledgerdb".into()))
        );
        assert_eq!(
            ado_connection("Server=sql,1433;Initial Catalog=Shop;User Id=sa"),
            Some(("sql".into(), "shop".into()))
        );
        assert_eq!(
            ado_connection("Data Source=sql:5432;Database=Shop"),
            Some(("sql".into(), "shop".into()))
        );
        assert_eq!(ado_connection("Host=localhost;Username=postgres"), None);
        assert_eq!(ado_connection("mongodb://mongodb/x"), None);
    }

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(ToString::to_string).collect()
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn topics_fan_out_from_either_side() {
        let s = services(&["payment", "dispatch", "audit", "web"], &[]);
        let cfg = ConfigIndex::default();
        let mut joins = Joins::default();
        joins.topics.insert(
            "orders".into(),
            TopicSides {
                producers: set(&["payment"]),
                consumers: set(&["dispatch", "audit"]),
                unknown: BTreeSet::new(),
            },
        );
        joins.topics.insert(
            "refunds".into(),
            TopicSides {
                producers: set(&["payment"]),
                ..TopicSides::default()
            },
        );
        joins.topics.insert(
            "robot-shop".into(),
            TopicSides {
                producers: set(&["payment"]),
                consumers: BTreeSet::new(),
                unknown: set(&["dispatch", "payment"]),
            },
        );
        let r = Resolver::new(&s, &cfg, joins);
        let topic = |key: &str, role: TopicRole| Candidate {
            target: Target::Topic {
                key: key.into(),
                role,
            },
            kind_hint: Some(EdgeType::Event),
            evidence: ev(),
        };
        let out = r.resolve_all("payment", &topic("orders", TopicRole::Producer));
        let got: Vec<(String, Option<String>, EdgeType, Confidence, String)> = out
            .into_iter()
            .map(|o| {
                let o = o.unwrap();
                (o.target, o.source, o.edge_type, o.confidence, o.detail)
            })
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    "audit".into(),
                    None,
                    EdgeType::Event,
                    Confidence::Inferred,
                    "publishes \"orders\", consumed by audit".into()
                ),
                (
                    "dispatch".into(),
                    None,
                    EdgeType::Event,
                    Confidence::Inferred,
                    "publishes \"orders\", consumed by dispatch".into()
                ),
            ]
        );
        let out = r.resolve_all("dispatch", &topic("orders", TopicRole::Consumer));
        assert_eq!(out.len(), 1);
        let o = out[0].clone().unwrap();
        assert_eq!(
            (o.target.as_str(), o.source.as_deref(), o.detail.as_str()),
            (
                "dispatch",
                Some("payment"),
                "consumes \"orders\", published by payment"
            )
        );
        assert_eq!(
            r.resolve_all("payment", &topic("refunds", TopicRole::Producer)),
            vec![Err(Unresolved::Unknown("topic:refunds".into()))]
        );
        let out = r.resolve_all("dispatch", &topic("robot-shop", TopicRole::Unknown));
        assert_eq!(out.len(), 1);
        let o = out[0].clone().unwrap();
        assert_eq!(
            (o.target.as_str(), o.source.as_deref(), o.detail.as_str()),
            (
                "dispatch",
                Some("payment"),
                "mentions \"robot-shop\", published by payment"
            )
        );
        assert_eq!(
            r.resolve_all("payment", &topic("robot-shop", TopicRole::Producer)),
            vec![Ok(Resolved {
                target: "dispatch".into(),
                edge_type: EdgeType::Event,
                confidence: Confidence::Inferred,
                detail: "publishes \"robot-shop\", mentioned by dispatch".into(),
                source: None,
            })],
            "an unknown-role mention counts as a counterpart, never the producer itself"
        );
        assert_eq!(
            r.resolve_all("web", &topic("nothing", TopicRole::Unknown)),
            vec![Err(Unresolved::Ignored)]
        );
    }

    #[test]
    fn brokers_resolve_by_library_family() {
        let mut s = services(
            &["payment"],
            &[("rabbitmq", "rabbitmq:3-management"), ("redis", "redis:7")],
        );
        // A code service merely named like a broker is not the broker...
        s.push(svc("kafka-consumer", None));
        // ...but a code service that builds a broker image is.
        let mut broker_image = svc("broker", Some("confluentinc/cp-kafka:7.6.0"));
        broker_image.role = ServiceRole::Code;
        s.push(broker_image);
        let cfg = ConfigIndex::default();
        let r = Resolver::new(&s, &cfg, Joins::default());
        let broker = |family: &str| Candidate {
            target: Target::Broker {
                family: family.into(),
                how: format!("imports pika ({family} client)"),
            },
            kind_hint: Some(EdgeType::Event),
            evidence: ev(),
        };
        let out = r.resolve_all("payment", &broker("rabbitmq"));
        assert_eq!(out.len(), 1);
        let o = out[0].clone().unwrap();
        assert_eq!(
            (
                o.target.as_str(),
                o.edge_type,
                o.confidence,
                o.detail.as_str()
            ),
            (
                "rabbitmq",
                EdgeType::Event,
                Confidence::Inferred,
                "imports pika (rabbitmq client)"
            )
        );
        let out = r.resolve_all("payment", &broker("kafka"));
        assert_eq!(out.len(), 1, "kafka-consumer is a client, not the broker");
        assert_eq!(
            out[0].clone().unwrap().target,
            "broker",
            "a code service that builds a broker image is still the broker"
        );
        assert_eq!(
            r.resolve_all("rabbitmq", &broker("rabbitmq")),
            vec![Err(Unresolved::SelfEdge)]
        );
    }

    #[test]
    fn shared_databases_join_every_other_user() {
        let s = services(&["orders", "reports", "ledger"], &[]);
        let cfg = ConfigIndex::default();
        let mut joins = Joins::default();
        joins
            .databases
            .insert("mysql/shop".into(), set(&["orders", "reports"]));
        joins.databases.insert("ledgerdb".into(), set(&["ledger"]));
        let r = Resolver::new(&s, &cfg, joins);
        let db = |key: &str| Candidate {
            target: Target::Database(key.into()),
            kind_hint: Some(EdgeType::Database),
            evidence: ev(),
        };
        let out = r.resolve_all("orders", &db("mysql/shop"));
        assert_eq!(
            out,
            vec![Ok(Resolved {
                target: "reports".into(),
                edge_type: EdgeType::Database,
                confidence: Confidence::Inferred,
                detail: "shared database mysql/shop with reports".into(),
                source: None,
            })]
        );
        assert_eq!(
            r.resolve_all("ledger", &db("ledgerdb")),
            vec![Err(Unresolved::Ignored)]
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn packages_resolve_exact_prefix_and_path() {
        let mut s = services(
            &[
                "web",
                "shared",
                "cart",
                "checkout",
                "Basket.API",
                "EventBus",
            ],
            &[],
        );
        for svc in &mut s {
            if svc.name == "Basket.API" || svc.name == "EventBus" {
                svc.root = Some(format!("src/{}", svc.name));
            }
        }
        let mut cfg = ConfigIndex::default();
        let pkg = |name: &str, service: &str| crate::map::config::Package {
            name: name.into(),
            service: service.into(),
            evidence: ev(),
        };
        cfg.packages.push(pkg("@acme/shared", "shared"));
        cfg.packages
            .push(pkg("github.com/acme/demo/services/cart", "cart"));
        let r = Resolver::new(&s, &cfg, Joins::default());
        let package = |name: &str| Candidate {
            target: Target::Package {
                name: name.into(),
                how: format!("import {name}"),
            },
            kind_hint: Some(EdgeType::Import),
            evidence: ev(),
        };
        let o = r.resolve("web", &package("@acme/shared/utils")).unwrap();
        assert_eq!(
            (
                o.target.as_str(),
                o.edge_type,
                o.confidence,
                o.detail.as_str()
            ),
            (
                "shared",
                EdgeType::Import,
                Confidence::Static,
                "import @acme/shared/utils"
            )
        );
        assert_eq!(
            r.resolve(
                "checkout",
                &package("github.com/acme/demo/services/cart/genproto")
            )
            .unwrap()
            .target,
            "cart"
        );
        assert_eq!(
            r.resolve(
                "checkout",
                &package("github.com/acme/demo/services/cartography")
            ),
            Err(Unresolved::Ignored),
            "a prefix must end at a separator"
        );
        assert_eq!(
            r.resolve("web", &package("express")),
            Err(Unresolved::Ignored)
        );
        assert_eq!(
            r.resolve("shared", &package("@acme/shared")),
            Err(Unresolved::SelfEdge)
        );
        let path = |path: &str| Candidate {
            target: Target::PackagePath {
                path: path.into(),
                how: "ProjectReference ..\\EventBus\\EventBus.csproj".into(),
            },
            kind_hint: Some(EdgeType::Import),
            evidence: ev(),
        };
        let o = r
            .resolve("Basket.API", &path("src/EventBus/EventBus.csproj"))
            .unwrap();
        assert_eq!(
            (o.target.as_str(), o.confidence),
            ("EventBus", Confidence::Static)
        );
        assert_eq!(
            r.resolve("Basket.API", &path("src/Nowhere/X.csproj")),
            Err(Unresolved::Ignored)
        );
        assert_eq!(
            owner_of_path(&s, "src/EventBus/EventBus.csproj"),
            Some("EventBus")
        );
        assert_eq!(
            owner_of_path(&s, "src/EventBusRabbitMQ/x.cs"),
            None,
            "a root prefix ends at a slash"
        );
    }
}
