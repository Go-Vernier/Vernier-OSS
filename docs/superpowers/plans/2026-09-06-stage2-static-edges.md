# Stage 2: Static Edges (Facts, Resolver, HTTP, gRPC) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `blast-radius analyze` reports HTTP and gRPC edges between discovered services, each with file-and-line evidence and a confidence label, validated against the documented call graphs of microservices-demo, robot-shop and eShop.

**Architecture:** Three layers under `crates/blastradius-core/src/map/`. `facts` turns one file into language-neutral facts (strings, templates, calls, imports, annotations, base types, environment references) through tree-sitter for seven languages and a regex extractor for everything else including config files. `matchers` turn facts into candidates that name what they found (a host, an environment variable, a proto service). `resolve` turns candidates into edges against an index of discovered services and their configured environment. `map::run` owns files by service root, fans out over rayon, merges duplicate edges and fills the `mapping` block of the JSON.

**Tech Stack:** tree-sitter 0.27 with tree-sitter-javascript 0.25, -typescript 0.23, -python 0.25, -go 0.25, -java 0.23, -c-sharp 0.23, -php 0.24; rayon 1; regex; existing yaml and fs modules.

**Spec:** `docs/superpowers/specs/2026-09-06-rust-engine-stage2-design.md`, sections "JSON contract" and "Stage 2 architecture". This plan covers the facts layer, the resolver, the http and grpc matchers, the mapping block and the report additions. Event, database and import matchers are the next plan; the resolver here already types an edge by its target so datastore and broker hosts found through environment variables come out as `database` and `event` edges.

## Global Constraints

- Everything in Plan A's Global Constraints still holds (toolchain path, fmt, clippy `-D warnings`, camelCase JSON, no network).
- Facts are language-neutral: no module under `map/` except `facts/` may name a language.
- Every edge carries at least one `Evidence` with a file and a line. Evidence `detail` says what was matched, for example `http://catalogue:8080/products`, `CATALOGUE_HOST=catalogue via docker-compose.yaml:61`, `pb.NewCartServiceClient`.
- Confidence table from the spec: literal host or resolved variable Static; unresolved variable matched by name, or template fragment, Uncertain; proto ownership Static.
- Recall over precision: never drop a candidate that resolves to a discovered service. Self-edges are dropped. Candidates that resolve to nothing are counted in `mapping.unresolved` and listed in `mapping.unresolvedTargets` (distinct, sorted, at most 50).
- Edge type follows the target when the candidate came from a host or a variable: a target whose image or name is a datastore (`redis`, `valkey`, `memcached`, `mongo`, `mongodb`, `mysql`, `mariadb`, `postgres`, `postgresql`, `cockroach`, `cassandra`, `elasticsearch`, `opensearch`, `influxdb`, `clickhouse`, `dynamodb`, `sqlserver`, `mssql`, `oracle`, `neo4j`, `couchbase`) gives `database`; a broker (`rabbitmq`, `kafka`, `redpanda`, `nats`, `activemq`, `pulsar`, `mosquitto`, `localstack`, `sqs`, `sns`, `eventbus`) gives `event`; a URL scheme decides first when present (`amqp`, `kafka`, `nats`, `mqtt` are event; `mongodb`, `postgres`, `postgresql`, `mysql`, `mariadb`, `redis`, `rediss`, `jdbc:*`, `memcached` are database); otherwise `http`, except a scheme-less `host:port` whose target owns a proto service, which is `grpc`.
- File ownership is the longest discovered `root` that prefixes the path. Files under no root are counted in `mapping.filesOutsideServices` and not scanned. Files over 1 MiB, `*.min.js`, lock files, images, fonts, archives and binaries are skipped and counted under `mapping.filesSkipped` by extension.
- Performance: whole analysis under 500 ms on every corpus repository, asserted by the corpus test.

## File Structure

```
crates/blastradius-core/Cargo.toml              + tree-sitter*, rayon
crates/blastradius-core/src/map/mod.rs          run(): ownership, fan-out, merge, MappingStats; pub types Candidate, Target
crates/blastradius-core/src/map/facts/mod.rs    Fact, Part, Arg, Language detection by extension, extract(path, text) -> Extraction
crates/blastradius-core/src/map/facts/treesitter.rs  per-language node tables and the tree walk
crates/blastradius-core/src/map/facts/regex.rs  strings, ${VAR}, env lookups, imports for unknown languages and config files
crates/blastradius-core/src/map/config.rs       ConfigIndex: compose/k8s env per service, ConfigMaps, dotenv, proto services
crates/blastradius-core/src/map/resolve.rs      Resolver: Target -> Option<Resolved>, host classification, normalisation
crates/blastradius-core/src/map/matchers/mod.rs Matcher trait, all()
crates/blastradius-core/src/map/matchers/http.rs
crates/blastradius-core/src/map/matchers/grpc.rs
crates/blastradius-core/src/analyze.rs          + mapping field, calls map::run
crates/blastradius-core/src/report.rs           STRUCTURE with counts, EDGES table, FINDINGS
crates/blastradius-core/tests/edges.rs          fixture tests for http, grpc, resolver confidence
crates/blastradius-core/tests/corpus.rs         + expected edges, minimum recall, unresolved report
test/fixtures/edges-http-app/                   compose + env + 4 services in js, py, go, php + nginx template
test/fixtures/edges-grpc-app/                   k8s manifests + proto + go server, py client, cs client
test/expected/corpus/{microservices-demo,robot-shop,eshop}.json  + "edges": [...]
```

---

### Task B1: Facts layer with tree-sitter and regex extractors

**Files:**
- Modify: `crates/blastradius-core/Cargo.toml`, `Cargo.toml` (workspace deps), `crates/blastradius-core/src/lib.rs` (add `pub mod map;`)
- Create: `src/map/mod.rs` (module declarations only for now), `src/map/facts/mod.rs`, `src/map/facts/treesitter.rs`, `src/map/facts/regex.rs`

**Interfaces:**
```rust
// facts/mod.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part { Lit(String), Var(String) }               // Var holds the substitution text: `host`, `CATALOGUE_HOST`, `DB_HOST:mysql`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg { Str(String), Template(Vec<Part>), Other(String) }
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fact {
    Str { value: String, line: u32 },
    Template { parts: Vec<Part>, line: u32 },
    Call { callee: String, args: Vec<Arg>, line: u32 },   // callee as written: `axios.get`, `pb.NewCartServiceClient`, `new Basket.BasketClient`
    Import { path: String, line: u32 },
    Annotation { name: String, args: Vec<Arg>, line: u32 },
    Extends { name: String, line: u32 },                   // Java superclass, C# base_list entries
    EnvRef { name: String, default: Option<String>, line: u32 },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Parser { TreeSitter, Regex }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language { JavaScript, TypeScript, Tsx, Python, Go, Java, CSharp, Php, Other }
pub struct Extraction { pub facts: Vec<Fact>, pub parser: Parser, pub language: &'static str }  // language label for mapping.parsers, e.g. "go", "conf"
pub fn language_for(path: &str) -> Option<Language>;      // None = do not scan (binary, lock, minified, image...)
pub fn extract(path: &str, text: &str) -> Option<Extraction>;
pub fn is_config_file(path: &str) -> bool;                 // .conf .template .properties .ini .cfg .toml .env* .json .yaml .yml .xml .sh .Dockerfile
```
- `EnvRef` is emitted by both extractors when a call's callee is an env lookup (`process.env.X` member access, `os.getenv`, `os.environ.get`, `os.environ[...]`, `os.Getenv`, `os.LookupEnv`, `System.getenv`, `Environment.GetEnvironmentVariable`, `getenv`, `ENV[...]`, `ENV.fetch`, `System.get_env`, `std::env::var`, `env::var`) with a string first argument; `default` is the second string argument when present. Config-file `${VAR}`, `${VAR:-d}`, `${VAR:d}`, `$VAR` references produce `EnvRef` too (default from the `:-`/`:` form) and also appear inside `Template` parts as `Var`.

