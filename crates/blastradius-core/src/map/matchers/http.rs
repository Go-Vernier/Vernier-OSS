//! URLs, `host:port` literals, templates around them, and environment
//! variables that name a host. The resolver types the edge by its target,
//! so a Redis URL found here becomes a database edge.
use super::{FileContext, Matcher};
use crate::map::facts::{Arg, Fact, Part, env_default};
use crate::map::resolve::{host_port, is_hostish_var, parse_url};
use crate::map::{Candidate, Target};
use crate::model::Evidence;

pub struct Http;

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

impl Matcher for Http {
    fn name(&self) -> &'static str {
        "http"
    }

    fn candidates(&self, ctx: &FileContext<'_>) -> Vec<Candidate> {
        let mut out = Vec::new();
        let push = |out: &mut Vec<Candidate>, target: Target, line: u32, detail: String| {
            out.push(Candidate {
                target,
                kind_hint: None,
                evidence: Evidence {
                    file: ctx.file.to_string(),
                    line: Some(line),
                    detail: Some(detail),
                },
            });
        };
        let infra = &ctx.config.infrastructure_names;

        for fact in ctx.facts {
            match fact {
                Fact::Str { value, line } => {
                    let v = value.trim();
                    if parse_url(v).is_some() {
                        push(&mut out, Target::Url(v.to_string()), *line, v.to_string());
                    } else if host_port(v).is_some() {
                        push(
                            &mut out,
                            Target::HostPort(v.to_string()),
                            *line,
                            v.to_string(),
                        );
                    } else if v.len() >= 4 && infra.iter().any(|n| n == v) {
                        push(
                            &mut out,
                            Target::BareName(v.to_string()),
                            *line,
                            format!("\"{v}\""),
                        );
                    }
                }
                Fact::Template { parts, line } => {
                    if template_is_interesting(parts) {
                        push(
                            &mut out,
                            Target::Template(parts.clone()),
                            *line,
                            render(parts),
                        );
                    }
                }
                Fact::EnvRef {
                    name,
                    default,
                    line,
                } => {
                    let default_is_host = default.as_deref().is_some_and(|d| {
                        parse_url(d).is_some()
                            || host_port(d).is_some()
                            || infra.iter().any(|n| n == d)
                    });
                    if is_hostish_var(name) || default_is_host {
                        push(
                            &mut out,
                            Target::EnvVar {
                                name: name.clone(),
                                default: default.clone(),
                            },
                            *line,
                            name.clone(),
                        );
                    }
                }
                Fact::Annotation { name, args, line } if name.ends_with("FeignClient") => {
                    for arg in args {
                        if let Arg::Str(s) = arg {
                            let target = if parse_url(s).is_some() {
                                Target::Url(s.clone())
                            } else {
                                Target::Host(s.clone())
                            };
                            push(&mut out, target, *line, format!("@FeignClient {s}"));
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
                                push(
                                    &mut out,
                                    Target::Template(parts.clone()),
                                    *line,
                                    render(parts),
                                );
                            }
                        }
                    }
                }
                Fact::Annotation { .. } | Fact::Import { .. } | Fact::Extends { .. } => {}
            }
        }
        out
    }
}
