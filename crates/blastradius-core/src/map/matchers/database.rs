//! Which database each service uses, keyed by host and database name, so two
//! services on the same key get a database edge between them. The edge to the
//! datastore itself comes from the http matcher, which types edges by target.
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use super::{FileContext, Matcher};
use crate::map::config::ConfigIndex;
use crate::map::facts::{Arg, Extraction, Fact, Part, env_default};
use crate::map::resolve::{
    DATABASE_SCHEMES, ado_connection, is_hostish_key, is_hostname, parse_url,
};
use crate::map::symbols::Symbols;
use crate::map::{Candidate, Target};
use crate::model::{EdgeType, Evidence};

pub struct Database;

static PDO_DSN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:mysql|pgsql|sqlsrv|oci|dblib|odbc):host=([A-Za-z0-9.-]+).*?dbname=([^;\s]+)")
        .unwrap()
});
/// `AddNpgsqlDbContext<X>("name")`, `AddNpgsqlDataSource("name")`,
/// `AddSqlServerDbContext<X>("name")`, `AddMongoDBClient("name")`: the first
/// string argument is an Aspire database resource. Redis and brokers carry no
/// database name and are left out.
static NAMED_RESOURCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\.(Add(?:Npgsql|SqlServer|MySql|Oracle|MongoDB|Cosmos)\w*(?:DbContext|DataSource|Client)|GetConnectionString)$").unwrap()
});
/// The declaration half of `NAMED_RESOURCE`: a service that calls one of
/// these is the owner of the resource name, not just a reader of it.
static DECLARED_RESOURCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\.(Add(?:Npgsql|SqlServer|MySql|Oracle|MongoDB|Cosmos)\w*(?:DbContext|DataSource|Client))$").unwrap()
});
static OBJECT_HOST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\bhost(?:name)?\s*[:=]\s*['"]([^'"]+)['"]"#).unwrap());
static OBJECT_DATABASE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(?:database|db|dbname)\s*[:=]\s*['"]([^'"]+)['"]"#).unwrap()
});
const DATABASE_KEYS: &[&str] = &[
    "database",
    "dbname",
    "db",
    "database-name",
    "databasename",
    "database_name",
    "schema",
    "catalog",
    "initial-catalog",
];

fn strip_generics(callee: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in callee.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// `host/dbname` from a database URL. None without a database name.
pub fn key_from_url(text: &str) -> Option<String> {
    let (scheme, host) = parse_url(text)?;
    let first = scheme.split(':').next().unwrap_or(&scheme);
    if !(DATABASE_SCHEMES.contains(&first) || DATABASE_SCHEMES.contains(&scheme.as_str())) {
        return None;
    }
    let after_scheme = text.trim().split_once("://")?.1;
    let path = after_scheme
        .split(['?', '#'])
        .next()
        .unwrap_or(after_scheme);
    let dbname = path.split_once('/')?.1.trim_matches('/');
    if dbname.is_empty() {
        return None;
    }
    Some(format!("{host}/{}", dbname.to_lowercase()))
}

pub fn key_from_connection_string(text: &str) -> Option<String> {
    if let Some((host, database)) = ado_connection(text) {
        return Some(format!("{host}/{database}"));
    }
    let caps = PDO_DSN.captures(text.trim())?;
    Some(format!(
        "{}/{}",
        caps[1].to_lowercase(),
        caps[2].to_lowercase()
    ))
}

/// None when `text` carries an unrendered template (`$`, `{`): tree-sitter
/// string literals in Java/C# and unquoted YAML values can hold `${VAR}`
/// verbatim, and a key built from that is junk. Rendered templates (the
/// `Fact::Template` branch) never contain those characters after `render`.
fn key_from_value(text: &str) -> Option<String> {
    if text.contains('$') || text.contains('{') {
        return None;
    }
    key_from_url(text).or_else(|| key_from_connection_string(text))
}

/// A template rendered with each variable's configured value or default.
/// None when a variable has neither.
fn render(ctx: &FileContext<'_>, parts: &[Part]) -> Option<String> {
    let mut out = String::new();
    for part in parts {
        match part {
            Part::Lit(s) => out.push_str(s),
            Part::Var(v) => {
                let (name, default) = env_default(v);
                let value = ctx
                    .config
                    .lookup(ctx.service, &name)
                    .map(|c| c.value.clone())
                    .or(default)?;
                out.push_str(&value);
            }
        }
    }
    Some(out)
}

fn evidence(ctx: &FileContext<'_>, line: u32, detail: String) -> Evidence {
    Evidence {
        file: ctx.file.to_string(),
        line: Some(line),
        detail: Some(detail),
    }
}

/// A database key found in one file: the key itself, where it came from,
/// and whether it is merely a name *looked up* (`GetConnectionString`)
/// rather than *declared* as a resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbKey {
    pub key: String,
    pub evidence: Evidence,
    pub from_lookup: bool,
}

