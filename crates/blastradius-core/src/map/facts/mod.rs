//! One file in, language-neutral facts out.
//!
//! Tree-sitter handles seven languages. Everything else, including
//! configuration files such as nginx templates and Spring properties, goes
//! through a regex extractor that produces the same facts. Nothing below
//! this module knows what language a fact came from.
mod regex;
mod treesitter;

use serde::{Deserialize, Serialize};

/// A piece of an interpolated or concatenated string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    Lit(String),
    /// The substitution as written, braces removed: `host`, `CATALOGUE_HOST`,
    /// `DB_HOST:mysql`.
    Var(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg {
    Str(String),
    Template(Vec<Part>),
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fact {
    Str {
        value: String,
        line: u32,
    },
    Template {
        parts: Vec<Part>,
        line: u32,
    },
    /// `callee` as written, arguments dropped: `axios.get`,
    /// `pb.NewCartServiceClient`, `new Basket.BasketClient`.
    Call {
        callee: String,
        args: Vec<Arg>,
        line: u32,
    },
    Import {
        path: String,
        line: u32,
    },
    Annotation {
        name: String,
        args: Vec<Arg>,
        line: u32,
    },
    /// A base class or interface: Java superclass, C# base list.
    Extends {
        name: String,
        line: u32,
    },
    /// An environment lookup, with the literal default when one is written
    /// beside it.
    EnvRef {
        name: String,
        default: Option<String>,
        line: u32,
    },
}

impl Fact {
    pub fn line(&self) -> u32 {
        match self {
            Self::Str { line, .. }
            | Self::Template { line, .. }
            | Self::Call { line, .. }
            | Self::Import { line, .. }
            | Self::Annotation { line, .. }
            | Self::Extends { line, .. }
            | Self::EnvRef { line, .. } => *line,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Parser {
    TreeSitter,
    Regex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    JavaScript,
    TypeScript,
    Tsx,
    Python,
    Go,
    Java,
    CSharp,
    Php,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extraction {
    pub facts: Vec<Fact>,
    pub parser: Parser,
    /// Label for `mapping.parsers`: `go`, `csharp`, `template`, `dockerfile`.
    pub language: String,
}

const SKIP_SUFFIXES: &[&str] = &[
    ".min.js",
    ".min.css",
    ".map",
    ".lock",
    "-lock.json",
    "-lock.yaml",
    ".sum",
    ".svg",
    ".png",
    ".jpg",
    ".jpeg",
    ".gif",
    ".ico",
    ".webp",
    ".bmp",
    ".woff",
    ".woff2",
    ".ttf",
    ".eot",
    ".otf",
    ".pdf",
    ".zip",
    ".gz",
    ".tgz",
    ".tar",
    ".jar",
    ".war",
    ".class",
    ".dll",
    ".exe",
    ".so",
    ".dylib",
    ".wasm",
    ".pyc",
    ".csv",
    ".snap",
    ".md",
    ".markdown",
    ".txt",
    ".rst",
    ".log",
    ".mp3",
    ".mp4",
    ".mov",
    ".avi",
    ".pb",
    ".bin",
    ".dat",
    ".db",
    ".sqlite",
    ".ipynb",
];

const SKIP_NAMES: &[&str] = &[
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "cargo.lock",
    "go.sum",
    "composer.lock",
    "gemfile.lock",
    "poetry.lock",
    "mix.lock",
    "license",
    "licence",
    "notice",
    "changelog",
];

/// Which parser a file gets, or None when it is not worth reading.
pub fn language_for(path: &str) -> Option<Language> {
    let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    if SKIP_NAMES.contains(&name.as_str()) || SKIP_SUFFIXES.iter().any(|s| name.ends_with(s)) {
        return None;
    }
    let ext = name.rsplit_once('.').map_or("", |(_, e)| e);
    Some(match ext {
        "js" | "mjs" | "cjs" | "jsx" => Language::JavaScript,
        "ts" | "mts" | "cts" => Language::TypeScript,
        "tsx" => Language::Tsx,
        "py" => Language::Python,
        "go" => Language::Go,
        "java" => Language::Java,
        "cs" => Language::CSharp,
        "php" => Language::Php,
        _ => Language::Other,
    })
}

const CONFIG_EXTENSIONS: &[&str] = &[
    "conf",
    "template",
    "tmpl",
    "properties",
    "ini",
    "cfg",
    "toml",
    "json",
    "yaml",
    "yml",
    "xml",
    "sh",
    "bash",
    "env",
    "hcl",
    "tf",
    "nomad",
];

/// Configuration and deployment files carry real edges: an nginx template's
/// `proxy_pass`, a Spring datasource URL, a Dockerfile's `ENV`.
pub fn is_config_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    if name.starts_with(".env") || name == "dockerfile" || name.ends_with(".dockerfile") {
        return true;
    }
    let ext = name.rsplit_once('.').map_or("", |(_, e)| e);
    CONFIG_EXTENSIONS.contains(&ext)
}

fn label_for(language: Language) -> &'static str {
    match language {
        Language::JavaScript => "javascript",
        Language::TypeScript => "typescript",
        Language::Tsx => "tsx",
        Language::Python => "python",
        Language::Go => "go",
        Language::Java => "java",
        Language::CSharp => "csharp",
        Language::Php => "php",
        Language::Other => "",
    }
}

/// Facts for one file. None when the file is skipped.
pub fn extract(path: &str, text: &str) -> Option<Extraction> {
    let language = language_for(path)?;
    if language != Language::Other {
        if let Some(facts) = treesitter::extract(language, text) {
            return Some(Extraction {
                facts,
                parser: Parser::TreeSitter,
                language: label_for(language).to_string(),
            });
        }
    }
    let name = path.rsplit('/').next().unwrap_or(path).to_lowercase();
    let label = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_string(),
        _ => name.trim_start_matches('.').to_string(),
    };
    Some(Extraction {
        facts: regex::extract(text),
        parser: Parser::Regex,
        language: label,
    })
}

/// Is this callee an environment lookup? Broad on purpose: `os.getenv`,
/// `os.environ.get`, `System.getenv().getOrDefault`, `Deno.env.get`,
/// `Environment.GetEnvironmentVariable`, `System.get_env`, `env::var`.
pub(crate) fn env_callee(callee: &str) -> bool {
    let c = callee.to_lowercase();
    c.contains("getenv")
        || c.contains("environ")
        || c.contains("environmentvariable")
        || c.contains("get_env")
        || c.contains("env::var")
        || c == "env.fetch"
        || c.ends_with("env.get")
        || c.ends_with(".lookupenv")
}

/// Strips one layer of quotes and any string prefix (`f`, `r`, `b`, `u`,
/// `@`, `$`). None when the text is not a quoted string.
pub(crate) fn unquote(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let start = trimmed.find(['"', '\'', '`']).filter(|&i| {
        i <= 3
            && trimmed[..i]
                .chars()
                .all(|c| c.is_ascii_alphabetic() || c == '@' || c == '$')
    })?;
    let body = &trimmed[start..];
    let quote = body.chars().next()?;
    let triple: String = std::iter::repeat_n(quote, 3).collect();
    if quote != '`' && body.len() >= 6 && body.starts_with(&triple) && body.ends_with(&triple) {
        return Some(body[3..body.len() - 3].to_string());
    }
    if body.len() >= 2 && body.ends_with(quote) {
        return Some(body[1..body.len() - 1].to_string());
    }
    None
}

/// Splits `${VAR}`, `{expr}`, `#{expr}` and `$name` substitutions out of a
/// string. Adjacent literal pieces are merged.
pub(crate) fn split_template(text: &str) -> Vec<Part> {
    let mut parts: Vec<Part> = Vec::new();
    let mut lit = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let push_lit = |parts: &mut Vec<Part>, lit: &mut String| {
        if !lit.is_empty() {
            parts.push(Part::Lit(std::mem::take(lit)));
        }
    };
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let braced = ((c == '$' || c == '#') && next == Some('{'))
            || (c == '{' && next.is_some_and(|n| n.is_ascii_alphabetic() || n == '_'));
        if braced {
            let open = if c == '{' { i } else { i + 1 };
            if let Some(close) = (open + 1..chars.len()).find(|&j| chars[j] == '}') {
                let inner: String = chars[open + 1..close].iter().collect();
                if !inner.is_empty() && !inner.contains('\n') {
                    push_lit(&mut parts, &mut lit);
                    parts.push(Part::Var(inner.trim().to_string()));
                    i = close + 1;
                    continue;
                }
            }
        }
        if c == '$' && next.is_some_and(|n| n.is_ascii_alphabetic() || n == '_') {
            let end = (i + 1..chars.len())
                .find(|&j| !(chars[j].is_ascii_alphanumeric() || chars[j] == '_'))
                .unwrap_or(chars.len());
            push_lit(&mut parts, &mut lit);
            parts.push(Part::Var(chars[i + 1..end].iter().collect()));
            i = end;
            continue;
        }
        lit.push(c);
        i += 1;
    }
    push_lit(&mut parts, &mut lit);
    parts
}

/// `DB_HOST:mysql` -> (`DB_HOST`, Some(`mysql`)); `X:-d` -> (`X`, Some(`d`));
/// `X` -> (`X`, None). Anything that is not a plain variable name stays as
/// the name with no default.
pub(crate) fn env_default(var: &str) -> (String, Option<String>) {
    let var = var.trim();
    match var.find([':', '-', '?', '=']) {
        Some(i) if i > 0 && is_var_name(&var[..i]) => {
            let name = var[..i].to_string();
            let rest = var[i..].trim_start_matches([':', '-', '?', '=']).trim();
            let default = (!rest.is_empty()).then(|| rest.to_string());
            (name, default)
        }
        _ => (var.to_string(), None),
    }
}

pub(crate) fn is_var_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn strs(facts: &[Fact]) -> Vec<&str> {
        facts
            .iter()
            .filter_map(|f| match f {
                Fact::Str { value, .. } => Some(value.as_str()),
                _ => None,
            })
            .collect()
    }
    fn calls(facts: &[Fact]) -> Vec<&str> {
        facts
            .iter()
            .filter_map(|f| match f {
                Fact::Call { callee, .. } => Some(callee.as_str()),
                _ => None,
            })
            .collect()
    }
    fn envs(facts: &[Fact]) -> Vec<(&str, Option<&str>)> {
        facts
            .iter()
            .filter_map(|f| match f {
                Fact::EnvRef { name, default, .. } => Some((name.as_str(), default.as_deref())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn javascript_strings_calls_env_and_templates() {
        let src = "const host = process.env.CATALOGUE_HOST || 'catalogue';\nfetch(`http://${host}:8080/items`);\naxios.get(\"http://user:8080/check/\" + id);\nconst got = require(\"got\");\n";
        let ex = extract("cart/server.js", src).unwrap();
        assert_eq!(ex.parser, Parser::TreeSitter);
        assert!(strs(&ex.facts).contains(&"catalogue"));
        assert!(strs(&ex.facts).contains(&"http://user:8080/check/"));
        assert!(calls(&ex.facts).contains(&"fetch") && calls(&ex.facts).contains(&"axios.get"));
        assert_eq!(envs(&ex.facts), vec![("CATALOGUE_HOST", Some("catalogue"))]);
        assert!(ex.facts.iter().any(|f| matches!(f, Fact::Template { parts, line: 2 } if parts == &[Part::Lit("http://".into()), Part::Var("host".into()), Part::Lit(":8080/items".into())])), "{:?}", ex.facts);
        assert!(
            ex.facts
                .iter()
                .any(|f| matches!(f, Fact::Import { path, .. } if path == "got"))
        );
        let Fact::Call { args, .. } = ex
            .facts
            .iter()
            .find(|f| matches!(f, Fact::Call { callee, .. } if callee == "axios.get"))
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(
            args[0],
            Arg::Template(vec![
                Part::Lit("http://user:8080/check/".into()),
                Part::Var("id".into())
            ])
        );
    }

    #[test]
    fn python_env_default_decorator_and_grpc_calls() {
        let src = "import demo_pb2_grpc\nurl = os.getenv('USER_HOST', 'user')\nstub = demo_pb2_grpc.RecommendationServiceStub(channel)\n@app.route('/pay')\ndef pay(): pass\ndemo_pb2_grpc.add_EmailServiceServicer_to_server(EmailService(), server)\ns = f\"http://{host}:8080\"\n";
        let ex = extract("payment/payment.py", src).unwrap();
        assert_eq!(envs(&ex.facts), vec![("USER_HOST", Some("user"))]);
        assert!(calls(&ex.facts).contains(&"demo_pb2_grpc.RecommendationServiceStub"));
        assert!(calls(&ex.facts).contains(&"demo_pb2_grpc.add_EmailServiceServicer_to_server"));
        assert!(
            ex.facts
                .iter()
                .any(|f| matches!(f, Fact::Annotation { name, .. } if name == "app.route")),
            "{:?}",
            ex.facts
        );
        assert!(
            ex.facts
                .iter()
                .any(|f| matches!(f, Fact::Import { path, .. } if path == "demo_pb2_grpc"))
        );
        assert!(ex.facts.iter().any(|f| matches!(f, Fact::Template { parts, .. } if parts.first() == Some(&Part::Lit("http://".into())) && parts.get(1) == Some(&Part::Var("host".into())))), "{:?}", ex.facts);
    }

    #[test]
    fn go_java_csharp_php_essentials() {
        let go = extract("a/main.go", "package main\nimport pb \"github.com/acme/genproto\"\nfunc main() {\n\taddr := os.Getenv(\"PRODUCT_CATALOG_SERVICE_ADDR\")\n\tc := pb.NewCartServiceClient(conn)\n\tpb.RegisterShippingServiceServer(srv, svc)\n\thttp.Get(\"http://catalogue:8080/products\")\n}\n").unwrap();
        assert_eq!(
            envs(&go.facts),
            vec![("PRODUCT_CATALOG_SERVICE_ADDR", None)]
        );
        assert!(
            calls(&go.facts).contains(&"pb.NewCartServiceClient")
                && calls(&go.facts).contains(&"pb.RegisterShippingServiceServer")
        );
        assert!(
            go.facts.iter().any(
                |f| matches!(f, Fact::Import { path, .. } if path == "github.com/acme/genproto")
            ),
            "{:?}",
            go.facts
        );

        let java = extract("a/Foo.java", "import org.x.RestTemplate;\n@FeignClient(name = \"customers-service\")\npublic class Foo extends AdServiceGrpc.AdServiceImplBase {\n  String url = \"http://ts-order-service:12031/api\";\n  void go() { restTemplate.getForObject(url + \"/x\", String.class); String h = System.getenv(\"DB_HOST\"); AdServiceGrpc.newBlockingStub(channel); }\n}\n").unwrap();
        assert!(java.facts.iter().any(|f| matches!(f, Fact::Annotation { name, args, .. } if name == "FeignClient" && args.contains(&Arg::Str("customers-service".into())))), "{:?}", java.facts);
        assert!(java.facts.iter().any(|f| matches!(f, Fact::Extends { name, .. } if name == "AdServiceGrpc.AdServiceImplBase")), "{:?}", java.facts);
        assert_eq!(envs(&java.facts), vec![("DB_HOST", None)]);
        assert!(
            calls(&java.facts).contains(&"restTemplate.getForObject")
                && calls(&java.facts).contains(&"AdServiceGrpc.newBlockingStub")
        );
        assert!(
            java.facts
                .iter()
                .any(|f| matches!(f, Fact::Import { path, .. } if path == "org.x.RestTemplate")),
            "{:?}",
            java.facts
        );

        let cs = extract("a/Program.cs", "public class CartServiceImpl : CartService.CartServiceBase {\n void Go() {\n  builder.Services.AddHttpClient<CatalogService>(o => o.BaseAddress = new(\"https+http://catalog-api\"));\n  var a = Environment.GetEnvironmentVariable(\"REDIS_ADDR\");\n  var s = $\"http://{host}:8080\";\n  var c = new Basket.BasketClient(channel);\n }\n}\n").unwrap();
        assert!(
            cs.facts.iter().any(
                |f| matches!(f, Fact::Extends { name, .. } if name == "CartService.CartServiceBase")
            ),
            "{:?}",
            cs.facts
        );
        assert!(strs(&cs.facts).contains(&"https+http://catalog-api"));
        assert_eq!(envs(&cs.facts), vec![("REDIS_ADDR", None)]);
        assert!(
            calls(&cs.facts).contains(&"new Basket.BasketClient"),
            "{:?}",
            calls(&cs.facts)
        );
        assert!(
            calls(&cs.facts)
                .iter()
                .any(|c| c.starts_with("builder.Services.AddHttpClient"))
        );
        assert!(cs.facts.iter().any(|f| matches!(f, Fact::Template { parts, .. } if parts.first() == Some(&Part::Lit("http://".into())) && parts.get(1) == Some(&Part::Var("host".into())))), "{:?}", cs.facts);

        let php = extract("a/index.php", "<?php\n$url = getenv('CATALOGUE_URL') ?: 'http://catalogue:8080';\n$d = file_get_contents(\"http://catalogue:8080/product/$sku\");\n$c = new Client(['base_uri' => 'http://user:8080']);\n$c->get('/x');\n").unwrap();
        assert_eq!(envs(&php.facts), vec![("CATALOGUE_URL", None)]);
        assert!(
            strs(&php.facts).contains(&"http://catalogue:8080")
                && strs(&php.facts).contains(&"http://user:8080")
        );
        assert!(
            calls(&php.facts).contains(&"file_get_contents")
                && calls(&php.facts).contains(&"new Client")
                && calls(&php.facts).contains(&"$c->get"),
            "{:?}",
            calls(&php.facts)
        );
        assert!(php.facts.iter().any(|f| matches!(f, Fact::Template { parts, .. } if parts.first() == Some(&Part::Lit("http://catalogue:8080/product/".into())))), "{:?}", php.facts);
    }

    #[test]
    fn regex_extractor_covers_config_and_unknown_languages() {
        let nginx = extract(
            "web/default.conf.template",
            "location /api/catalogue/ {\n    proxy_pass http://${CATALOGUE_HOST}:8080/;\n}\n",
        )
        .unwrap();
        assert_eq!(nginx.parser, Parser::Regex);
        assert_eq!(nginx.language, "template");
        assert_eq!(envs(&nginx.facts), vec![("CATALOGUE_HOST", None)]);
        assert!(nginx.facts.iter().any(|f| matches!(f, Fact::Template { parts, line: 2 } if parts == &[Part::Lit("http://".into()), Part::Var("CATALOGUE_HOST".into()), Part::Lit(":8080/".into())])), "{:?}", nginx.facts);

        let props = extract(
            "shipping/src/main/resources/application.properties",
            "spring.datasource.url=jdbc:mysql://${DB_HOST:mysql}:3306/cities\n",
        )
        .unwrap();
        assert_eq!(envs(&props.facts), vec![("DB_HOST", Some("mysql"))]);
        assert!(props.facts.iter().any(|f| matches!(f, Fact::Template { parts, .. } if parts.first() == Some(&Part::Lit("jdbc:mysql://".into())))), "{:?}", props.facts);

        let elixir = extract("x/lib/app.ex", "url = System.get_env(\"FLAGD_HOST\") || \"flagd\"\nHTTPoison.get(\"http://product-catalog:8080/products\")\n").unwrap();
        assert_eq!(elixir.parser, Parser::Regex);
        assert_eq!(elixir.language, "ex");
        assert_eq!(envs(&elixir.facts), vec![("FLAGD_HOST", Some("flagd"))]);
        assert!(
            strs(&elixir.facts).contains(&"http://product-catalog:8080/products")
                && strs(&elixir.facts).contains(&"flagd")
        );
        assert!(calls(&elixir.facts).contains(&"HTTPoison.get"));

        let ruby = extract(
            "x/app.rb",
            "host = ENV['REDIS_HOST'] || 'redis'\nENV.fetch(\"CART_URL\", \"http://cart:8080\")\n",
        )
        .unwrap();
        assert_eq!(
            envs(&ruby.facts),
            vec![
                ("REDIS_HOST", Some("redis")),
                ("CART_URL", Some("http://cart:8080"))
            ]
        );
    }

    #[test]
    fn skips_what_should_not_be_read() {
        assert_eq!(language_for("a/b.min.js"), None);
        assert_eq!(language_for("a/package-lock.json"), None);
        assert_eq!(language_for("a/pnpm-lock.yaml"), None);
        assert_eq!(language_for("a/logo.png"), None);
        assert_eq!(language_for("a/font.woff2"), None);
        assert_eq!(language_for("a/README.md"), None);
        assert_eq!(language_for("a/Dockerfile"), Some(Language::Other));
        assert_eq!(language_for("a/x.tsx"), Some(Language::Tsx));
        assert_eq!(language_for("a/x.cs"), Some(Language::CSharp));
        assert!(
            is_config_file("a/nginx.conf")
                && is_config_file("a/.env.example")
                && !is_config_file("a/main.go")
        );
        assert!(extract("a/b.min.js", "x").is_none());
    }
}
