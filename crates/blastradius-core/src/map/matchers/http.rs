//! URLs, `host:port` literals, templates around them, and environment
//! variables that name a host. The resolver types the edge by its target,
//! so a Redis URL found here becomes a database edge.
use super::{FileContext, Matcher};
use std::sync::LazyLock;

use regex::Regex;

use crate::map::facts::{Arg, Fact, Part, env_default};
use crate::map::resolve::{
    ado_connection, host_port, is_hostish_key, is_hostish_var, is_hostname, is_local, parse_url,
};
use crate::map::{Candidate, Target};
use crate::model::Evidence;

pub struct Http;

/// PHP PDO data source names: `mysql:host=mysql;dbname=ratings`.
static PDO_DSN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(mysql|pgsql|sqlsrv|oci|dblib|odbc|mongodb):host=([A-Za-z0-9.-]+)").unwrap()
});

fn render(parts: &[Part]) -> String {
    parts
        .iter()
        .map(|p| match p {
            Part::Lit(s) => s.clone(),
            Part::Var(v) => format!("${{{v}}}"),
        })
        .collect()
}

fn template_is_interesting(parts: &[Part]) -> bool {
    parts.iter().any(|p| match p {
        Part::Lit(l) => l.contains("://"),
        Part::Var(v) => is_hostish_var(&env_default(v).0),
    })
}

fn evidence(ctx: &FileContext<'_>, line: u32, detail: String) -> Evidence {
    Evidence {
        file: ctx.file.to_string(),
        line: Some(line),
        detail: Some(detail),
    }
}

/// A configuration key that names a host: `spring.data.mongodb.host: mongodb`,
/// `bootstrap-servers: kafka:9092`, Dockerfile `ENV REDIS_HOST redis`. URLs and
/// `${VAR}` values are found by the string and template paths instead.
fn from_setting(
    ctx: &FileContext<'_>,
    key: &str,
    value: &str,
    line: u32,
    out: &mut Vec<Candidate>,
) {
    if !is_hostish_key(key) {
        return;
    }
    let v = value.trim().trim_matches(['"', '\'']);
    if v.is_empty() || v.contains('$') || v.contains("://") || v.contains('{') {
        return;
    }
    let target = if host_port(v).is_some() {
        Target::HostPort(v.to_string())
    } else if is_hostname(v) {
        Target::Host(v.to_string())
    } else {
        return;
    };
    out.push(Candidate {
        target,
        kind_hint: None,
        evidence: evidence(ctx, line, format!("{key}={v}")),
    });
}

fn from_string(ctx: &FileContext<'_>, value: &str, line: u32, out: &mut Vec<Candidate>) {
    let v = value.trim();
    let names = &ctx.config.service_names;
    let target = if let Some((host, _)) = ado_connection(v) {
        if is_local(&host) {
            // A localhost connection string names no service; resolving it
            // would only add "localhost" to the unresolved list.
            return;
        }
        Target::Host(host)
    } else if let Some(dsn) = PDO_DSN.captures(v) {
        Target::Url(format!("{}://{}", &dsn[1], &dsn[2]))
    } else if parse_url(v).is_some() {
        Target::Url(v.to_string())
    } else if host_port(v).is_some() {
        Target::HostPort(v.to_string())
    } else if v.len() >= 4 && names.iter().any(|n| n == v) {
        // A plain literal naming a service: the resolver keeps it only when
        // that service is a datastore or broker.
        Target::BareName(v.to_string())
    } else {
        return;
    };
    let detail = if matches!(target, Target::BareName(_)) {
        format!("\"{v}\"")
    } else {
        v.to_string()
    };
    out.push(Candidate {
        target,
        kind_hint: None,
        evidence: evidence(ctx, line, detail),
    });
}

fn from_env(
    ctx: &FileContext<'_>,
    name: &str,
    default: Option<&str>,
    line: u32,
    out: &mut Vec<Candidate>,
) {
    let infra = &ctx.config.infrastructure_names;
    let default_is_host = default.is_some_and(|d| {
        parse_url(d).is_some() || host_port(d).is_some() || infra.iter().any(|n| n == d)
    });
    if is_hostish_var(name) || default_is_host {
        out.push(Candidate {
            target: Target::EnvVar {
                name: name.to_string(),
                default: default.map(str::to_string),
            },
            kind_hint: None,
            evidence: evidence(ctx, line, name.to_string()),
        });
    }
}

