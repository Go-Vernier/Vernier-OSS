//! Runtime service names rarely equal repository service names:
//! `checkout-api` in traces, `services/checkout` in the tree. Each runtime
//! name is resolved once, through the first tier that answers, and the whole
//! table is reported so a partial join is never silent.
use std::collections::BTreeSet;

use crate::config::RuntimeConfig;
use crate::discover::directories::normalise;
use crate::model::Service;

use super::RuntimeError;

pub const FUZZY_THRESHOLD: f64 = 0.9;
const FUZZY_MIN_LEN: usize = 4;

#[derive(Debug, Clone, PartialEq)]
pub enum MatchHow {
    Config,
    Exact,
    Normalised,
    Fuzzy(f64),
    Ignored,
    Unmatched,
}

impl MatchHow {
    pub fn as_str(&self) -> String {
        match self {
            Self::Config => "config".into(),
            Self::Exact => "exact".into(),
            Self::Normalised => "normalised".into(),
            Self::Fuzzy(score) => format!("fuzzy {score:.2}"),
            Self::Ignored => "ignored".into(),
            Self::Unmatched => "unmatched".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mapping {
    pub runtime: String,
    pub service: Option<String>,
    pub how: MatchHow,
}

/// One mapping per runtime name, in sorted order. Errors when the config
/// maps a name to a service that was not discovered.
pub fn match_names(
    runtime: &BTreeSet<String>,
    services: &[Service],
    config: &RuntimeConfig,
) -> Result<Vec<Mapping>, RuntimeError> {
    let normalised: Vec<(String, &Service)> =
        services.iter().map(|s| (normalise(&s.name), s)).collect();
    let mut out = Vec::with_capacity(runtime.len());
    for name in runtime {
        let mapping = |service: Option<&Service>, how: MatchHow| Mapping {
            runtime: name.clone(),
            service: service.map(|s| s.name.clone()),
            how,
        };
        if config.ignore.iter().any(|i| i == name) {
            out.push(mapping(None, MatchHow::Ignored));
            continue;
        }
        if let Some(target) = config.map.get(name) {
            let Some(service) = services.iter().find(|s| &s.name == target) else {
                return Err(RuntimeError::Config(format!(
                    "blast-radius.config.json maps {name} to {target}, which was not discovered"
                )));
            };
            out.push(mapping(Some(service), MatchHow::Config));
            continue;
        }
        if let Some(service) = services.iter().find(|s| &s.name == name) {
            out.push(mapping(Some(service), MatchHow::Exact));
            continue;
        }
        let n = normalise(name);
        if !n.is_empty() {
            if let Some((_, service)) = normalised.iter().find(|(ns, _)| *ns == n) {
                out.push(mapping(Some(service), MatchHow::Normalised));
                continue;
            }
        }
        let best = normalised
            .iter()
            .filter(|(ns, _)| n.len() >= FUZZY_MIN_LEN && ns.len() >= FUZZY_MIN_LEN)
            .map(|(ns, s)| (strsim::jaro_winkler(&n, ns), *s))
            .filter(|(score, _)| *score >= FUZZY_THRESHOLD)
            .max_by(|a, b| a.0.total_cmp(&b.0));
        match best {
            Some((score, service)) => out.push(mapping(Some(service), MatchHow::Fuzzy(score))),
            None => out.push(mapping(None, MatchHow::Unmatched)),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DiscoveryStrategy, Evidence, ServiceRole, ServiceSource};
    use pretty_assertions::assert_eq;

    fn svc(name: &str, infra: bool) -> Service {
        Service {
            name: name.into(),
            root: (!infra).then(|| name.to_string()),
            language: None,
            entry_points: vec![],
            role: if infra {
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
            image: infra.then(|| format!("{name}:latest")),
            package_name: None,
        }
    }

    fn names(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn tiers_in_order_config_exact_normalised_fuzzy() {
        let services = vec![
            svc("checkout", false),
            svc("payment", false),
            svc("catalogue", false),
            svc("orders", false),
            svc("notifications", false),
            svc("redis", true),
        ];
        let mut config = RuntimeConfig::default();
        config.map.insert("pay".into(), "payment".into());
        config.ignore.push("load-generator".into());
        let runtime = names(&[
            "checkout",
            "checkout-api",
            "chckout",
            "pay",
            "load-generator",
            "auth-proxy",
            "catalogue-service",
            "redis",
            "Orders",
        ]);
        let got: Vec<(String, Option<String>, String)> = match_names(&runtime, &services, &config)
            .unwrap()
            .into_iter()
            .map(|m| (m.runtime, m.service, m.how.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("Orders".into(), Some("orders".into()), "normalised".into()),
                ("auth-proxy".into(), None, "unmatched".into()),
                (
                    "catalogue-service".into(),
                    Some("catalogue".into()),
                    "normalised".into()
                ),
                (
                    "chckout".into(),
                    Some("checkout".into()),
                    "fuzzy 0.97".into()
                ),
                ("checkout".into(), Some("checkout".into()), "exact".into()),
                (
                    "checkout-api".into(),
                    Some("checkout".into()),
                    "normalised".into()
                ),
                ("load-generator".into(), None, "ignored".into()),
                ("pay".into(), Some("payment".into()), "config".into()),
                ("redis".into(), Some("redis".into()), "exact".into()),
            ]
        );
    }

    #[test]
    fn fuzzy_needs_four_characters_and_the_threshold() {
        let services = vec![svc("cart", false), svc("payment", false)];
        let config = RuntimeConfig::default();
        let got = match_names(
            &names(&["car", "carts", "paymnt", "billing"]),
            &services,
            &config,
        )
        .unwrap();
        // match_names walks a BTreeSet, so lookup by name rather than input order.
        let how = |name: &str| got.iter().find(|m| m.runtime == name).unwrap().how.as_str();
        assert_eq!(how("car"), "unmatched", "car: too short for fuzzy");
        assert!(
            how("carts").starts_with("fuzzy"),
            "carts -> cart: {}",
            how("carts")
        );
        assert!(
            how("paymnt").starts_with("fuzzy"),
            "paymnt -> payment: {}",
            how("paymnt")
        );
        assert_eq!(
            how("billing"),
            "unmatched",
            "billing is nothing like payment"
        );
    }

    #[test]
    fn config_naming_an_undiscovered_service_is_an_error() {
        let services = vec![svc("payment", false)];
        let mut config = RuntimeConfig::default();
        config.map.insert("pay".into(), "payments-v2".into());
        let err = match_names(&names(&["pay"]), &services, &config).unwrap_err();
        assert!(matches!(err, RuntimeError::Config(_)), "{err}");
        assert!(err.to_string().contains("payments-v2"), "{err}");
    }
}
