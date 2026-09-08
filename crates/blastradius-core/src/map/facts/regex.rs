//! Facts for languages without a grammar and for configuration files.
//! Line by line: quoted strings, bare URLs, `${VAR}` references,
//! environment lookups, calls with their string arguments, imports.
use std::sync::LazyLock;

use regex::Regex;

use super::{Arg, Fact, Part, env_default, split_template};

static QUOTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#""((?:[^"\\]|\\.)*)"|'((?:[^'\\]|\\.)*)'|`((?:[^`\\]|\\.)*)`"#).unwrap()
});
static BARE_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\b[a-z][a-z0-9+.:-]*://[^\s"'<>;,)]+"#).unwrap());
static BRACED_VAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$\{([^}]+)\}").unwrap());
static BARE_VAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(^|[^\\$\w])\$([A-Za-z_][A-Za-z0-9_]{2,})\b").unwrap());
static ENV_CALL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?:getenv|environ\.get|get_env|GetEnvironmentVariable|env::var|ENV\.fetch|Deno\.env\.get|LookupEnv)\s*\(\s*(['"])([A-Za-z_][A-Za-z0-9_]*)['"](?:\s*,\s*(['"])((?:[^'"\\]|\\.)*)['"])?"#,
    )
    .unwrap()
});
static ENV_INDEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bENV\s*\[\s*['"]([A-Za-z_][A-Za-z0-9_]*)['"]\s*\]"#).unwrap());
static OR_DEFAULT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^\s*\)?\s*(?:\|\||\?\?|\?:|or)\s*['"]((?:[^'"\\]|\\.)*)['"]"#).unwrap()
});
static CALL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([A-Za-z_$][\w$]*(?:(?:\.|->|::)[A-Za-z_][\w$]*)*)\s*\(").unwrap()
});
static IMPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^\s*(?:import|require|require_once|include|include_once|use|from|load)\b[^'"\n]*?['"]([^'"]+)['"]"#).unwrap()
});
/// `key: value`, `key = value`, `- key: value`, `export KEY=value`; the value
/// may be quoted; a trailing `# comment` is dropped.
static SETTING_KV: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"^\s*(?:export\s+|-\s+)?([A-Za-z_][\w.\-]*(?:\[[^\]]*\])?)\s*[:=]\s*(?:"([^"]*)"|'([^']*)'|([^\s#]+))\s*(?:#.*)?$"#,
    )
    .unwrap()
});
/// `<artifactId>ts-common</artifactId>` on one line.
static SETTING_XML: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*<([A-Za-z_][\w.\-]*)(?:\s[^>]*)?>([^<]+)</([A-Za-z_][\w.\-]*)>\s*$").unwrap()
});
/// Dockerfile `ENV KEY value`, `ENV KEY=value`, `ARG KEY="value"`.
static SETTING_DOCKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"^\s*(?:ENV|ARG)\s+([A-Za-z_]\w*)(?:\s*=\s*|\s+)(?:"([^"]*)"|'([^']*)'|(\S+))\s*$"#,
    )
    .unwrap()
});
/// `name = <expression ending in a string literal>`, with optional modifiers
/// (`public final static`, `const`, `val`), an optional type before the name
/// or after a colon, and an optional `self.`/`this.`/`$` prefix. `==` and
/// `=>` are not assignments.
static ASSIGN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^\s*(?:(?:public|private|protected|internal|static|final|readonly|const|var|let|val|export|override|lazy|volatile)\s+)*(?:[A-Za-z_][\w<>\[\]?.,]*\s+)?(?:(?:self|this|cls)\.)?\$?([A-Za-z_]\w*)\s*(?::\s*[\w<>\[\]?.]+)?\s*=\s*([^=>\s].*)$",
    )
    .unwrap()
});

const KEYWORDS: &[&str] = &[
    "if", "elif", "else", "while", "for", "foreach", "switch", "case", "return", "function", "def",
    "fn", "catch", "match", "when", "unless", "until", "print", "echo", "defined", "not", "and",
    "or", "in", "is", "new", "typeof", "sizeof", "assert", "raise", "throw", "yield", "await",
];

pub(super) fn extract(text: &str) -> Vec<Fact> {
    let mut facts = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = u32::try_from(i + 1).unwrap_or(u32::MAX);
        let trimmed = raw.trim_start();
        if trimmed.starts_with('#') && !trimmed.starts_with("#{") || trimmed.starts_with("//") {
            continue;
        }
        extract_line(raw, line, &mut facts);
        facts.extend(setting_line(raw, line).or_else(|| assignment_line(raw, line)));
    }
    facts
}

/// Names assigned a string literal, for any language.
pub(super) fn assignments(text: &str) -> Vec<Fact> {
    text.lines()
        .enumerate()
        .filter_map(|(i, raw)| assignment_line(raw, u32::try_from(i + 1).unwrap_or(u32::MAX)))
        .collect()
}

fn quoted_group(caps: &regex::Captures<'_>, first: usize) -> String {
    caps.get(first)
        .or_else(|| caps.get(first + 1))
        .or_else(|| caps.get(first + 2))
        .map_or("", |m| m.as_str())
        .to_string()
}