- [ ] **Step 1: Write the failing unit tests** in `facts/mod.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn strs(facts: &[Fact]) -> Vec<&str> { facts.iter().filter_map(|f| match f { Fact::Str { value, .. } => Some(value.as_str()), _ => None }).collect() }
    fn calls(facts: &[Fact]) -> Vec<&str> { facts.iter().filter_map(|f| match f { Fact::Call { callee, .. } => Some(callee.as_str()), _ => None }).collect() }
    fn envs(facts: &[Fact]) -> Vec<(&str, Option<&str>)> { facts.iter().filter_map(|f| match f { Fact::EnvRef { name, default, .. } => Some((name.as_str(), default.as_deref())), _ => None }).collect() }

    #[test]
    fn javascript_strings_calls_env_and_templates() {
        let src = "const host = process.env.CATALOGUE_HOST || 'catalogue';\nfetch(`http://${host}:8080/items`);\naxios.get(\"http://user:8080/check/\" + id);\nconst got = require(\"got\");\n";
        let ex = extract("cart/server.js", src).unwrap();
        assert_eq!(ex.parser, Parser::TreeSitter);
        assert!(strs(&ex.facts).contains(&"catalogue"));
        assert!(strs(&ex.facts).contains(&"http://user:8080/check/"));
        assert!(calls(&ex.facts).contains(&"fetch") && calls(&ex.facts).contains(&"axios.get"));
        assert_eq!(envs(&ex.facts), vec![("CATALOGUE_HOST", None)]);
        assert!(ex.facts.iter().any(|f| matches!(f, Fact::Template { parts, line: 2 } if parts == &[Part::Lit("http://".into()), Part::Var("host".into()), Part::Lit(":8080/items".into())])));
        assert!(ex.facts.iter().any(|f| matches!(f, Fact::Import { path, .. } if path == "got")));
        let Fact::Call { args, .. } = ex.facts.iter().find(|f| matches!(f, Fact::Call { callee, .. } if callee == "axios.get")).unwrap() else { unreachable!() };
        assert_eq!(args[0], Arg::Str("http://user:8080/check/".into()));
    }

    #[test]
    fn python_env_default_decorator_and_grpc_calls() {
        let src = "import demo_pb2_grpc\nurl = os.getenv('USER_HOST', 'user')\nstub = demo_pb2_grpc.RecommendationServiceStub(channel)\n@app.route('/pay')\ndef pay(): pass\ndemo_pb2_grpc.add_EmailServiceServicer_to_server(EmailService(), server)\ns = f\"http://{host}:8080\"\n";
        let ex = extract("payment/payment.py", src).unwrap();
        assert_eq!(envs(&ex.facts), vec![("USER_HOST", Some("user"))]);
        assert!(calls(&ex.facts).contains(&"demo_pb2_grpc.RecommendationServiceStub"));
        assert!(calls(&ex.facts).contains(&"demo_pb2_grpc.add_EmailServiceServicer_to_server"));
        assert!(ex.facts.iter().any(|f| matches!(f, Fact::Annotation { name, .. } if name == "app.route")));
        assert!(ex.facts.iter().any(|f| matches!(f, Fact::Import { path, .. } if path == "demo_pb2_grpc")));
        assert!(ex.facts.iter().any(|f| matches!(f, Fact::Template { parts, .. } if parts.first() == Some(&Part::Lit("http://".into())) && parts.get(1) == Some(&Part::Var("host".into())))));
    }

    #[test]
    fn go_java_csharp_php_essentials() {
        let go = extract("a/main.go", "package main\nimport pb \"github.com/acme/genproto\"\nfunc main() {\n\taddr := os.Getenv(\"PRODUCT_CATALOG_SERVICE_ADDR\")\n\tc := pb.NewCartServiceClient(conn)\n\tpb.RegisterShippingServiceServer(srv, svc)\n\thttp.Get(\"http://catalogue:8080/products\")\n}\n").unwrap();
        assert_eq!(envs(&go.facts), vec![("PRODUCT_CATALOG_SERVICE_ADDR", None)]);
        assert!(calls(&go.facts).contains(&"pb.NewCartServiceClient") && calls(&go.facts).contains(&"pb.RegisterShippingServiceServer"));
        assert!(go.facts.iter().any(|f| matches!(f, Fact::Import { path, .. } if path == "github.com/acme/genproto")));

        let java = extract("a/Foo.java", "import org.x.RestTemplate;\n@FeignClient(name = \"customers-service\")\npublic class Foo extends AdServiceGrpc.AdServiceImplBase {\n  String url = \"http://ts-order-service:12031/api\";\n  void go() { restTemplate.getForObject(url + \"/x\", String.class); String h = System.getenv(\"DB_HOST\"); AdServiceGrpc.newBlockingStub(channel); }\n}\n").unwrap();
        assert!(java.facts.iter().any(|f| matches!(f, Fact::Annotation { name, args, .. } if name == "FeignClient" && args.contains(&Arg::Str("customers-service".into())))));
        assert!(java.facts.iter().any(|f| matches!(f, Fact::Extends { name, .. } if name == "AdServiceGrpc.AdServiceImplBase")));
        assert_eq!(envs(&java.facts), vec![("DB_HOST", None)]);
        assert!(calls(&java.facts).contains(&"restTemplate.getForObject") && calls(&java.facts).contains(&"AdServiceGrpc.newBlockingStub"));

        let cs = extract("a/Program.cs", "public class CartServiceImpl : CartService.CartServiceBase {\n void Go() {\n  builder.Services.AddHttpClient<CatalogService>(o => o.BaseAddress = new(\"https+http://catalog-api\"));\n  var a = Environment.GetEnvironmentVariable(\"REDIS_ADDR\");\n  var s = $\"http://{host}:8080\";\n  var c = new Basket.BasketClient(channel);\n }\n}\n").unwrap();
        assert!(cs.facts.iter().any(|f| matches!(f, Fact::Extends { name, .. } if name == "CartService.CartServiceBase")));
        assert!(strs(&cs.facts).contains(&"https+http://catalog-api"));
        assert_eq!(envs(&cs.facts), vec![("REDIS_ADDR", None)]);
        assert!(calls(&cs.facts).contains(&"new Basket.BasketClient"));
        assert!(calls(&cs.facts).iter().any(|c| c.starts_with("builder.Services.AddHttpClient")));

        let php = extract("a/index.php", "<?php\n$url = getenv('CATALOGUE_URL') ?: 'http://catalogue:8080';\n$d = file_get_contents(\"http://catalogue:8080/product/$sku\");\n$c = new Client(['base_uri' => 'http://user:8080']);\n$c->get('/x');\n").unwrap();
        assert_eq!(envs(&php.facts), vec![("CATALOGUE_URL", None)]);
        assert!(strs(&php.facts).contains(&"http://catalogue:8080") && strs(&php.facts).contains(&"http://user:8080"));
        assert!(calls(&php.facts).contains(&"file_get_contents") && calls(&php.facts).contains(&"new Client") && calls(&php.facts).contains(&"$c->get"));
        assert!(php.facts.iter().any(|f| matches!(f, Fact::Template { parts, .. } if parts.first() == Some(&Part::Lit("http://catalogue:8080/product/".into())))));
    }

    #[test]
    fn regex_extractor_covers_config_and_unknown_languages() {
        let nginx = extract("web/default.conf.template", "location /api/catalogue/ {\n    proxy_pass http://${CATALOGUE_HOST}:8080/;\n}\n").unwrap();
        assert_eq!(nginx.parser, Parser::Regex);
        assert_eq!(nginx.language, "template");
        assert_eq!(envs(&nginx.facts), vec![("CATALOGUE_HOST", None)]);
        assert!(nginx.facts.iter().any(|f| matches!(f, Fact::Template { parts, line: 2 } if parts == &[Part::Lit("http://".into()), Part::Var("CATALOGUE_HOST".into()), Part::Lit(":8080/".into())])));

        let props = extract("shipping/src/main/resources/application.properties", "spring.datasource.url=jdbc:mysql://${DB_HOST:mysql}:3306/cities\n").unwrap();
        assert_eq!(envs(&props.facts), vec![("DB_HOST", Some("mysql"))]);

        let elixir = extract("x/lib/app.ex", "url = System.get_env(\"FLAGD_HOST\") || \"flagd\"\nHTTPoison.get(\"http://product-catalog:8080/products\")\n").unwrap();
        assert_eq!(elixir.parser, Parser::Regex);
        assert_eq!(elixir.language, "ex");
        assert_eq!(envs(&elixir.facts), vec![("FLAGD_HOST", None)]);
        assert!(strs(&elixir.facts).contains(&"http://product-catalog:8080/products") && strs(&elixir.facts).contains(&"flagd"));

        let ruby = extract("x/app.rb", "host = ENV['REDIS_HOST'] || 'redis'\nENV.fetch(\"CART_URL\", \"http://cart:8080\")\n").unwrap();
        assert_eq!(envs(&ruby.facts), vec![("REDIS_HOST", None), ("CART_URL", Some("http://cart:8080"))]);
    }

    #[test]
    fn skips_what_should_not_be_read() {
        assert_eq!(language_for("a/b.min.js"), None);
        assert_eq!(language_for("a/package-lock.json"), None);
        assert_eq!(language_for("a/pnpm-lock.yaml"), None);
        assert_eq!(language_for("a/logo.png"), None);
        assert_eq!(language_for("a/font.woff2"), None);
        assert_eq!(language_for("a/Dockerfile"), Some(Language::Other));
        assert_eq!(language_for("a/x.tsx"), Some(Language::Tsx));
        assert_eq!(language_for("a/x.cs"), Some(Language::CSharp));
        assert!(is_config_file("a/nginx.conf") && is_config_file("a/.env.example") && !is_config_file("a/main.go"));
        assert!(extract("a/b.min.js", "x").is_none());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p blastradius-core map::facts`
Expected: compile error.

- [ ] **Step 3: Add dependencies**

Workspace `Cargo.toml` `[workspace.dependencies]`:
```toml
tree-sitter = "0.27"
tree-sitter-javascript = "0.25"
tree-sitter-typescript = "0.23"
tree-sitter-python = "0.25"
tree-sitter-go = "0.25"
tree-sitter-java = "0.23"
tree-sitter-c-sharp = "0.23"
tree-sitter-php = "0.24"
rayon = "1"
```
Core crate `[dependencies]`: the same nine with `.workspace = true`.

- [ ] **Step 4: Implement `facts/mod.rs`**

- `language_for(path)`: take the lowercase file name. Return `None` for: names ending in `.min.js`, `.min.css`, `.map`, `.lock`, `-lock.json`, `-lock.yaml`, `.sum`, `.svg`, `.png`, `.jpg`, `.jpeg`, `.gif`, `.ico`, `.webp`, `.woff`, `.woff2`, `.ttf`, `.eot`, `.otf`, `.pdf`, `.zip`, `.gz`, `.tar`, `.jar`, `.class`, `.dll`, `.exe`, `.so`, `.dylib`, `.wasm`, `.pyc`, `.csv`, `.snap`, `.md`, `.txt`, `.rst`, `.log`, and names `package-lock.json`, `yarn.lock`, `pnpm-lock.yaml`, `Cargo.lock`, `go.sum`, `composer.lock`, `Gemfile.lock`, `poetry.lock`, `mix.lock`. Then by extension: `js mjs cjs jsx` JavaScript; `ts mts cts` TypeScript; `tsx` Tsx; `py` Python; `go` Go; `java` Java; `cs` CSharp; `php` Php; anything else `Other`.
- `is_config_file(path)`: extension in `conf template properties ini cfg toml json yaml yml xml sh bash env` or the file name starts with `.env` or equals `Dockerfile` or ends with `.Dockerfile`.
- `extract(path, text)`: `language_for` None → None. Tree-sitter languages → `treesitter::extract(language, text)` with `parser: TreeSitter`, `language` label `"javascript" | "typescript" | "tsx" | "python" | "go" | "java" | "csharp" | "php"`. Other → `regex::extract(text)` with label = extension lowercase, or the file name for extensionless files (`dockerfile`). If tree-sitter returns no tree (timeout or error), fall back to regex.
- Shared helpers used by both extractors, in `facts/mod.rs`:
  ```rust
  pub(crate) const ENV_CALLEES: &[&str] = &["os.getenv", "os.environ.get", "os.Getenv", "os.LookupEnv", "System.getenv", "Environment.GetEnvironmentVariable", "getenv", "ENV.fetch", "System.get_env", "std::env::var", "env::var", "Deno.env.get", "System.getProperty"];
  pub(crate) fn env_callee(callee: &str) -> bool   // exact match, or ends with `.` + one of them (e.g. `Environment.GetEnvironmentVariable` inside a namespace)
  pub(crate) fn unquote(raw: &str) -> Option<String>   // strips one layer of matching quotes, Python/JS prefixes (f, r, b, u, @, $), backticks; None when not a quoted string
  pub(crate) fn split_template(text: &str) -> Vec<Part>   // `${...}` / `{...}` / `$name` / `#{...}` substitutions -> Var, rest Lit; Var text has the braces removed
  pub(crate) fn env_default(var: &str) -> (String, Option<String>)  // "DB_HOST:mysql" -> ("DB_HOST", Some("mysql")); "X:-d" -> ("X", Some("d")); "X" -> ("X", None)
  ```

- [ ] **Step 5: Implement `facts/treesitter.rs`**

Per-language table, from the parse trees observed on the probe:

| Language | string kinds | template kinds | call kinds (callee = text of first named child, minus the arguments) | import kinds (path = first string child, or the dotted name text) | annotation kinds | extends |
| --- | --- | --- | --- | --- | --- | --- |
| JavaScript, TypeScript, Tsx | `string` | `template_string` (children `string_fragment` → Lit, `template_substitution` → Var of inner text) | `call_expression`, `new_expression` (callee prefixed `new `) | `import_statement`; plus a `require` call with a string arg produces an Import | `decorator` | none |
| Python | `string` (text starting `f"`/`f'`/`F` → template: split `{...}`), `concatenated_string` (each child string → Str) | f-strings as above | `call` | `import_statement` (each `dotted_name` text), `import_from_statement` (the module `dotted_name`) | `decorator` (name = text of its `call`'s function or its `attribute`/`identifier`) | none |
| Go | `interpreted_string_literal`, `raw_string_literal` | none | `call_expression` | `import_spec` (its string literal) | none | none |
| Java | `string_literal` | none | `method_invocation` (callee = text before the `argument_list` child), `object_creation_expression` (callee `new ` + type text) | `import_declaration` (text between `import ` and `;`) | `annotation`, `marker_annotation` (name = child `identifier`/`scoped_identifier`; args = string literals in `annotation_argument_list`, including inside `element_value_pair`) | `superclass` (type text), `super_interfaces` types |
| C# | `string_literal`, `verbatim_string_literal`, `raw_string_literal` | `interpolated_string_expression` (children `string_content` → Lit, `interpolation` → Var of inner identifier text) | `invocation_expression` (callee = text of first child), `object_creation_expression` (`new ` + type text), `implicit_object_creation_expression` (callee `new`) | `using_directive` (text between `using ` and `;`) | `attribute` (name = first child text; args = strings in `argument_list`) | `base_list` (each named child's text) |
| PHP | `string` | `encapsed_string` (children `string_content` → Lit, `variable_name`/`encapsed_variable`/`simple_variable` → Var) | `function_call_expression`, `member_call_expression`, `scoped_call_expression`, `nullsafe_member_call_expression` (callee = text before `arguments` child), `object_creation_expression` (`new ` + class text) | `namespace_use_declaration` (each `namespace_use_clause` text) | `attribute` | `base_clause` |

Implementation shape:
```rust
pub(crate) fn extract(language: Language, text: &str) -> Option<Vec<Fact>> {
    let ts_language: tree_sitter::Language = match language { JavaScript => tree_sitter_javascript::LANGUAGE.into(), TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(), Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(), Python => tree_sitter_python::LANGUAGE.into(), Go => tree_sitter_go::LANGUAGE.into(), Java => tree_sitter_java::LANGUAGE.into(), CSharp => tree_sitter_c_sharp::LANGUAGE.into(), Php => tree_sitter_php::LANGUAGE_PHP.into(), Other => return None };
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&ts_language).ok()?;
    let tree = parser.parse(text, None)?;
    let table = table_for(language);
    let mut facts = Vec::new();
    walk(tree.root_node(), text.as_bytes(), &table, &mut facts);
    Some(facts)
}
```
`walk` is a manual recursion with `node.walk()`/`children`: for each named node, check kind against the table in the order strings, templates, calls, imports, annotations, extends; after handling a call, still recurse into its children so nested strings become facts (a string is a fact whether or not it is also an argument). Line = `node.start_position().row + 1`. Callee text: `node.utf8_text(src)` of the child before the arguments child; arguments: iterate named children of the arguments node, string → `Arg::Str(unquoted)`, template → `Arg::Template(parts)`, else `Arg::Other(text)`. Env: when `env_callee(callee)` and the first arg is `Arg::Str`, also push `Fact::EnvRef { name, default: second Arg::Str }`. JavaScript `member_expression` whose text starts with `process.env.` produces `EnvRef` (name = last segment, default = the string on the right of an enclosing `||` or `??` `binary_expression`, when it is a string). Python `subscript` on `os.environ` with a string index produces `EnvRef`. Strip quotes with `unquote`. Deduplicate nothing here; the matchers merge.

Set `parser.set_timeout_micros(200_000)` so a pathological file cannot stall the run; on `None` the caller falls back to regex.

- [ ] **Step 6: Implement `facts/regex.rs`**

For each line (1-based):
- Quoted strings: `"([^"\\]|\\.)*"` and `'([^'\\]|\\.)*'` and backtick strings → `Str` (or `Template` when the content contains `${`, `#{`, `{` followed by an identifier and `}`; use `split_template`).
- Bare URLs outside quotes (config files): `[a-z][a-z0-9+.-]*://[^\s"'<>;,)]+` → `Str`, or `Template` when it contains `${`/`$`.
- `${VAR}`, `${VAR:-d}`, `${VAR:d}`, `$VAR` anywhere → `EnvRef` via `env_default`.
- Env lookup calls: regex built from `ENV_CALLEES` joined with `|`, followed by `\s*\(\s*(['"])([A-Za-z_][A-Za-z0-9_]*)\1(?:\s*,\s*(['"])([^'"]*)\3)?` → `EnvRef { name, default }`; also `ENV\[['"]([A-Z_][A-Z0-9_]*)['"]\]` → `EnvRef`, with default from a following `\|\|\s*(['"])([^'"]*)\1`.
- Calls: `\b([A-Za-z_][\w$]*(?:(?:\.|->|::)[A-Za-z_][\w$]*)*)\s*\(` → `Call { callee, args: strings inside the parentheses up to the matching close on the same line }`. Skip callees that are keywords (`if`, `while`, `for`, `switch`, `return`, `function`, `def`, `elif`, `catch`).
- Imports: `^\s*(?:import|require|use|include|from)\b.*?(['"])([^'"]+)\1` → `Import`.

- [ ] **Step 7: Run tests, fmt, clippy**

Run: `cargo test -p blastradius-core map::facts && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: 5 passed. The first tree-sitter build takes a minute.

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock crates/blastradius-core
git commit -m "feat(map): language-neutral facts from tree-sitter and a regex fallback"
```

---

### Task B2: Configuration index

**Files:**
- Create: `src/map/config.rs`

**Interfaces:**
```rust
pub struct EnvValue { pub value: String, pub evidence: Evidence }
pub struct ProtoService { pub name: String, pub package: Option<String>, pub evidence: Evidence }
pub struct ConfigIndex {
    per_service: HashMap<String, IndexMap<String, EnvValue>>,   // service name -> VAR -> value (compose environment, k8s container env)
    global: IndexMap<String, EnvValue>,                         // ConfigMap data, dotenv at root and beside compose files (first wins)
    pub protos: Vec<ProtoService>,
}
impl ConfigIndex {
    pub fn build(root: &Path, index: &FileIndex, services: &[Service]) -> Self;
    pub fn lookup(&self, service: &str, var: &str) -> Option<&EnvValue>;  // per-service first, then global
}
```
- Compose: for every compose file (same patterns as discovery), for each service, `environment` as a map (`KEY: value`) or a list (`KEY=value`), values interpolated with the compose dir's dotenv; evidence is the compose file and the key's line. `env_file:` entries (string or list) are read as dotenv relative to the compose dir and added per service.
- Kubernetes: for every workload document, `containers[].env[]` with `name` and literal `value` → per service (by `metadata.name`); `valueFrom` is ignored. `kind: ConfigMap` → `data` entries into `global` with the ConfigMap file and line.
- Dotenv: `.env.example`, `.env` at root → global (`.env` wins).
- Protos: every `*.proto` file: regex `(?m)^\s*service\s+(\w+)\s*\{` and `(?m)^\s*package\s+([\w.]+)\s*;` → `ProtoService`.

- [ ] **Step 1: Write the failing tests** (unit tests in `config.rs`, using the new fixtures created in Task B4; write the fixture first, see B4 Step 1, then these):

```rust
#[test]
fn compose_environment_per_service_and_dotenv_global() {
    let root = fixture("edges-http-app"); let index = FileIndex::build(&root);
    let services = blastradius::discover::discover_services(&root, &index).services;
    let cfg = ConfigIndex::build(&root, &index, &services);
    let v = cfg.lookup("web", "CATALOGUE_HOST").unwrap();
    assert_eq!(v.value, "catalogue");
    assert_eq!((v.evidence.file.as_str(), v.evidence.line), ("docker-compose.yml", Some(5)));
    assert_eq!(cfg.lookup("cart", "REDIS_HOST").map(|v| v.value.as_str()), Some("redis"));
    assert_eq!(cfg.lookup("nobody", "GLOBAL_THING").map(|v| v.value.as_str()), Some("from-dotenv"));
    assert!(cfg.lookup("web", "MISSING").is_none());
}

#[test]
fn kubernetes_env_configmaps_and_protos() {
    let root = fixture("edges-grpc-app"); let index = FileIndex::build(&root);
    let services = blastradius::discover::discover_services(&root, &index).services;
    let cfg = ConfigIndex::build(&root, &index, &services);
    assert_eq!(cfg.lookup("frontend", "CART_SERVICE_ADDR").map(|v| v.value.as_str()), Some("cartservice:7070"));
    assert_eq!(cfg.lookup("frontend", "FROM_CONFIGMAP").map(|v| v.value.as_str()), Some("emailservice:5000"));
    let names: Vec<&str> = cfg.protos.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["CartService", "EmailService", "ShippingService"]);
    assert_eq!(cfg.protos[0].evidence.file, "protos/demo.proto");
}
```

- [ ] **Step 2: Run to verify failure**, **Step 3: Implement**, **Step 4: Run tests, fmt, clippy**

- [ ] **Step 5: Commit**

```bash
git add crates/blastradius-core/src/map/config.rs test/fixtures
git commit -m "feat(map): configuration index of environment values and proto services"
```

---

### Task B3: Resolver

**Files:**
- Create: `src/map/resolve.rs`; add `Candidate`, `Target` to `src/map/mod.rs`

**Interfaces:**
```rust
// map/mod.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Url(String),            // full URL text, scheme included; also `jdbc:...`
    HostPort(String),       // `payment:50051`
    Host(String),           // bare host from an env default or a known-hostname position
    EnvVar { name: String, default: Option<String> },
    Template(Vec<Part>),
    ProtoService(String),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub target: Target,
    pub kind_hint: Option<EdgeType>,   // grpc matcher sets Some(Grpc); http matcher leaves None so the resolver types by target
    pub evidence: Evidence,
}
// resolve.rs
pub struct Resolved { pub target: String, pub edge_type: EdgeType, pub confidence: Confidence, pub detail: String }
pub struct Resolver<'a> { services: &'a [Service], config: &'a ConfigIndex, by_normalised: HashMap<String, String>, proto_owner: HashMap<String, String> }
impl<'a> Resolver<'a> {
    pub fn new(services: &'a [Service], config: &'a ConfigIndex, proto_owner: HashMap<String, String>) -> Self;
    pub fn resolve(&self, source: &str, candidate: &Candidate) -> Result<Resolved, Unresolved>;   // Unresolved(String) names what could not be placed
    pub fn service_for_host(&self, host: &str) -> Option<&str>;   // exact, then first DNS label exact, then normalised
    pub fn classify(&self, target: &str, scheme: Option<&str>, scheme_less_host_port: bool) -> EdgeType;
}
pub fn normalise_var(name: &str) -> String;   // strips hostish suffixes then discover::directories::normalise
pub fn parse_url(text: &str) -> Option<(String /*scheme*/, String /*host*/)>;
```
Resolution rules, in order, per `Target`:
- `Url`: `parse_url` (handles `scheme://[user:pass@]host[:port][/...]`, `jdbc:sub://host`, `https+http://host`); host `localhost`, `127.0.0.1`, `0.0.0.0`, `::1`, or anything with a dot-separated public suffix that is not a discovered service → Unresolved. `service_for_host` → Static, type by scheme then target classification. Detail: the URL text.
- `HostPort`: host part → `service_for_host` → Static; type `grpc` when the target owns a proto service, else by target classification (`http` default). Detail: the text.
- `Host`: `service_for_host`, Static, type by target classification. Detail: the host.
- `EnvVar`: `config.lookup(source, name)` → value → try as `Url`, then `HostPort`, then `Host`; Static; detail `NAME=value via file:line`. Else `default` → same three tries, Static, detail `NAME default value`. Else `normalise_var(name)` equals a service's normalised name → Uncertain, detail `NAME matched by name`. Else Unresolved(name).
- `Template`: for each `Var` part, `EnvVar` rule with no default; for each `Lit` part, if it parses as a URL or host:port, the Url/HostPort rule; the first hit wins and confidence is capped at Uncertain unless a Var resolved through config (then Static). Unresolved when nothing hits.
- `ProtoService`: `proto_owner[name]` → Static, type Grpc, detail `proto service Name`. Else Unresolved.
- Any resolution whose target equals `source` → dropped (return `Err(Unresolved("self"))` and let `run` not count it).

`service_for_host`: exact name; else the first DNS label (`cart.default.svc.cluster.local` → `cart`) exact; else `normalise(label)` equals `normalise(service.name)` (also try `image_basename(service.image)` normalised for infrastructure); else None.

`normalise_var`: uppercase; strip a trailing `_HOST`, `_HOSTNAME`, `_HOST_NAME`, `_URL`, `_URI`, `_ADDR`, `_ADDRESS`, `_ENDPOINT`, `_BASE_URL`, `_SERVICE_HOST`, `_SERVICE_ADDR`, `_SERVER`, `_API_URL`, `_API`, `_SERVICE`, `_PORT` (repeat until none matches, at most three times); then `normalise` (lowercase, drop separators, strip `service|svc|api|server` suffix).

- [ ] **Step 1: Write the failing unit tests** in `resolve.rs` (build `Service` values inline with a helper; no fixtures)

```rust
#[test]
fn hosts_resolve_exact_label_and_normalised() {
    let r = resolver(&["cart", "productcatalogservice", "Basket.API"], &[("redis", "redis:7")]);
    assert_eq!(r.service_for_host("cart"), Some("cart"));
    assert_eq!(r.service_for_host("cart.default.svc.cluster.local"), Some("cart"));
    assert_eq!(r.service_for_host("product-catalog-service"), Some("productcatalogservice"));
    assert_eq!(r.service_for_host("basket-api"), Some("Basket.API"));
    assert_eq!(r.service_for_host("localhost"), None);
    assert_eq!(r.service_for_host("redis"), Some("redis"));
}

#[test]
fn urls_type_by_scheme_then_target() {
    let r = resolver(&["catalogue", "payment"], &[("redis", "redis:7"), ("rabbitmq", "rabbitmq:3")]);
    let ok = |t: Target| r.resolve("web", &Candidate { target: t, kind_hint: None, evidence: ev() }).unwrap();
    let c = ok(Target::Url("http://catalogue:8080/products".into()));
    assert_eq!((c.target.as_str(), c.edge_type, c.confidence), ("catalogue", EdgeType::Http, Confidence::Static));
    assert_eq!(ok(Target::Url("redis://redis:6379/0".into())).edge_type, EdgeType::Database);
    assert_eq!(ok(Target::Url("amqp://guest:guest@rabbitmq:5672".into())).edge_type, EdgeType::Event);
    assert_eq!(ok(Target::Host("redis".into())).edge_type, EdgeType::Database);
    assert_eq!(ok(Target::Url("jdbc:mysql://mysql:3306/cities".into())).edge_type, EdgeType::Database).unwrap_err_is_unresolved_for_unknown_mysql(); // see below
}
```
Replace the last line with two assertions: `jdbc:mysql://catalogue:3306/x` resolves to `catalogue` with `Database`; `http://example.com/x` is `Err`.

```rust
#[test]
fn env_vars_resolve_through_config_default_or_name() {
    let mut cfg = ConfigIndex::default();
    cfg.insert_for_test("web", "CATALOGUE_HOST", "catalogue", "docker-compose.yml", 5);
    let r = Resolver::new(&services(&["catalogue", "user", "cart"], &[]), &cfg, HashMap::new());
    let c = r.resolve("web", &cand(Target::EnvVar { name: "CATALOGUE_HOST".into(), default: None })).unwrap();
    assert_eq!((c.target.as_str(), c.confidence), ("catalogue", Confidence::Static));
    assert!(c.detail.contains("CATALOGUE_HOST=catalogue via docker-compose.yml:5"));
    let c = r.resolve("web", &cand(Target::EnvVar { name: "USER_HOST".into(), default: Some("user".into()) })).unwrap();
    assert_eq!((c.target.as_str(), c.confidence), ("user", Confidence::Static));
    let c = r.resolve("web", &cand(Target::EnvVar { name: "CART_SERVICE_ADDR".into(), default: None })).unwrap();
    assert_eq!((c.target.as_str(), c.confidence), ("cart", Confidence::Uncertain));
    assert!(r.resolve("web", &cand(Target::EnvVar { name: "AMQP_HOST".into(), default: None })).is_err());
    assert!(r.resolve("cart", &cand(Target::EnvVar { name: "CART_HOST".into(), default: None })).is_err(), "self edge dropped");
}

#[test]
fn templates_and_protos() {
    let mut owner = HashMap::new(); owner.insert("CartService".to_string(), "cart".to_string());
    let cfg = ConfigIndex::default();
    let r = Resolver::new(&services(&["cart", "user"], &[]), &cfg, owner);
    let c = r.resolve("web", &cand(Target::Template(vec![Part::Lit("http://".into()), Part::Var("USER_HOST".into()), Part::Lit(":8080/".into())]))).unwrap();
    assert_eq!((c.target.as_str(), c.confidence, c.edge_type), ("user", Confidence::Uncertain, EdgeType::Http));
    let c = r.resolve("web", &cand(Target::ProtoService("CartService".into()))).unwrap();
    assert_eq!((c.target.as_str(), c.confidence, c.edge_type), ("cart", Confidence::Static, EdgeType::Grpc));
    let c = r.resolve("web", &cand(Target::HostPort("cart:7070".into()))).unwrap();
    assert_eq!(c.edge_type, EdgeType::Grpc, "scheme-less host:port to a proto owner is grpc");
    assert_eq!(normalise_var("PRODUCT_CATALOG_SERVICE_ADDR"), "productcatalog");
    assert_eq!(normalise_var("CATALOGUE_HOST"), "catalogue");
    assert_eq!(normalise_var("SPRING_DATASOURCE_URL"), "springdatasource");
}
```
`ConfigIndex::default()` and `insert_for_test` (cfg(test) helper) exist for these tests.

- [ ] **Step 2: Run to verify failure**, **Step 3: Implement**, **Step 4: Run tests, fmt, clippy**

- [ ] **Step 5: Commit**

```bash
git add crates/blastradius-core/src/map
git commit -m "feat(map): resolver from hosts, variables, templates and protos to services"
```

---

### Task B4: Fixtures, HTTP matcher, gRPC matcher, and the map stage

**Files:**
- Create: `test/fixtures/edges-http-app/**`, `test/fixtures/edges-grpc-app/**`, `src/map/matchers/mod.rs`, `src/map/matchers/http.rs`, `src/map/matchers/grpc.rs`, `crates/blastradius-core/tests/edges.rs`
- Modify: `src/map/mod.rs` (`run`), `src/analyze.rs` (mapping field, call `map::run`), `src/lib.rs`

**Interfaces:**
```rust
// matchers/mod.rs
pub trait Matcher { fn name(&self) -> &'static str; fn candidates(&self, ctx: &FileContext<'_>) -> Vec<Candidate>; }
pub struct FileContext<'a> { pub service: &'a str, pub file: &'a str, pub facts: &'a [Fact], pub config: &'a ConfigIndex }
pub fn all() -> Vec<Box<dyn Matcher + Sync>>;   // http, grpc
// grpc.rs also exposes ownership discovery used before resolution:
pub fn proto_owners(files: &[(String /*service*/, String /*file*/, Vec<Fact>)], config: &ConfigIndex, services: &[Service]) -> HashMap<String, String>;
// map/mod.rs
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MappingStats { pub files_scanned: usize, pub files_skipped: IndexMap<String, usize>, pub files_outside_services: usize, pub unresolved: usize, pub unresolved_targets: Vec<String>, pub parsers: IndexMap<String, facts::Parser> }
pub struct MapResult { pub edges: Vec<Edge>, pub stats: MappingStats }
pub fn run(root: &Path, index: &FileIndex, services: &[Service]) -> MapResult;
```

**HTTP matcher** (`http.rs`), over facts of one file:
- Every `Str` whose value parses as a URL → `Target::Url`. Every `Str` matching `^[a-z][a-z0-9-]*(\.[a-z0-9-]+)*:\d{2,5}$` → `Target::HostPort`.
- Every `Template` whose Lit parts contain `://` or whose Var part is hostish → `Target::Template(parts)`.
- Every `EnvRef` whose name is hostish (`normalise_var` changed it, or it ends with `_HOST|_URL|_URI|_ADDR|_ADDRESS|_ENDPOINT|_SERVER|_SERVICE`) → `Target::EnvVar`. Its `default`, when a bare word, also produces `Target::Host(default)` only if it equals a discovered infrastructure service name exactly (the resolver handles the check; the matcher emits `Target::Host` and lets it fail otherwise).
- Every `Str` equal to a discovered infrastructure service's name exactly (length ≥ 4) → `Target::Host` (bare hostname defaults like `"rabbitmq"`). The matcher receives the service names through `FileContext::config` (add `pub infrastructure_names: &[String]` to `FileContext`).
- `Annotation` named `FeignClient` with a string arg → `Target::Host(arg)`; `Annotation` `LoadBalanced`/`HttpExchange` ignored.
- Evidence: file, line, detail = the matched text (URL, `VAR`, template rendered with `${VAR}` markers, annotation text).

**gRPC matcher** (`grpc.rs`):
- Client facts: `Call` callee matching `New(\w+)Client$`, `(\w+)Client$` after `new `, `(\w+)Stub$`, `(\w+)Grpc\.new\w*Stub`, `AddGrpcClient<(\w+)\.(\w+)Client>` → service name = the captured group with `Client`/`Stub` removed; when the callee is `X.YClient`/`X.YStub`, use `Y`, and if `Y` is not a known proto service but `X` is, use `X` (C# `Basket.BasketClient`). Emit `Candidate { target: ProtoService(name), kind_hint: Some(Grpc) }` only when `name` is in `config.protos` or normalises to a discovered service.
- Server facts (for ownership, not edges): `Call` callee matching `Register(\w+)Server$`, `add_(\w+)Servicer_to_server`, `addService` with an arg text containing `\.(\w+)\.service`; `Extends` matching `(\w+)Grpc\.(\w+)ImplBase` → group 1, `(\w+)\.(\w+)Base$` → group 1. `proto_owners` collects `proto service name → owning service` from these; when a proto service has no registration anywhere, the owner is the discovered service whose normalised name equals the normalised proto name (`CartService` → `cart` → `cartservice`).
- `Health` and `Reflection` are never edges.

**`map::run`**:
1. Build `ConfigIndex`. Sort code services by root length descending for ownership. `owner_of(file)`: first service whose `root` is `.` or `file.starts_with(root + "/")`.
2. Partition `index.files()`: outside → count; `language_for` None → `files_skipped[ext]++`; else read and `facts::extract` in parallel (`rayon::par_iter`), collecting `(service, file, Extraction)`. Record `parsers[label] = parser` (first seen) and `files_scanned`.
3. `proto_owners` over all extractions; `Resolver::new`.
4. For every extraction and every matcher, candidates → `resolver.resolve(service, &candidate)`; `Ok` → edge `{ source: service, target, edge_type, confidence, evidence: [Evidence { file, line, detail }] }`; `Err(Unresolved(name))` when not a self-edge → `unresolved += 1`, push name.
5. Merge edges by `(source, target, edge_type)`: union evidence (dedupe by file+line+detail, sort by file then line, cap 25 per edge), keep the highest confidence. Sort edges by source, target, type. `unresolved_targets` distinct sorted capped 50.

**`analyze`**: after discovery, `let mapped = map::run(&root, &index, &services)`; add edges to the graph (targets always exist, so `add_edge` cannot fail; `expect` with a message). `Analysis` gains `pub mapping: MappingStats`; `AnalysisJson` gains `mapping` between `edges` and `runtime`.

- [ ] **Step 1: Write the fixtures**

`test/fixtures/edges-http-app/`:
```
docker-compose.yml
.env
web/Dockerfile
web/default.conf.template
cart/package.json            {"name":"cart","main":"server.js"}
cart/server.js
catalogue/go.mod             module catalogue
catalogue/main.go
payment/requirements.txt     flask
payment/payment.py
ratings/composer.json        {"name":"robot/ratings"}
ratings/html/index.php
```
`docker-compose.yml`:
```yaml
services:
  web:
    build: ./web
    environment:
      CATALOGUE_HOST: catalogue
      USER_HOST: user
  cart:
    build: ./cart
    environment:
      - REDIS_HOST=redis
  catalogue:
    build: ./catalogue
  payment:
    build: ./payment
  ratings:
    build: ./ratings
  redis:
    image: redis:7
  rabbitmq:
    image: rabbitmq:3-management
  mongodb:
    image: mongo:6
```
`.env`: `GLOBAL_THING=from-dotenv`
`web/default.conf.template`:
```
location /api/catalogue/ { proxy_pass http://${CATALOGUE_HOST}:8080/; }
location /api/cart/ { proxy_pass http://${CART_HOST}:8080/; }
location /api/ratings/ { proxy_pass http://${RATINGS_HOST}:80/; }
```
`cart/server.js`:
```js
const redis = require('redis');
const catalogueHost = process.env.CATALOGUE_HOST || 'catalogue';
const redisHost = process.env.REDIS_HOST || 'redis';
fetch(`http://${catalogueHost}:8080/product/${sku}`);
```
`catalogue/main.go`:
```go
package main
import "os"
func main() {
	mongo := os.Getenv("MONGO_URL")
	if mongo == "" { mongo = "mongodb://mongodb:27017/catalogue" }
}
```
`payment/payment.py`:
```python
import os, requests
USER = os.getenv('USER_HOST', 'user')
CART = os.getenv('CART_HOST', 'cart')
AMQP = os.getenv('AMQP_HOST', 'rabbitmq')
requests.get('http://' + USER + ':8080/check/' + id)
requests.get('http://{}:8080/cart/{}'.format(CART, id))
requests.post('https://paypal.com/pay')
```
`ratings/html/index.php`:
```php
<?php
$url = getenv('CATALOGUE_URL') ?: 'http://catalogue:8080';
$pdo = new PDO(getenv('PDO_URL'));
```
Note `user` is deliberately not a service: `USER_HOST` is an unresolved target and appears in `unresolvedTargets`.

`test/fixtures/edges-grpc-app/`:
```
kubernetes/frontend.yaml
kubernetes/cartservice.yaml
kubernetes/emailservice.yaml
kubernetes/shippingservice.yaml
kubernetes/config.yaml           ConfigMap FROM_CONFIGMAP: emailservice:5000
protos/demo.proto                package hipstershop; service CartService {} service EmailService {} service ShippingService {}
src/frontend/go.mod  src/frontend/main.go        pb.NewCartServiceClient(conn); pb.NewShippingServiceClient(conn); os.Getenv("CART_SERVICE_ADDR")
src/cartservice/cartservice.csproj  src/cartservice/Program.cs   class CartServiceImpl : CartService.CartServiceBase
src/emailservice/requirements.txt  src/emailservice/email_server.py   demo_pb2_grpc.add_EmailServiceServicer_to_server(...)
src/shippingservice/go.mod  src/shippingservice/main.go   pb.RegisterShippingServiceServer(srv, svc)
src/checkoutservice/go.mod  src/checkoutservice/main.go   pb.NewEmailServiceClient(conn); os.Getenv("EMAIL_SERVICE_ADDR")
```
`kubernetes/frontend.yaml` has a Deployment `frontend` with env `CART_SERVICE_ADDR: "cartservice:7070"` and `FROM_CONFIGMAP` coming from the ConfigMap (write the ConfigMap `data` and reference nothing; the index puts ConfigMap data in `global`). `kubernetes/checkoutservice.yaml` has a Deployment `checkoutservice` with env `EMAIL_SERVICE_ADDR: "emailservice:5000"`. Every Deployment names an image `acme/<name>:1.0`.

- [ ] **Step 2: Write the failing integration tests** `tests/edges.rs`

```rust
mod common;
use blastradius::*;
use common::fixture;
use pretty_assertions::assert_eq;
use std::collections::BTreeSet;

fn edges_of(name: &str) -> (Vec<Edge>, AnalysisJson) { let a = analyze(&fixture(name)).unwrap(); let j = a.to_json(); (j.edges.clone(), j) }
fn triple(e: &Edge) -> (String, String, EdgeType) { (e.source.clone(), e.target.clone(), e.edge_type) }
fn find<'a>(edges: &'a [Edge], s: &str, t: &str, ty: EdgeType) -> &'a Edge { edges.iter().find(|e| e.source == s && e.target == t && e.edge_type == ty).unwrap_or_else(|| panic!("no edge {s} -> {t} {ty:?} in {:?}", edges.iter().map(triple).collect::<Vec<_>>())) }

#[test]
fn http_edges_from_literals_env_templates_and_config_files() {
    let (edges, json) = edges_of("edges-http-app");
    let set: BTreeSet<(String, String, EdgeType)> = edges.iter().map(triple).collect();
    let expect = |s: &str, t: &str, ty: EdgeType| assert!(set.contains(&(s.into(), t.into(), ty)), "missing {s} -> {t} {ty:?}; have {set:?}");
    expect("web", "catalogue", EdgeType::Http);      // nginx template ${CATALOGUE_HOST} resolved through compose environment
    expect("web", "cart", EdgeType::Http);           // ${CART_HOST} unresolved, matched by name
    expect("web", "ratings", EdgeType::Http);
    expect("cart", "catalogue", EdgeType::Http);     // process.env default + template
    expect("cart", "redis", EdgeType::Database);     // REDIS_HOST via compose -> infrastructure redis
    expect("catalogue", "mongodb", EdgeType::Database);
    expect("payment", "cart", EdgeType::Http);
    expect("payment", "rabbitmq", EdgeType::Event);  // AMQP_HOST default "rabbitmq" bare infrastructure name
    expect("ratings", "catalogue", EdgeType::Http);

    let e = find(&edges, "web", "catalogue", EdgeType::Http);
    assert_eq!(e.confidence, Confidence::Static);
    assert_eq!(e.evidence[0].file, "web/default.conf.template");
    assert_eq!(e.evidence[0].line, Some(1));
    assert!(e.evidence[0].detail.as_deref().unwrap().contains("CATALOGUE_HOST=catalogue via docker-compose.yml:5"), "{:?}", e.evidence);
    assert_eq!(find(&edges, "web", "cart", EdgeType::Http).confidence, Confidence::Uncertain);
    assert_eq!(find(&edges, "cart", "catalogue", EdgeType::Http).confidence, Confidence::Static);
    assert_eq!(find(&edges, "catalogue", "mongodb", EdgeType::Database).evidence[0].detail.as_deref(), Some("mongodb://mongodb:27017/catalogue"));

    assert!(!edges.iter().any(|e| e.target == "paypal.com" || e.source == e.target));
    assert!(json.mapping.unresolved_targets.contains(&"USER_HOST".to_string()), "{:?}", json.mapping.unresolved_targets);
    assert!(json.mapping.files_scanned >= 5);
    assert_eq!(json.mapping.parsers.get("template").copied(), Some(blastradius::map::facts::Parser::Regex));
    assert_eq!(json.mapping.parsers.get("go").copied(), Some(blastradius::map::facts::Parser::TreeSitter));
}

#[test]
fn grpc_edges_from_stubs_and_proto_ownership() {
    let (edges, json) = edges_of("edges-grpc-app");
    let e = find(&edges, "frontend", "cartservice", EdgeType::Grpc);
    assert_eq!(e.confidence, Confidence::Static);
    assert!(e.evidence.iter().any(|v| v.detail.as_deref() == Some("pb.NewCartServiceClient")), "{:?}", e.evidence);
    assert!(e.evidence.iter().any(|v| v.detail.as_deref().unwrap().starts_with("CART_SERVICE_ADDR=cartservice:7070")), "the _ADDR variable merges into the grpc edge: {:?}", e.evidence);
    find(&edges, "frontend", "shippingservice", EdgeType::Grpc);
    find(&edges, "checkoutservice", "emailservice", EdgeType::Grpc);
    assert!(!edges.iter().any(|e| e.edge_type == EdgeType::Http && e.source == "frontend" && e.target == "cartservice"), "no duplicate http edge for the same pair");
    assert_eq!(json.mapping.unresolved, 0, "{:?}", json.mapping.unresolved_targets);
}

#[test]
fn json_contract_gains_mapping_between_edges_and_runtime() {
    let json = serde_json::to_value(analyze(&fixture("edges-http-app")).unwrap().to_json()).unwrap();
    let keys: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, vec!["repository", "root", "discovery", "services", "edges", "mapping", "runtime"]);
    let m = &json["mapping"];
    for k in ["filesScanned", "filesSkipped", "filesOutsideServices", "unresolved", "unresolvedTargets", "parsers"] { assert!(m.get(k).is_some(), "{k}"); }
    let first = &json["edges"][0];
    assert_eq!(first.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(), vec!["source", "target", "type", "confidence", "evidence"]);
}