fn push(out: &mut Vec<DbKey>, key: String, evidence: Evidence, from_lookup: bool) {
    if !out.iter().any(|k| k.key == key) {
        out.push(DbKey {
            key,
            evidence,
            from_lookup,
        });
    }
}

/// Every database key one file uses, in fact order, one entry per key.
#[allow(clippy::too_many_lines)]
pub fn keys(ctx: &FileContext<'_>) -> Vec<DbKey> {
    let mut out: Vec<DbKey> = Vec::new();
    let mut host_setting: Option<(String, String)> = None;
    let mut db_setting: Option<(String, String, u32)> = None;
    for fact in ctx.facts {
        match fact {
            Fact::Str { value, line } => {
                if let Some(key) = key_from_value(value) {
                    push(&mut out, key, evidence(ctx, *line, value.clone()), false);
                }
            }
            Fact::Template { parts, line } => {
                if let Some(rendered) = render(ctx, parts) {
                    if let Some(key) = key_from_value(&rendered) {
                        push(&mut out, key, evidence(ctx, *line, rendered), false);
                    }
                }
            }
            Fact::EnvRef {
                name,
                default,
                line,
            } => {
                if let Some(configured) = ctx.config.lookup(ctx.service, name) {
                    if let Some(key) = key_from_value(&configured.value) {
                        let at = match configured.evidence.line {
                            Some(l) => format!("{}:{l}", configured.evidence.file),
                            None => configured.evidence.file.clone(),
                        };
                        push(
                            &mut out,
                            key,
                            evidence(ctx, *line, format!("{name}={} via {at}", configured.value)),
                            false,
                        );
                        continue;
                    }
                }
                if let Some(default) = default {
                    if let Some(key) = key_from_value(default) {
                        push(
                            &mut out,
                            key,
                            evidence(ctx, *line, format!("{name} default {default}")),
                            false,
                        );
                    }
                }
            }
            Fact::Setting { key, value, line } => {
                if let Some(k) = key_from_value(value) {
                    push(
                        &mut out,
                        k,
                        evidence(ctx, *line, format!("{key}={value}")),
                        false,
                    );
                    continue;
                }
                let last = key.rsplit('.').next().unwrap_or(key).to_lowercase();
                if DATABASE_KEYS.contains(&last.as_str())
                    && db_setting.is_none()
                    && !value.contains('$')
                {
                    db_setting = Some((key.clone(), value.clone(), *line));
                } else if is_hostish_key(key) && host_setting.is_none() && is_hostname(value) {
                    host_setting = Some((key.clone(), value.clone()));
                }
            }
            Fact::Call { callee, args, line } => {
                let plain = strip_generics(callee);
                if NAMED_RESOURCE.is_match(&plain) {
                    if let Some(Arg::Str(name)) = args.iter().find(|a| matches!(a, Arg::Str(_))) {
                        let from_lookup = !DECLARED_RESOURCE.is_match(&plain);
                        push(
                            &mut out,
                            name.to_lowercase(),
                            evidence(ctx, *line, format!("{callee}(\"{name}\")")),
                            from_lookup,
                        );
                    }
                    continue;
                }
                for arg in args {
                    let Arg::Other(text) = arg else { continue };
                    if !text.trim_start().starts_with('{') {
                        continue;
                    }
                    if let (Some(h), Some(d)) =
                        (OBJECT_HOST.captures(text), OBJECT_DATABASE.captures(text))
                    {
                        let (host, db) = (h[1].to_lowercase(), d[1].to_lowercase());
                        push(
                            &mut out,
                            format!("{host}/{db}"),
                            evidence(ctx, *line, format!("{callee} host={host} database={db}")),
                            false,
                        );
                    }
                }
            }
            _ => {}
        }
    }
    if let (Some((hk, host)), Some((dk, db, line))) = (host_setting, db_setting) {
        push(
            &mut out,
            format!("{}/{}", host.to_lowercase(), db.to_lowercase()),
            evidence(ctx, line, format!("{hk}={host}, {dk}={db}")),
            false,
        );
    }
    out
}