fn setting_line(raw: &str, line: u32) -> Option<Fact> {
    let trimmed = raw.trim_start();
    if trimmed.starts_with('#') || trimmed.starts_with("//") {
        return None;
    }
    if let Some(caps) = SETTING_DOCKER.captures(raw) {
        return Some(Fact::Setting {
            key: caps[1].to_string(),
            value: quoted_group(&caps, 2),
            line,
        });
    }
    if let Some(caps) = SETTING_XML.captures(raw) {
        if caps[1] == caps[3] {
            return Some(Fact::Setting {
                key: caps[1].to_string(),
                value: caps[2].trim().to_string(),
                line,
            });
        }
    }
    let caps = SETTING_KV.captures(raw)?;
    let value = quoted_group(&caps, 2);
    (!value.is_empty()).then(|| Fact::Setting {
        key: caps[1].to_string(),
        value,
        line,
    })
}

/// The last string literal on the line is the value, and only when the line
/// ends with it (ignoring `;`, `,` and closing brackets), so `foo("a").bar()`
/// binds nothing.
fn assignment_line(raw: &str, line: u32) -> Option<Fact> {
    let caps = ASSIGN.captures(raw)?;
    let name = caps[1].to_string();
    let rest = caps[2]
        .trim_end()
        .trim_end_matches([';', ',', ')', ']', '}'])
        .trim_end();
    if !rest.ends_with(['"', '\'', '`']) {
        return None;
    }
    let last = QUOTED.captures_iter(rest).last()?;
    let value = last
        .get(1)
        .or_else(|| last.get(2))
        .or_else(|| last.get(3))
        .map_or("", |m| m.as_str());
    if value.is_empty() {
        return None;
    }
    Some(Fact::Setting {
        key: name,
        value: value.to_string(),
        line,
    })
}

fn push_string(value: &str, line: u32, facts: &mut Vec<Fact>) {
    if value.is_empty() {
        return;
    }
    let parts = split_template(value);
    if parts.iter().any(|p| matches!(p, Part::Var(_))) {
        facts.push(Fact::Template { parts, line });
    } else {
        facts.push(Fact::Str {
            value: value.to_string(),
            line,
        });
    }
}

fn extract_line(raw: &str, line: u32, facts: &mut Vec<Fact>) {
    // Quoted strings, remembering their spans so bare URLs inside them are
    // not counted twice.
    let mut quoted_spans: Vec<(usize, usize)> = Vec::new();
    for caps in QUOTED.captures_iter(raw) {
        let whole = caps.get(0).unwrap();
        quoted_spans.push((whole.start(), whole.end()));
        let content = caps
            .get(1)
            .or_else(|| caps.get(2))
            .or_else(|| caps.get(3))
            .map_or("", |m| m.as_str());
        push_string(content, line, facts);
    }
    for m in BARE_URL.find_iter(raw) {
        if quoted_spans
            .iter()
            .any(|(s, e)| m.start() >= *s && m.end() <= *e)
        {
            continue;
        }
        push_string(m.as_str(), line, facts);
    }

    // ${VAR}, ${VAR:-default}, $VAR
    for caps in BRACED_VAR.captures_iter(raw) {
        let (name, default) = env_default(&caps[1]);
        if super::is_var_name(&name) {
            facts.push(Fact::EnvRef {
                name,
                default,
                line,
            });
        }
    }
    for caps in BARE_VAR.captures_iter(raw) {
        let name = caps[2].to_string();
        if name.chars().any(|c| c.is_ascii_uppercase()) {
            facts.push(Fact::EnvRef {
                name,
                default: None,
                line,
            });
        }
    }

    // Environment lookups as calls or ENV[...] indexing.
    for caps in ENV_CALL.captures_iter(raw) {
        let name = caps[2].to_string();
        let default = caps
            .get(4)
            .map(|m| m.as_str().to_string())
            .or_else(|| or_default(&raw[caps.get(0).unwrap().end()..]));
        facts.push(Fact::EnvRef {
            name,
            default,
            line,
        });
    }
    for caps in ENV_INDEX.captures_iter(raw) {
        let name = caps[1].to_string();
        let default = or_default(&raw[caps.get(0).unwrap().end()..]);
        facts.push(Fact::EnvRef {
            name,
            default,
            line,
        });
    }

    // Calls with their string arguments.
    for caps in CALL.captures_iter(raw) {
        let callee = caps[1].to_string();
        let last = callee.rsplit(['.', ':', '>']).next().unwrap_or(&callee);
        if KEYWORDS.contains(&last) || KEYWORDS.contains(&callee.as_str()) {
            continue;
        }
        let open = caps.get(0).unwrap().end();
        let inner = balanced(&raw[open..]);
        let args: Vec<Arg> = QUOTED
            .captures_iter(inner)
            .map(|c| {
                let s = c
                    .get(1)
                    .or_else(|| c.get(2))
                    .or_else(|| c.get(3))
                    .map_or("", |m| m.as_str());
                let parts = split_template(s);
                if parts.iter().any(|p| matches!(p, Part::Var(_))) {
                    Arg::Template(parts)
                } else {
                    Arg::Str(s.to_string())
                }
            })
            .collect();
        facts.push(Fact::Call { callee, args, line });
    }

    if let Some(caps) = IMPORT.captures(raw) {
        facts.push(Fact::Import {
            path: caps[1].to_string(),
            line,
        });
    }
}

/// The literal after `|| `, `?? `, `?: ` or `or ` when one follows.
fn or_default(rest: &str) -> Option<String> {
    OR_DEFAULT.captures(rest).map(|c| c[1].to_string())
}

/// The text up to the parenthesis that closes an already-open one, or the
/// rest of the line.
fn balanced(s: &str) -> &str {
    let mut depth = 1usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return &s[..i];
                }
            }
            _ => {}
        }
    }
    s
}
