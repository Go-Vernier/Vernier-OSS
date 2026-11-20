//! The agent judge: the user's own signed-in Claude Code or Codex reads the
//! repository, read-only, and says whether the links static analysis was
//! least sure of are real. It judges only `inferred` and `uncertain` links;
//! the rest were found in the code or seen in production.
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::agent::{self, Agent, AgentError};
use crate::analyze::Analysis;
use crate::blast::{Blast, Hop, Relation};
use crate::graph::BlastGraph;
use crate::model::{Confidence, Evidence};

pub const JUDGE_TIMEOUT_SECONDS: u64 = 600;

/// More than this and the agent runs out of time reading; the rest stay
/// unjudged.
pub const MAX_LINKS: usize = 20;

const MAX_CHECK: usize = 6;
const MAX_EVIDENCE: usize = 3;
const SNIPPET_CHARS: usize = 300;

pub const SYSTEM: &str = "You are reviewing links that a static analyzer reported between services in a repository. \
For each link you are given, read the evidence files and their callers, and decide whether the code really makes the link. \
Verdicts: confirmed = the code really makes that call, consumes that topic or shares that table at runtime; \
doubtful = the code does not (dead code, an unused string, a test, a different service with a similar name); \
unclear = you cannot tell from the code (configuration decides). \
Cite the file and line that settle each verdict. Judge only the links listed, and use the same from and to names. \
Also give an overall risk (low, medium or high) for the change, a short summary, and up to six things the reviewer should check. \
Never claim a service is safe; a link you found no evidence for is doubtful, not harmless. \
Do not modify any files.";

/// One link put to the agent. `from` and `to` read as the statement
/// `from <relation> to`: the opposite way round from a blast [`Hop`], whose
/// `from` is the service already in the radius.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub from: String,
    pub to: String,
    pub relation: Relation,
    pub confidence: Confidence,
    pub evidence: Vec<Evidence>,
}

/// The distinct inferred and uncertain links in the blast radius, in report
/// order, with the evidence of the edges that produced each hop.
pub fn links_to_judge(blast: &Blast, graph: &BlastGraph) -> Vec<Link> {
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut links = Vec::new();
    for hop in blast.reached.iter().flat_map(|r| &r.path) {
        if !matches!(hop.confidence, Confidence::Inferred | Confidence::Uncertain) {
            continue;
        }
        if !seen.insert((hop.to.clone(), hop.from.clone())) {
            continue;
        }
        links.push(Link {
            from: hop.to.clone(),
            to: hop.from.clone(),
            relation: hop.relation,
            confidence: hop.confidence,
            evidence: hop_evidence(graph, hop),
        });
        if links.len() == MAX_LINKS {
            break;
        }
    }
    links
}

/// The edges `blast::steps` read to make `hop`. Calls, imports and shared
/// databases come from the direct edge, dependent to dependency; consumes
/// from the publisher's event edge; a shared broker from both services'
/// edges to the broker.
fn hop_evidence(graph: &BlastGraph, hop: &Hop) -> Vec<Evidence> {
    let edges = graph.edges();
    let mut found: Vec<Evidence> = Vec::new();
    let mut take = |source: &str, target: &str| {
        for e in edges
            .iter()
            .filter(|e| e.source == source && e.target == target && e.edge_type == hop.edge_type)
        {
            for ev in &e.evidence {
                if !found.contains(ev) {
                    found.push(ev.clone());
                }
            }
        }
    };
    match (hop.relation, hop.via.as_deref()) {
        (Relation::Consumes, _) => take(&hop.from, &hop.to),
        (Relation::SharesBroker, Some(via)) => {
            take(&hop.to, via);
            take(&hop.from, via);
        }
        (Relation::SharesBroker, None) => {}
        _ => take(&hop.to, &hop.from),
    }
    found.truncate(MAX_EVIDENCE);
    found
}

fn place(e: &Evidence) -> String {
    match e.line {
        Some(line) => format!("{}:{line}", e.file),
        None => e.file.clone(),
    }
}