#[test]
fn discovery_only_fixtures_still_have_no_edges_and_report_skips() {
    let json = analyze(&fixture("compose-app")).unwrap().to_json();
    assert!(json.edges.is_empty());
    assert!(json.mapping.files_scanned >= 1);
}
```

- [ ] **Step 3: Run to verify failure**, **Step 4: Implement matchers and `run`**, **Step 5: Run all tests including parity**

Parity note: the parity JSON files under `test/expected/discovery/` do not have a `mapping` key. Update `tests/parity.rs` `normalise` to remove `mapping` from the actual output and to accept an empty `edges` array on both sides; the parity test still guards discovery. Corpus counts are unchanged.

- [ ] **Step 6: fmt, clippy, commit**

```bash
git add crates/blastradius-core test/fixtures
git commit -m "feat(map): http and grpc edges with evidence, confidence and the mapping block"
```

---

### Task B5: Terminal report: structure, edges, findings

**Files:**
- Modify: `src/report.rs`; add tests to `tests/edges.rs`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn report_shows_structure_edges_and_findings() {
    let r = format_repo_report(&analyze(&fixture("edges-http-app")).unwrap(), false);
    assert!(r.contains("STRUCTURE"));
    assert!(regex::Regex::new(r"Total edges\s+\d+").unwrap().is_match(&r), "{r}");
    assert!(r.contains("Static") && r.contains("Uncertain"), "{r}");
    assert!(r.contains("EDGES"));
    assert!(regex::Regex::new(r"web\s+->\s+catalogue\s+http\s+static\s+web/default\.conf\.template:1").unwrap().is_match(&r), "{r}");
    assert!(r.contains("FINDINGS"));
    assert!(r.contains("Never called by another service"), "{r}");
    assert!(r.contains("Most connected"), "{r}");
    assert!(regex::Regex::new(r"Scanned \d+ files in \d+ services").unwrap().is_match(&r), "{r}");
    assert!(r.contains("1 target could not be matched to a service: USER_HOST") || r.contains("could not be matched"), "{r}");
    assert!(!r.contains("Dependency mapping is not built yet"));
}

#[test]
fn report_without_edges_says_none_were_found() {
    let r = format_repo_report(&analyze(&fixture("compose-app")).unwrap(), false);
    assert!(r.contains("Total edges") && r.contains("0"));
    assert!(r.contains("No static edges found"), "{r}");
}
```