/// The lowercased resource name of every call a service uses to *declare* a
/// database, data source or client (`Add*DbContext`, `Add*DataSource`,
/// `Add*Client`) across the whole repository. A connection-string name that
/// is only ever *looked up* (`GetConnectionString`) and never declared this
/// way is not in this set.
pub fn declared_resources(extractions: &[(String, String, Extraction)]) -> HashSet<String> {
    let mut declared = HashSet::new();
    for (_, _, ex) in extractions {
        for fact in &ex.facts {
            let Fact::Call { callee, args, .. } = fact else {
                continue;
            };
            let plain = strip_generics(callee);
            if !DECLARED_RESOURCE.is_match(&plain) {
                continue;
            }
            if let Some(Arg::Str(name)) = args.iter().find(|a| matches!(a, Arg::Str(_))) {
                declared.insert(name.to_lowercase());
            }
        }
    }
    declared
}

/// key -> services using it. A key that only ever came from a connection-
/// string lookup (`GetConnectionString`) joins nothing unless some service
/// also declares that same name as a resource.
pub fn database_index(
    extractions: &[(String, String, Extraction)],
    config: &ConfigIndex,
    symbols: &Symbols,
) -> HashMap<String, BTreeSet<String>> {
    let declared = declared_resources(extractions);
    let mut index: HashMap<String, BTreeSet<String>> = HashMap::new();
    for (service, file, ex) in extractions {
        let ctx = FileContext {
            service,
            file,
            facts: &ex.facts,
            config,
            symbols,
        };
        for db_key in keys(&ctx) {
            if db_key.from_lookup && !declared.contains(&db_key.key) {
                continue;
            }
            index.entry(db_key.key).or_default().insert(service.clone());
        }
    }
    index
}