fn from_template(ctx: &FileContext<'_>, parts: &[Part], line: u32, out: &mut Vec<Candidate>) {
    out.push(Candidate {
        target: Target::Template(parts.to_vec()),
        kind_hint: None,
        evidence: evidence(ctx, line, render(parts)),
    });
}

impl Matcher for Http {
    fn name(&self) -> &'static str {
        "http"
    }

    fn candidates(&self, ctx: &FileContext<'_>) -> Vec<Candidate> {
        let mut out = Vec::new();
        for fact in ctx.facts {
            match fact {
                Fact::Str { value, line } => from_string(ctx, value, *line, &mut out),
                Fact::Template { parts, line } => {
                    if template_is_interesting(parts) {
                        from_template(ctx, parts, *line, &mut out);
                    }
                }
                Fact::EnvRef {
                    name,
                    default,
                    line,
                } => {
                    from_env(ctx, name, default.as_deref(), *line, &mut out);
                }
                Fact::Annotation { name, args, line } if name.ends_with("FeignClient") => {
                    for arg in args {
                        if let Arg::Str(s) = arg {
                            let target = if parse_url(s).is_some() {
                                Target::Url(s.clone())
                            } else {
                                Target::Host(s.clone())
                            };
                            out.push(Candidate {
                                target,
                                kind_hint: None,
                                evidence: evidence(ctx, *line, format!("@FeignClient {s}")),
                            });
                        }
                    }
                }
                Fact::Call { args, line, .. } => {
                    for arg in args {
                        if let Arg::Template(parts) = arg {
                            if parts
                                .iter()
                                .any(|p| matches!(p, Part::Lit(l) if l.contains("://")))
                            {
                                from_template(ctx, parts, *line, &mut out);
                            }
                        }
                    }
                }
                Fact::Setting { key, value, line } => {
                    from_setting(ctx, key, value, *line, &mut out);
                }
                Fact::Annotation { .. } | Fact::Import { .. } | Fact::Extends { .. } => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::config::ConfigIndex;
    use pretty_assertions::assert_eq;

    fn setting(key: &str, value: &str) -> Fact {
        Fact::Setting {
            key: key.into(),
            value: value.into(),
            line: 7,
        }
    }

    #[test]
    fn hostish_settings_become_host_candidates() {
        let facts = vec![
            setting("spring.data.mongodb.host", "mongodb"),
            setting("spring.data.mongodb.database", "inventory"),
            setting("bootstrap-servers", "kafka:9092"),
            setting("url", "jdbc:mysql://mysql:3306/x"),
            setting("host", "${MONGO_HOST:mongodb}"),
            setting("REDIS_HOST", "redis"),
            setting("server.port", "8080"),
        ];
        let cfg = ConfigIndex::default();
        let ctx = FileContext {
            service: "inventory",
            file: "inventory/src/main/resources/application.properties",
            facts: &facts,
            config: &cfg,
        };
        let out = Http.candidates(&ctx);
        let targets: Vec<(&Target, &str)> = out
            .iter()
            .map(|c| (&c.target, c.evidence.detail.as_deref().unwrap()))
            .collect();
        assert_eq!(
            targets,
            vec![
                (
                    &Target::Host("mongodb".into()),
                    "spring.data.mongodb.host=mongodb"
                ),
                (
                    &Target::HostPort("kafka:9092".into()),
                    "bootstrap-servers=kafka:9092"
                ),
                (&Target::Host("redis".into()), "REDIS_HOST=redis"),
            ]
        );
    }

    #[test]
    fn ado_connection_strings_name_their_host() {
        let facts = vec![Fact::Str {
            value: "Server=sql;Database=Shop;User Id=sa".into(),
            line: 3,
        }];
        let cfg = ConfigIndex::default();
        let ctx = FileContext {
            service: "ledger",
            file: "ledger/appsettings.json",
            facts: &facts,
            config: &cfg,
        };
        let out = Http.candidates(&ctx);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].target, Target::Host("sql".into()));
    }
}
