//! The data every stage writes. Field names are the JSON contract.
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

/// How sure we are that an edge exists. The weakest confidence on a path
/// decides the confidence of everything reached through it.
///
/// - observed: seen in production traces
/// - static: a code path exists, never observed
/// - inferred: joined through an indirection we cannot resolve exactly
///   (a topic, a shared table)
/// - uncertain: a computed URL or a fuzzy name match; included on purpose,
///   because recall beats precision here
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Observed,
    Static,
    Inferred,
    Uncertain,
}

impl Confidence {
    /// Higher is more certain.
    pub fn rank(self) -> u8 {
        match self {
            Self::Observed => 3,
            Self::Static => 2,
            Self::Inferred => 1,
            Self::Uncertain => 0,
        }
    }
}

/// Where a fact came from. Every service and every edge carries one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Evidence {
    /// Path relative to the repository root, POSIX separators.
    pub file: String,
    /// 1-based line, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Short note: the matched key, the image, the URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// The strategies stage 1 tries, in this order, stopping at the first that
/// yields more than one service with code in the repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiscoveryStrategy {
    DockerCompose,
    Kubernetes,
    Monorepo,
    Workspace,
}

impl DiscoveryStrategy {
    pub const ALL: [Self; 4] = [
        Self::DockerCompose,
        Self::Kubernetes,
        Self::Monorepo,
        Self::Workspace,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::DockerCompose => "docker-compose",
            Self::Kubernetes => "kubernetes",
            Self::Monorepo => "monorepo",
            Self::Workspace => "workspace",
        }
    }
}

/// What told us a service exists. `Root` is the honest fallback: the
/// repository itself is one service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServiceSource {
    Strategy(DiscoveryStrategy),
    Root,
}

impl ServiceSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strategy(s) => s.as_str(),
            Self::Root => "root",
        }
    }
}

impl Serialize for ServiceSource {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ServiceSource {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        if text == "root" {
            return Ok(Self::Root);
        }
        DiscoveryStrategy::ALL
            .into_iter()
            .find(|s| s.as_str() == text)
            .map(Self::Strategy)
            .ok_or_else(|| D::Error::custom(format!("unknown service source {text:?}")))
    }
}

/// A service with code in this repository, or infrastructure the
/// repository declares but does not build (a database image, a broker).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceRole {
    Code,
    Infrastructure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Service {
    pub name: String,
    /// Directory relative to the repository root. None for image-only services.
    pub root: Option<String>,
    pub language: Option<String>,
    /// Files that start the service, relative to its root. Best effort.
    pub entry_points: Vec<String>,
    pub role: ServiceRole,
    pub discovered_by: ServiceSource,
    /// The declaration that told us this service exists.
    pub evidence: Evidence,
    /// Container image, when the declaration names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Package name from its manifest, when different from the directory name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeType {
    Http,
    Event,
    Grpc,
    Database,
    Import,
}

impl EdgeType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Event => "event",
            Self::Grpc => "grpc",
            Self::Database => "database",
            Self::Import => "import",
        }
    }
}

/// `source` depends on `target`: source calls, publishes to, imports from,
/// or shares a database with target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub source: String,
    pub target: String,
    #[serde(rename = "type")]
    pub edge_type: EdgeType,
    pub confidence: Confidence,
    pub evidence: Vec<Evidence>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn service_serialises_camel_case_and_omits_absent_fields() {
        let s = Service {
            name: "redis".into(),
            root: None,
            language: None,
            entry_points: vec![],
            role: ServiceRole::Infrastructure,
            discovered_by: ServiceSource::Strategy(DiscoveryStrategy::DockerCompose),
            evidence: Evidence {
                file: "docker-compose.yml".into(),
                line: Some(9),
                detail: None,
            },
            image: Some("redis:7-alpine".into()),
            package_name: None,
        };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "name": "redis", "root": null, "language": null, "entryPoints": [],
                "role": "infrastructure", "discoveredBy": "docker-compose",
                "evidence": { "file": "docker-compose.yml", "line": 9 },
                "image": "redis:7-alpine"
            })
        );
    }

    #[test]
    fn root_source_serialises_as_root_and_confidence_ranks() {
        assert_eq!(
            serde_json::to_value(ServiceSource::Root).unwrap(),
            serde_json::json!("root")
        );
        let back: ServiceSource = serde_json::from_value(serde_json::json!("root")).unwrap();
        assert_eq!(back, ServiceSource::Root);
        let back: ServiceSource = serde_json::from_value(serde_json::json!("kubernetes")).unwrap();
        assert_eq!(back, ServiceSource::Strategy(DiscoveryStrategy::Kubernetes));
        assert!(Confidence::Observed.rank() > Confidence::Static.rank());
        assert!(Confidence::Static.rank() > Confidence::Inferred.rank());
        assert!(Confidence::Inferred.rank() > Confidence::Uncertain.rank());
        assert_eq!(
            serde_json::to_value(EdgeType::Http).unwrap(),
            serde_json::json!("http")
        );
    }
}