impl Matcher for Database {
    fn name(&self) -> &'static str {
        "database"
    }

    fn candidates(&self, ctx: &FileContext<'_>) -> Vec<Candidate> {
        keys(ctx)
            .into_iter()
            .map(|db_key| Candidate {
                target: Target::Database(db_key.key),
                kind_hint: Some(EdgeType::Database),
                evidence: db_key.evidence,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::config::ConfigIndex;
    use crate::map::facts::{Arg, Parser, Part};
    use crate::map::symbols::Symbols;
    use pretty_assertions::assert_eq;

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn keys_from_urls_and_connection_strings() {
        assert_eq!(
            key_from_url("mongodb://mongodb:27017/catalogue"),
            Some("mongodb/catalogue".into())
        );
        assert_eq!(
            key_from_url("jdbc:mysql://mysql:3306/shop?useSSL=false"),
            Some("mysql/shop".into())
        );
        assert_eq!(
            key_from_url("postgres://app:secret@postgres/shop?sslmode=disable"),
            Some("postgres/shop".into())
        );
        assert_eq!(key_from_url("redis://redis:6379/0"), Some("redis/0".into()));
        assert_eq!(
            key_from_url("mongodb://user:pw@ts-order-mongo:27017/ts-order?authSource=admin"),
            Some("ts-order-mongo/ts-order".into())
        );
        assert_eq!(key_from_url("redis://redis:6379"), None);
        assert_eq!(key_from_url("http://catalogue:8080/products"), None);
        assert_eq!(key_from_url("mongodb://mongodb:27017/"), None);
        assert_eq!(
            key_from_connection_string("Host=localhost;Database=LedgerDB;Username=postgres"),
            Some("localhost/ledgerdb".into())
        );
        assert_eq!(
            key_from_connection_string("mysql:host=mysql;dbname=ratings;charset=utf8mb4"),
            Some("mysql/ratings".into())
        );
        assert_eq!(
            key_from_connection_string("Host=localhost;Username=postgres"),
            None
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn keys_of_a_file_from_every_shape() {
        let mut cfg = ConfigIndex::default();
        cfg.insert_for_test(
            "reports",
            "DB_CONNECTION_STRING",
            "postgres://app:secret@postgres/shop",
            "docker-compose.yml",
            9,
        );
        let symbols = Symbols::default();
        let facts = vec![
            Fact::Str {
                value: "mongodb://mongodb:27017/catalogue".into(),
                line: 1,
            },
            Fact::Template {
                parts: vec![
                    Part::Lit("jdbc:mysql://".into()),
                    Part::Var("DB_HOST:mysql".into()),
                    Part::Lit(":3306/".into()),
                    Part::Var("DB_NAME:shop".into()),
                    Part::Lit("?useSSL=false".into()),
                ],
                line: 2,
            },
            Fact::Template {
                parts: vec![
                    Part::Lit("jdbc:mysql://".into()),
                    Part::Var("UNKNOWN_HOST".into()),
                    Part::Lit(":3306/x".into()),
                ],
                line: 3,
            },
            Fact::EnvRef {
                name: "DB_CONNECTION_STRING".into(),
                default: None,
                line: 4,
            },
            Fact::EnvRef {
                name: "LEGACY_DB_URL".into(),
                default: Some("mysql://mysql:3306/legacy".into()),
                line: 5,
            },
            Fact::Setting {
                key: "spring.data.mongodb.host".into(),
                value: "mongodb".into(),
                line: 6,
            },
            Fact::Setting {
                key: "spring.data.mongodb.database".into(),
                value: "inventory".into(),
                line: 7,
            },
            Fact::Call {
                callee: "builder.AddNpgsqlDbContext<LedgerContext>".into(),
                args: vec![Arg::Str("ledgerdb".into())],
                line: 8,
            },
            Fact::Call {
                callee: "builder.Configuration.GetConnectionString".into(),
                args: vec![Arg::Str("OrderingDb".into())],
                line: 9,
            },
            Fact::Call {
                callee: "builder.AddRedisClient".into(),
                args: vec![Arg::Str("redis".into())],
                line: 10,
            },
            Fact::Call {
                callee: "mysql.createConnection".into(),
                args: vec![Arg::Other(
                    "{ host: 'mysql', user: 'x', database: 'cities' }".into(),
                )],
                line: 11,
            },
            Fact::Str {
                value: "Host=localhost;Database=LedgerDB;Username=postgres".into(),
                line: 12,
            },
        ];
        let ctx = FileContext {
            service: "reports",
            file: "reports/x",
            facts: &facts,
            config: &cfg,
            symbols: &symbols,
        };
        let got: Vec<(String, u32, String)> = keys(&ctx)
            .into_iter()
            .map(|k| (k.key, k.evidence.line.unwrap(), k.evidence.detail.unwrap()))
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    "mongodb/catalogue".into(),
                    1,
                    "mongodb://mongodb:27017/catalogue".into()
                ),
                (
                    "mysql/shop".into(),
                    2,
                    "jdbc:mysql://mysql:3306/shop?useSSL=false".into()
                ),
                (
                    "postgres/shop".into(),
                    4,
                    "DB_CONNECTION_STRING=postgres://app:secret@postgres/shop via docker-compose.yml:9"
                        .into()
                ),
                (
                    "mysql/legacy".into(),
                    5,
                    "LEGACY_DB_URL default mysql://mysql:3306/legacy".into()
                ),
                (
                    "ledgerdb".into(),
                    8,
                    "builder.AddNpgsqlDbContext<LedgerContext>(\"ledgerdb\")".into()
                ),
                (
                    "orderingdb".into(),
                    9,
                    "builder.Configuration.GetConnectionString(\"OrderingDb\")".into()
                ),
                (
                    "mysql/cities".into(),
                    11,
                    "mysql.createConnection host=mysql database=cities".into()
                ),
                (
                    "localhost/ledgerdb".into(),
                    12,
                    "Host=localhost;Database=LedgerDB;Username=postgres".into()
                ),
                (
                    "mongodb/inventory".into(),
                    7,
                    "spring.data.mongodb.host=mongodb, spring.data.mongodb.database=inventory".into()
                ),
            ]
        );
    }

    #[test]
    fn lookup_names_need_a_declaration() {
        let cfg = ConfigIndex::default();
        let symbols = Symbols::default();
        let lookup = |service: &str| -> (String, String, Extraction) {
            (
                service.into(),
                format!("{service}/Program.cs"),
                Extraction {
                    facts: vec![Fact::Call {
                        callee: "builder.Configuration.GetConnectionString".into(),
                        args: vec![Arg::Str("DefaultConnection".into())],
                        line: 1,
                    }],
                    parser: Parser::Regex,
                    language: "x".into(),
                },
            )
        };
        let mut extractions = vec![lookup("alpha"), lookup("beta")];
        let index = database_index(&extractions, &cfg, &symbols);
        assert_eq!(
            index.get("defaultconnection"),
            None,
            "two lookups of the ASP.NET template default do not share a database"
        );

        extractions.push((
            "gamma".into(),
            "gamma/Program.cs".into(),
            Extraction {
                facts: vec![Fact::Call {
                    callee: "builder.AddNpgsqlDataSource".into(),
                    args: vec![Arg::Str("DefaultConnection".into())],
                    line: 1,
                }],
                parser: Parser::Regex,
                language: "x".into(),
            },
        ));
        let index = database_index(&extractions, &cfg, &symbols);
        assert_eq!(
            index.get("defaultconnection"),
            Some(&set(&["alpha", "beta", "gamma"])),
            "gamma's declaration makes the name a real shared key"
        );
    }
}