- [ ] **Step 2: Run to verify failure**, **Step 3: Implement**

Replace the STRUCTURE placeholder with:
```
STRUCTURE

  Total edges              9
  Static                   6
  Inferred                 0
  Uncertain                3
  By type                  http 6 · database 2 · event 1

  Scanned 7 files in 5 services (tree-sitter: go, javascript, php, python · regex: template)
  1 target could not be matched to a service: USER_HOST

EDGES

  SOURCE     ->  TARGET     TYPE      CONFIDENCE  EVIDENCE
  cart       ->  catalogue  http      static      cart/server.js:4  http://${catalogueHost}:8080/product/${sku}
  ...

FINDINGS

  Never called by another service        2 services
    payment, web                         (dead, or just quiet?)

  Most connected                         catalogue
    touched by 3 services
```
Rules: EDGES rows sorted as in the JSON, evidence column is the first evidence's `file:line` followed by two spaces and its detail truncated to 60 characters. Confidence printed lowercase. `Never called` lists code services with no inbound edge, sorted, wrapped at 70 columns. `Most connected` is the service with the most distinct inbound sources; ties broken by name. When there are no edges: `Total edges 0` then the dimmed line `No static edges found: no HTTP or gRPC call to another discovered service was recognised.` and no EDGES or FINDINGS sections. Unresolved line: `N targets could not be matched to a service: a, b, c` (first five, then `...`); singular when N is 1.