/// Every object is closed and lists all its keys as required, which the
/// strict structured-output modes of both agents demand; the optional file
/// and line are nullable instead.
pub fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["risk", "summary", "links", "check"],
        "properties": {
            "risk": {"type": "string", "enum": ["low", "medium", "high"]},
            "summary": {"type": "string"},
            "links": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["from", "to", "verdict", "why", "file", "line"],
                    "properties": {
                        "from": {"type": "string"},
                        "to": {"type": "string"},
                        "verdict": {"type": "string", "enum": ["confirmed", "doubtful", "unclear"]},
                        "why": {"type": "string"},
                        "file": {"type": ["string", "null"]},
                        "line": {"type": ["integer", "null"]}
                    }
                }
            },
            "check": {"type": "array", "items": {"type": "string"}}
        }
    })
}

/// The user message: the blast radius, then the links to judge.
pub fn prompt(repository: &str, blast: &Blast, links: &[Link]) -> String {
    let json = serde_json::to_string_pretty(blast).unwrap_or_default();
    let mut out = format!("Repository: {repository}\n\nBlast radius:\n{json}\n\nLinks to judge:\n");
    for l in links {
        let evidence = if l.evidence.is_empty() {
            "none recorded".to_string()
        } else {
            l.evidence.iter().map(place).collect::<Vec<_>>().join(", ")
        };
        let line = format!(
            "- {} -> {} {} ({}) evidence: {evidence}\n",
            l.from,
            l.to,
            l.relation.as_str(),
            l.confidence.as_str()
        );
        out.push_str(&line);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    Low,
    Medium,
    High,
}

impl Risk {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Confirmed,
    Doubtful,
    Unclear,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Doubtful => "doubtful",
            Self::Unclear => "unclear",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkVerdict {
    pub from: String,
    pub to: String,
    pub verdict: Verdict,
    pub why: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// The judgment as printed and as the JSON carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Judgment {
    /// `claude-code` or `codex`.
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub risk: Risk,
    pub summary: String,
    pub links: Vec<LinkVerdict>,
    pub check: Vec<String>,
}

#[derive(Deserialize)]
struct RawJudgment {
    risk: Risk,
    summary: String,
    links: Vec<LinkVerdict>,
    check: Vec<String>,
}

fn head(text: &str) -> String {
    text.chars().take(SNIPPET_CHARS).collect()
}

fn tail(text: &str) -> String {
    let chars: Vec<char> = text.trim().chars().collect();
    chars[chars.len().saturating_sub(SNIPPET_CHARS)..]
        .iter()
        .collect()
}

fn unreadable(agent: Agent, got: &str) -> AgentError {
    AgentError::Failed(format!(
        "{} returned an answer Vernier can't read: {}",
        agent.label(),
        head(got)
    ))
}

/// Checks the agent's answer against the schema and against what was asked:
/// verdicts for links nobody asked about are dropped, the first verdict per
/// link wins, and a link the agent skipped is `unclear`, never silently
/// absent. The result follows the order of `asked`.
pub fn parse(agent: Agent, raw: &Value, asked: &[Link]) -> Result<Judgment, AgentError> {
    let parsed: RawJudgment =
        serde_json::from_value(raw.clone()).map_err(|_| unreadable(agent, &raw.to_string()))?;
    let links = asked
        .iter()
        .map(|a| {
            let found = parsed
                .links
                .iter()
                .find(|v| v.from == a.from && v.to == a.to);
            match found {
                Some(v) => {
                    let file = v
                        .file
                        .as_deref()
                        .map(str::trim)
                        .filter(|f| !f.is_empty())
                        .map(str::to_string);
                    LinkVerdict {
                        from: a.from.clone(),
                        to: a.to.clone(),
                        verdict: v.verdict,
                        why: v.why.trim().to_string(),
                        line: file.as_ref().and(v.line),
                        file,
                    }
                }
                None => LinkVerdict {
                    from: a.from.clone(),
                    to: a.to.clone(),
                    verdict: Verdict::Unclear,
                    why: "not judged".into(),
                    file: None,
                    line: None,
                },
            }
        })
        .collect();
    let check = parsed
        .check
        .iter()
        .map(|c| c.trim())
        .filter(|c| !c.is_empty())
        .take(MAX_CHECK)
        .map(str::to_string)
        .collect();
    Ok(Judgment {
        agent: match agent {
            Agent::ClaudeCode => "claude-code",
            Agent::Codex => "codex",
        }
        .into(),
        model: None,
        risk: parsed.risk,
        summary: parsed.summary.trim().to_string(),
        links,
        check,
    })
}

/// The command line that runs one read-only judgment. Claude Code takes the
/// schema inline; Codex reads `schema.json` from `tmp` and writes its answer
/// to `answer.json` there. The prompt goes on stdin (`-` for Codex).
pub fn judge_argv(agent: Agent, model: Option<&str>, root: &Path, tmp: &Path) -> Vec<String> {
    let mut argv: Vec<String> = vec![agent.binary().into()];
    let mut add = |args: &[&str]| argv.extend(args.iter().map(|a| (*a).to_string()));
    match agent {
        Agent::ClaudeCode => {
            add(&[
                "-p",
                "--output-format",
                "json",
                "--tools",
                "Read,Grep,Glob",
                "--permission-mode",
                "dontAsk",
                "--strict-mcp-config",
                "--no-session-persistence",
                "--disable-slash-commands",
            ]);
            if let Some(m) = model {
                add(&["--model", m]);
            }
            add(&[
                "--system-prompt",
                SYSTEM,
                "--json-schema",
                &schema().to_string(),
            ]);
        }
        Agent::Codex => {
            add(&[
                "exec",
                "--sandbox",
                "read-only",
                "--skip-git-repo-check",
                "--ephemeral",
                "--color",
                "never",
            ]);
            if let Some(m) = model {
                add(&["-m", m]);
            }
            add(&[
                "-C",
                &root.display().to_string(),
                "--output-schema",
                &tmp.join("schema.json").display().to_string(),
                "-o",
                &tmp.join("answer.json").display().to_string(),
                "-c",
                &format!(
                    "developer_instructions={}",
                    serde_json::to_string(SYSTEM).unwrap_or_default()
                ),
                "-",
            ]);
        }
    }
    argv
}

/// Runs the agent on `links` and returns its checked judgment. The agent is
/// the user's own signed-in CLI; nothing here reads its credentials.
pub fn judge(
    agent: Agent,
    model: Option<&str>,
    analysis: &Analysis,
    links: &[Link],
) -> Result<Judgment, AgentError> {
    let Some(blast) = &analysis.blast else {
        return Err(AgentError::Failed(
            "vernier: nothing to judge without a change".into(),
        ));
    };
    let binary = agent::which(agent.binary())
        .ok_or_else(|| AgentError::NotReady(agent.install_hint().to_string()))?;
    let tmp = std::env::temp_dir().join(format!("vernier-judge-{}", std::process::id()));
    std::fs::create_dir_all(&tmp)
        .map_err(|e| AgentError::Failed(format!("vernier: can't create {}: {e}", tmp.display())))?;
    let result = run_judge(agent, model, analysis, blast, links, &binary, &tmp);
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

fn run_judge(
    agent: Agent,
    model: Option<&str>,
    analysis: &Analysis,
    blast: &Blast,
    links: &[Link],
    binary: &Path,
    tmp: &Path,
) -> Result<Judgment, AgentError> {
    let write_err = |e: std::io::Error| {
        AgentError::Failed(format!("vernier: can't write {}: {e}", tmp.display()))
    };
    std::fs::write(tmp.join("schema.json"), schema().to_string()).map_err(write_err)?;
    let mut argv = judge_argv(agent, model, &analysis.root, tmp);
    argv[0] = binary.display().to_string();
    let out = agent::run(
        &argv,
        &prompt(&analysis.repository, blast, links),
        &analysis.root,
        Duration::from_secs(JUDGE_TIMEOUT_SECONDS),
    )?;
    let label = agent.label();
    if !out.status_ok {
        let text = if out.stderr.trim().is_empty() {
            &out.stdout
        } else {
            &out.stderr
        };
        return Err(AgentError::Failed(format!(
            "{label} failed: {}",
            tail(text)
        )));
    }
    let answer = match agent {
        Agent::ClaudeCode => {
            let envelope: Value =
                serde_json::from_str(&out.stdout).map_err(|_| unreadable(agent, &out.stdout))?;
            if envelope["is_error"].as_bool() == Some(true) {
                let why = envelope["result"].as_str().unwrap_or("unknown error");
                return Err(AgentError::Failed(format!("{label} failed: {why}")));
            }
            match envelope.get("structured_output") {
                Some(v) if v.is_object() => v.clone(),
                _ => return Err(unreadable(agent, &out.stdout)),
            }
        }
        Agent::Codex => {
            let text = std::fs::read_to_string(tmp.join("answer.json"))
                .map_err(|_| unreadable(agent, "no answer file was written"))?;
            serde_json::from_str(&text).map_err(|_| unreadable(agent, &text))?
        }
    };
    let mut judgment = parse(agent, &answer, links)?;
    judgment.model = model.map(str::to_string);
    Ok(judgment)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Agent, AgentError};
    use crate::blast::{Blast, Change, Hop, Reached, Relation};
    use crate::graph::BlastGraph;
    use crate::model::{
        Confidence, DiscoveryStrategy, Edge, EdgeType, Evidence, Service, ServiceRole,
        ServiceSource,
    };
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::path::Path;

    fn service(name: &str) -> Service {
        Service {
            name: name.into(),
            root: Some(name.into()),
            language: None,
            entry_points: vec![],
            role: ServiceRole::Code,
            discovered_by: ServiceSource::Strategy(DiscoveryStrategy::Monorepo),
            evidence: Evidence {
                file: format!("{name}/go.mod"),
                line: None,
                detail: None,
            },
            image: None,
            package_name: None,
        }
    }

    fn ev(file: &str, line: u32) -> Evidence {
        Evidence {
            file: file.into(),
            line: Some(line),
            detail: None,
        }
    }

    fn edge(s: &str, t: &str, ty: EdgeType, c: Confidence, evidence: Vec<Evidence>) -> Edge {
        Edge {
            source: s.into(),
            target: t.into(),
            edge_type: ty,
            confidence: c,
            evidence,
            observed: None,
        }
    }

    fn hop(from: &str, to: &str, relation: Relation, ty: EdgeType, c: Confidence) -> Hop {
        Hop {
            from: from.into(),
            to: to.into(),
            relation,
            edge_type: ty,
            confidence: c,
            via: None,
            calls: None,
        }
    }

    fn graph() -> BlastGraph {
        let mut g = BlastGraph::new();
        for n in ["web", "cart", "payment", "dispatch"] {
            g.add_service(service(n));
        }
        g.add_edge(edge(
            "web",
            "cart",
            EdgeType::Http,
            Confidence::Uncertain,
            vec![ev("web/app.js", 12)],
        ))
        .unwrap();
        g.add_edge(edge(
            "payment",
            "cart",
            EdgeType::Http,
            Confidence::Static,
            vec![ev("payment/main.go", 3)],
        ))
        .unwrap();
        g.add_edge(edge(
            "payment",
            "dispatch",
            EdgeType::Event,
            Confidence::Inferred,
            vec![ev("payment/pub.go", 9)],
        ))
        .unwrap();
        g
    }

    fn blast() -> Blast {
        let web_cart = hop(
            "cart",
            "web",
            Relation::Calls,
            EdgeType::Http,
            Confidence::Uncertain,
        );
        let pay_cart = hop(
            "cart",
            "payment",
            Relation::Calls,
            EdgeType::Http,
            Confidence::Static,
        );
        let dispatch = hop(
            "payment",
            "dispatch",
            Relation::Consumes,
            EdgeType::Event,
            Confidence::Inferred,
        );
        let reached = vec![
            Reached {
                service: "web".into(),
                depth: 1,
                confidence: Confidence::Uncertain,
                path: vec![web_cart.clone()],
            },
            Reached {
                service: "payment".into(),
                depth: 1,
                confidence: Confidence::Static,
                path: vec![pay_cart.clone()],
            },
            Reached {
                service: "dispatch".into(),
                depth: 2,
                confidence: Confidence::Inferred,
                path: vec![pay_cart, dispatch],
            },
            Reached {
                service: "web".into(),
                depth: 2,
                confidence: Confidence::Uncertain,
                path: vec![web_cart],
            },
        ];
        Blast {
            change: Change::from_files(&["cart/api.go".to_string()]),
            depth: 3,
            changed: vec![],
            unowned: vec![],
            reached,
            infrastructure: vec![],
            not_reached: vec![],
            summary: crate::blast::Summary {
                changed: 1,
                reached: 3,
                by_confidence: crate::blast::ByConfidence::default(),
            },
        }
    }

    fn link(from: &str, to: &str) -> Link {
        Link {
            from: from.into(),
            to: to.into(),
            relation: Relation::Calls,
            confidence: Confidence::Uncertain,
            evidence: vec![],
        }
    }

    #[test]
    fn links_to_judge_takes_only_inferred_and_uncertain_once_each() {
        let links = links_to_judge(&blast(), &graph());
        let pairs: Vec<(&str, &str)> = links
            .iter()
            .map(|l| (l.from.as_str(), l.to.as_str()))
            .collect();
        assert_eq!(pairs, vec![("web", "cart"), ("dispatch", "payment")]);
        assert_eq!(links[0].relation, Relation::Calls);
        assert_eq!(links[0].evidence, vec![ev("web/app.js", 12)]);
        assert_eq!(links[1].relation, Relation::Consumes);
        assert_eq!(links[1].evidence, vec![ev("payment/pub.go", 9)]);
    }

    #[test]
    fn shared_broker_evidence_comes_from_both_edges_to_the_broker() {
        let mut g = graph();
        g.add_service(Service {
            role: ServiceRole::Infrastructure,
            root: None,
            ..service("rabbitmq")
        });
        g.add_edge(edge(
            "payment",
            "rabbitmq",
            EdgeType::Event,
            Confidence::Static,
            vec![ev("payment/mq.go", 1)],
        ))
        .unwrap();
        g.add_edge(edge(
            "web",
            "rabbitmq",
            EdgeType::Event,
            Confidence::Static,
            vec![ev("web/mq.js", 2)],
        ))
        .unwrap();
        let mut h = hop(
            "payment",
            "web",
            Relation::SharesBroker,
            EdgeType::Event,
            Confidence::Uncertain,
        );
        h.via = Some("rabbitmq".into());
        let mut b = blast();
        b.reached = vec![Reached {
            service: "web".into(),
            depth: 1,
            confidence: Confidence::Uncertain,
            path: vec![h],
        }];
        let links = links_to_judge(&b, &g);
        assert_eq!(
            links[0].evidence,
            vec![ev("web/mq.js", 2), ev("payment/mq.go", 1)]
        );
    }

    #[test]
    fn links_are_capped() {
        let mut b = blast();
        b.reached = (0..MAX_LINKS + 5)
            .map(|i| Reached {
                service: format!("s{i}"),
                depth: 1,
                confidence: Confidence::Uncertain,
                path: vec![hop(
                    "cart",
                    &format!("s{i}"),
                    Relation::Calls,
                    EdgeType::Http,
                    Confidence::Uncertain,
                )],
            })
            .collect();
        assert_eq!(links_to_judge(&b, &graph()).len(), MAX_LINKS);
    }

    fn objects(v: &serde_json::Value, found: &mut usize) {
        match v {
            serde_json::Value::Object(map) => {
                if let Some(props) = map.get("properties") {
                    *found += 1;
                    assert_eq!(map.get("additionalProperties"), Some(&json!(false)), "{v}");
                    let mut keys: Vec<&str> = props
                        .as_object()
                        .unwrap()
                        .keys()
                        .map(String::as_str)
                        .collect();
                    let mut required: Vec<&str> = map["required"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|s| s.as_str().unwrap())
                        .collect();
                    keys.sort_unstable();
                    required.sort_unstable();
                    assert_eq!(keys, required);
                }
                map.values().for_each(|c| objects(c, found));
            }
            serde_json::Value::Array(items) => items.iter().for_each(|c| objects(c, found)),
            _ => {}
        }
    }

    #[test]
    fn schema_is_strict() {
        let mut found = 0;
        objects(&schema(), &mut found);
        assert_eq!(found, 2, "the root and each link");
    }

    #[test]
    fn prompt_lists_judged_links_with_evidence() {
        let b = blast();
        let links = links_to_judge(&b, &graph());
        let p = prompt("acme/shop", &b, &links);
        assert!(
            p.contains("web -> cart calls (uncertain) evidence: web/app.js:12"),
            "{p}"
        );
        assert!(p.contains("acme/shop"), "{p}");
        assert!(
            p.contains(&serde_json::to_string_pretty(&b).unwrap()),
            "{p}"
        );
        let none = prompt("r", &b, &[link("a", "b")]);
        assert!(none.contains("a -> b calls (uncertain) evidence: none recorded"));
    }

    #[test]
    fn parse_drops_unknown_links_keeps_first_and_fills_skipped() {
        let row = |f: &str, t: &str, v: &str| json!({"from": f, "to": t, "verdict": v, "why": " because ", "file": null, "line": null});
        let raw = json!({
            "risk": "medium",
            "summary": " Cart changed. ",
            "links": [row("a", "b", "confirmed"), row("a", "b", "doubtful"), row("x", "y", "confirmed")],
            "check": ["  run tests ", "", "   "],
        });
        let j = parse(Agent::ClaudeCode, &raw, &[link("a", "b"), link("c", "d")]).unwrap();
        assert_eq!(j.agent, "claude-code");
        assert_eq!(j.model, None);
        assert_eq!(j.risk, Risk::Medium);
        assert_eq!(j.summary, "Cart changed.");
        assert_eq!(j.check, vec!["run tests".to_string()]);
        assert_eq!(j.links.len(), 2);
        assert_eq!(j.links[0].verdict, Verdict::Confirmed);
        assert_eq!(j.links[0].why, "because");
        assert_eq!(
            (j.links[1].from.as_str(), j.links[1].to.as_str()),
            ("c", "d")
        );
        assert_eq!(j.links[1].verdict, Verdict::Unclear);
        assert_eq!(j.links[1].why, "not judged");
    }

    #[test]
    fn parse_caps_check_at_six() {
        let raw = json!({
            "risk": "low", "summary": "s", "links": [],
            "check": ["1","2","3","4","5","6","7","8"],
        });
        let j = parse(Agent::Codex, &raw, &[]).unwrap();
        assert_eq!(j.agent, "codex");
        assert_eq!(j.check.len(), 6);
    }

    #[test]
    fn parse_rejects_wrong_shape() {
        let raw = json!({"risk": "extreme"});
        let Err(AgentError::Failed(m)) = parse(Agent::Codex, &raw, &[]) else {
            panic!("expected Failed");
        };
        assert!(m.contains("Codex returned"), "{m}");
        assert!(m.contains("extreme"), "{m}");
    }

    #[test]
    fn judge_argv_is_read_only() {
        let root = Path::new("/repo");
        let tmp = Path::new("/tmp/j");
        let claude = judge_argv(Agent::ClaudeCode, None, root, tmp);
        assert_eq!(claude[0], "claude");
        for want in ["--tools", "Read,Grep,Glob", "dontAsk", "--json-schema"] {
            assert!(claude.iter().any(|a| a == want), "{claude:?}");
        }
        assert!(!claude.iter().any(|a| a == "--bare"));
        assert!(!claude.iter().any(|a| a == "--model"));
        let with = judge_argv(Agent::ClaudeCode, Some("opus"), root, tmp);
        let at = with.iter().position(|a| a == "--model").unwrap();
        assert_eq!(with[at + 1], "opus");

        let codex = judge_argv(Agent::Codex, None, root, tmp);
        assert_eq!(codex[0], "codex");
        for want in ["read-only", "-C", "/repo", "--output-schema"] {
            assert!(codex.iter().any(|a| a == want), "{codex:?}");
        }
        assert!(codex.iter().any(|a| a.ends_with("schema.json")));
        assert!(codex.iter().any(|a| a.ends_with("answer.json")));
        assert_eq!(codex.last().map(String::as_str), Some("-"));
        assert!(!codex.iter().any(|a| a == "-m"));
        let with = judge_argv(Agent::Codex, Some("gpt"), root, tmp);
        let at = with.iter().position(|a| a == "-m").unwrap();
        assert_eq!(with[at + 1], "gpt");
        assert_eq!(with.last().map(String::as_str), Some("-"));
    }
}