- [ ] **Step 4: Run tests, fmt, clippy**, **Step 5: Commit**

```bash
git add crates/blastradius-core
git commit -m "feat(report): structure counts, edges table and findings"
```

---

### Task B6: Corpus edges, recall check, and fixes

**Files:**
- Modify: `test/expected/corpus/microservices-demo.json`, `robot-shop.json`, `eshop.json`, `crates/blastradius-core/tests/corpus.rs`

- [ ] **Step 1: Add expected edges from the documented architectures**

`microservices-demo.json` gains (from `kubernetes-manifests/*.yaml` `_ADDR` variables and `protos/demo.proto`):
```json
"edges": [
  {"source":"frontend","target":"adservice","type":"grpc"},
  {"source":"frontend","target":"cartservice","type":"grpc"},
  {"source":"frontend","target":"checkoutservice","type":"grpc"},
  {"source":"frontend","target":"currencyservice","type":"grpc"},
  {"source":"frontend","target":"productcatalogservice","type":"grpc"},
  {"source":"frontend","target":"recommendationservice","type":"grpc"},
  {"source":"frontend","target":"shippingservice","type":"grpc"},
  {"source":"checkoutservice","target":"cartservice","type":"grpc"},
  {"source":"checkoutservice","target":"currencyservice","type":"grpc"},
  {"source":"checkoutservice","target":"emailservice","type":"grpc"},
  {"source":"checkoutservice","target":"paymentservice","type":"grpc"},
  {"source":"checkoutservice","target":"productcatalogservice","type":"grpc"},
  {"source":"checkoutservice","target":"shippingservice","type":"grpc"},
  {"source":"recommendationservice","target":"productcatalogservice","type":"grpc"},
  {"source":"loadgenerator","target":"frontend","type":"http"}
]
```
`robot-shop.json` gains (from `web/default.conf.template`, `docker-compose.yaml` `depends_on`, and each service's source):
```json
"edges": [
  {"source":"web","target":"catalogue","type":"http"},
  {"source":"web","target":"user","type":"http"},
  {"source":"web","target":"cart","type":"http"},
  {"source":"web","target":"shipping","type":"http"},
  {"source":"web","target":"payment","type":"http"},
  {"source":"web","target":"ratings","type":"http"},
  {"source":"cart","target":"catalogue","type":"http"},
  {"source":"cart","target":"redis","type":"database"},
  {"source":"catalogue","target":"mongodb","type":"database"},
  {"source":"user","target":"mongodb","type":"database"},
  {"source":"user","target":"redis","type":"database"},
  {"source":"shipping","target":"mysql","type":"database"},
  {"source":"shipping","target":"cart","type":"http"},
  {"source":"payment","target":"user","type":"http"},
  {"source":"payment","target":"cart","type":"http"},
  {"source":"payment","target":"rabbitmq","type":"event"},
  {"source":"ratings","target":"mysql","type":"database"},
  {"source":"ratings","target":"catalogue","type":"http"},
  {"source":"dispatch","target":"rabbitmq","type":"event"}
]
```
`eshop.json` gains (from `src/eShop.AppHost/Program.cs` `WithReference` and `src/WebApp/Extensions/Extensions.cs`):
```json
"edges": [
  {"source":"WebApp","target":"Basket.API","type":"grpc"},
  {"source":"WebApp","target":"Catalog.API","type":"http"},
  {"source":"WebApp","target":"Ordering.API","type":"http"},
  {"source":"WebhookClient","target":"Webhooks.API","type":"http"}
]
```

- [ ] **Step 2: Extend the corpus test**

`Expected` gains `#[serde(default)] edges: Vec<ExpectedEdge>` with `struct ExpectedEdge { source: String, target: String, #[serde(rename = "type")] edge_type: String }`. For each repo, print `edges total / static / inferred / uncertain / unresolved` next to the timing line. Every expected edge must appear in the output with the same type (confidence not asserted); missing ones are collected into `failures` with the closest edges found for that source (any type, any target) to make the diff readable. Extra edges are printed as `  + source -> target type confidence  file:line  detail` when the env var `CORPUS_VERBOSE` is set. The 500 ms assertion stays.

- [ ] **Step 3: Run and fix recall failures**

Run: `cargo test -p blastradius-core --test corpus -- --nocapture`

Work through every missing edge by reading the corpus source at the place the architecture says the call happens, then fix the extractor or matcher, never the expected file, unless the architecture note was wrong. Likely gaps and where they land: shipping's `application.properties` uses `${DB_HOST:mysql}` style defaults (regex extractor Template with `Var("DB_HOST:mysql")`, resolver `env_default`); dispatch's Go default `"rabbitmq"` is a bare infrastructure name (http matcher bare-name rule); robot-shop `web` has no compose environment, so its nginx edges are Uncertain by name; eShop `Basket.API` is `http://basket-api` inside `AddGrpcClient<Basket.BasketClient>` (grpc matcher `X.YClient` rule with `X` = `Basket`, resolved by normalised name since eShop has protos under `src/Basket.API/Proto/basket.proto`).

- [ ] **Step 4: Run everything, fmt, clippy**

- [ ] **Step 5: Commit**

```bash
git add test/expected crates/blastradius-core/tests/corpus.rs crates/blastradius-core/src
git commit -m "test(corpus): expected http and grpc edges for microservices-demo, robot-shop and eShop"
```

---

### Task B7: Docs

**Files:**
- Modify: `README.md`, `docs/superpowers/specs/2026-09-06-rust-engine-stage2-design.md`

- [ ] **Step 1: README**

Status table row 2 becomes `2. Map | Static edges: HTTP calls, gRPC stubs, plus datastore and broker hosts found on the way | HTTP and gRPC built; events, databases, imports next`. Replace the "What it looks like" block with real output of `cargo run -q -- analyze corpus/robot-shop` including the STRUCTURE, first eight EDGES rows and FINDINGS. Add a section "How mapping works" with the three layers in four short paragraphs and the confidence table's resolution rules in one table.

- [ ] **Step 2: Spec addendum**

Under "Stage 2 architecture", add a paragraph "Decisions made while building" recording: config files in the regex scan; edge type follows the target; scheme-less host:port to a proto owner is gRPC; bare infrastructure names count as hosts; `unresolvedTargets` added to `mapping`.

- [ ] **Step 3: Commit**

```bash
git add README.md docs
git commit -m "docs: mapping stage, real robot-shop output, decisions made while building"
```

---

## Self-review

- Spec coverage: facts layer (B1), matchers http and grpc (B4), resolver table (B3), config resolution through compose, k8s, dotenv (B2), file ownership and skipped-file accounting (B4 `run`), `mapping` block (B4), terminal additions (B5), corpus expected edges for the three named repositories and the 500 ms bound (B6). Events, databases and imports are explicitly deferred; the resolver's target typing gives database and event edges for hosts found through the http matcher, and the corpus expectations use them.
- Placeholders: the fixture file contents are given in full or by exact one-line description; every test is written out; the resolver test has one line marked for replacement with two concrete assertions, done inline in Step 1 of B3.
- Types: `Fact`, `Part`, `Arg`, `Parser`, `Language` (B1) are used unchanged by B3, B4. `Candidate`, `Target` live in `map/mod.rs` and are what `http.rs`, `grpc.rs` produce and `resolve.rs` consumes. `MappingStats` field names match the JSON keys asserted in B4 and the report lines in B5. `ConfigIndex::lookup(service, var)` is the one call the resolver makes into B2.
