# Stage 2: Event, Database and Import Edges Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `blast-radius analyze` reports the three remaining static edge types: `event` edges from producer to consumer joined on a topic, queue or event type; `database` edges between services that share a database; `import` edges from cross-service package, module and project references. Every edge carries file-and-line evidence and a confidence label, and the corpus test asserts the documented ones on robot-shop, eShop, train-ticket and the OpenTelemetry demo.

**Architecture:** Same three layers as the first Stage 2 plan. The facts layer gains one fact, `Setting` (a name bound to a literal: `host: mongodb` in YAML, `EXCHANGE = 'robot-shop'` in code), so configuration values and constants become visible without any module knowing a language. Three new matchers under `map/matchers/` turn facts into candidates. Two of them need a repository-wide join (who else produces this topic, who else uses this database), so `map::run` runs a pre-pass over every file's facts that fills a `Joins` index, and the resolver gains `resolve_all`, which fans one candidate out to every counterpart. A per-service symbol table resolves constants (`self.EXCHANGE`, `Queues.queueName`) to the literals assigned to them.

**Tech Stack:** Rust 1.98 stable via Homebrew rustup, tree-sitter 0.27 with the grammars already in the workspace, rayon, regex, indexmap. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-09-06-rust-engine-stage2-design.md`, sections "Stage 2 architecture" (matchers `event`, `database`, `import`; resolver rows Topic, Database, Package), "Terminal report additions" (shared databases finding) and "Validation" (fixtures `edges-events-app`, `edges-db-app`, `edges-import-app`; corpus expected edges). The previous plan, `docs/superpowers/plans/2026-09-06-stage2-static-edges.md`, built the facts layer, resolver, http and grpc matchers this plan extends.

## Global Constraints

- Toolchain: `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"` before any `cargo` command (cargo is not on the default PATH). Every task ends green on `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings` (pedantic lints are warnings, so they fail clippy) and `cargo test`.
- Facts are language-neutral: no module under `map/` except `map/facts/` may name a language or a language-specific node kind.
- Every edge carries at least one `Evidence` with a file and a line. `detail` says what was matched, in words a reader can check against the file: `publishes "orders", consumed by dispatch`, `shared database mysql/shop with reports`, `ProjectReference ..\EventBus\EventBus.csproj`, `spring.data.mongodb.host=mongodb`.
- Confidence: a topic join, a shared database and a broker known only through its client library are `Inferred`. A package, module or project reference and a configured setting are `Static`. Nothing in this plan produces `Observed`.
- Direction: `event` edges point producer → consumer, as the spec's Stage 4 walks them ("if the changed service publishes a topic, include every consumer"). `database` share edges are emitted in both directions. `import` edges point importer → imported.
- Recall over precision: a candidate that resolves to a discovered service is never dropped; self-edges are dropped; a topic with a producer or consumer but no counterpart is listed in `mapping.unresolvedTargets` as `topic:<key>`; an import that matches no discovered package is dropped silently (it is an external library, not a missing edge).
- The JSON contract does not change. `mapping` keeps its six fields.
- Performance: whole analysis under 500 ms on every corpus repository, asserted by the corpus test. The extra regex pass over every file must not break that; measure with `cargo test --test corpus -- --nocapture`.
- Discovery output must not change: `cargo test --test parity` stays green. The discovery fixtures have no cross-service imports, so the new matchers add no edges there.
- Commit after every task with a conventional-commit subject (`feat(map): ...`, `test(corpus): ...`, `docs: ...`) and a body that says what and why, matching `git log`.

## File Structure

```
crates/blastradius-core/src/map/facts/mod.rs        + Fact::Setting; runs the assignments pass for tree-sitter languages
crates/blastradius-core/src/map/facts/regex.rs      + settings(): key/value, XML element, Dockerfile ENV; assignments(): `name = ... "literal"`
crates/blastradius-core/src/map/facts/treesitter.rs + Go composite literals as calls; annotation values that are symbols; C# Configuration["KEY"]
crates/blastradius-core/src/map/symbols.rs          NEW  Symbols: service -> name -> literals, built from Setting facts; lookup() for `self.X`, `Queues.name`
crates/blastradius-core/src/map/config.rs           + packages read from each service's manifest; dotenv self-interpolation; compose `- KEY` pass-through
crates/blastradius-core/src/map/resolve.rs          + Joins, TopicSides, Resolved.source, resolve_all(), Topic/Broker/Database/Package/PackagePath, is_hostish_key(), ado_connection()
crates/blastradius-core/src/map/matchers/mod.rs     + FileContext.symbols; all() registers event, database, import
crates/blastradius-core/src/map/matchers/http.rs    + Setting handling, ADO.NET connection strings
crates/blastradius-core/src/map/matchers/event.rs   NEW  mentions(), roles, topic keys, broker families, topic_index()
crates/blastradius-core/src/map/matchers/database.rs NEW keys(), database_index(), key_from_url(), key_from_connection_string()
crates/blastradius-core/src/map/matchers/import.rs  NEW  imports, manifest dependencies, project references
crates/blastradius-core/src/map/mod.rs              + Target variants, TopicRole, owner_of_path(); run() builds symbols and joins; resolve_all; source override
crates/blastradius-core/src/report.rs               + Shared databases finding; wording of the no-edges line
crates/blastradius-core/tests/edges.rs              + one test per fixture, report test
test/fixtures/edges-events-app/                     compose + python, go, js, cs, java services; rabbitmq and kafka images
test/fixtures/edges-db-app/                         compose + .env + go, js, java, python, cs services; mongo, mysql, postgres, valkey images
test/fixtures/edges-import-app/                     monorepo: apps/, packages/, services/ with js, go, cs, java, rust, python members
test/expected/corpus/{robot-shop,eshop,train-ticket,opentelemetry-demo}.json  + edges
README.md                                           Stage 2 built; mapping section; sample output
docs/superpowers/specs/2026-09-06-rust-engine-stage2-design.md  + decisions made while building
```

---

### Task 1: Setting facts, assignments, and three tree-sitter refinements

**Files:**
- Modify: `crates/blastradius-core/src/map/facts/mod.rs` (Fact enum, `extract`, tests)
- Modify: `crates/blastradius-core/src/map/facts/regex.rs` (new regexes, `settings`, `assignments`)
- Modify: `crates/blastradius-core/src/map/facts/treesitter.rs` (GO table, `args_of`, `on_annotation`, C# element access, `walk`)
- Modify: `crates/blastradius-core/src/map/matchers/http.rs:151` (exhaustive match gains `Fact::Setting { .. }`)

**Interfaces:**
- Produces:
  ```rust
  // facts/mod.rs
  Fact::Setting { key: String, value: String, line: u32 }
  // A name bound to a literal. Config files: `host: mongodb`, `spring.data.mongodb.host=mongodb`,
  // `<artifactId>ts-common</artifactId>`, `ENV REDIS_HOST redis`. Code: `EXCHANGE = 'robot-shop'`,
  // `public final static String queueName = "email";`. Quotes around the value are removed.

  // facts/regex.rs
  pub(super) fn settings(text: &str) -> Vec<Fact>      // config-file shapes only
  pub(super) fn assignments(text: &str) -> Vec<Fact>   // `name = ... "literal"` in any language
  ```
- `facts::extract` appends `regex::assignments(text)` to tree-sitter facts; `regex::extract` runs both `settings` and `assignments` per line and drops exact duplicates.
- Go composite literals become `Fact::Call { callee: "sarama.ProducerMessage", args: [Str("orders"), ...] }`.
- Java and C# annotation arguments that are identifiers (`queues = Queues.queueName`) become `Arg::Other("Queues.queueName")`.
- C# `builder.Configuration["VALKEY_ADDR"]` becomes `Fact::EnvRef { name: "VALKEY_ADDR", default: None }`.

- [ ] **Step 1: Write the failing tests in `facts/mod.rs`**

Add inside `mod tests`, after the existing helpers, a `settings` helper and five tests:

```rust
    fn settings(facts: &[Fact]) -> Vec<(&str, &str)> {
        facts
            .iter()
            .filter_map(|f| match f {
                Fact::Setting { key, value, .. } => Some((key.as_str(), value.as_str())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn settings_from_config_files() {
        let yml = extract(
            "a/src/main/resources/application.yml",
            "spring:\n  data:\n    mongodb:\n      host: ts-order-mongo\n      database: ts-order\n  kafka:\n    bootstrap-servers: kafka:9092\n# host: commented-out\nurl: \"jdbc:mysql://mysql:3306/x\"\n",
        )
        .unwrap();
        assert_eq!(
            settings(&yml.facts),
            vec![
                ("host", "ts-order-mongo"),
                ("database", "ts-order"),
                ("bootstrap-servers", "kafka:9092"),
                ("url", "jdbc:mysql://mysql:3306/x"),
            ]
        );
        let props = extract("a/application.properties", "spring.data.mongodb.host=ts-order-mongo\n").unwrap();
        assert_eq!(settings(&props.facts), vec![("spring.data.mongodb.host", "ts-order-mongo")]);
        let xml = extract(
            "a/pom.xml",
            "<project>\n  <artifactId>ts-order-service</artifactId>\n  <dependency>\n    <groupId>ts</groupId>\n    <artifactId>ts-common</artifactId>\n  </dependency>\n  <ProjectReference Include=\"..\\X\\X.csproj\" />\n</project>\n",
        )
        .unwrap();
        assert_eq!(
            settings(&xml.facts),
            vec![("artifactId", "ts-order-service"), ("groupId", "ts"), ("artifactId", "ts-common")]
        );
        let docker = extract("a/Dockerfile", "FROM x\nENV CATALOGUE_HOST catalogue\nENV REDIS_HOST=redis\nARG TAG=\"1.0\"\n").unwrap();
        assert_eq!(
            settings(&docker.facts),
            vec![("CATALOGUE_HOST", "catalogue"), ("REDIS_HOST", "redis"), ("TAG", "1.0")]
        );
    }

    #[test]
    fn assignments_bind_names_to_literals_in_every_language() {
        let py = extract("a/rabbitmq.py", "class Publisher:\n    EXCHANGE='robot-shop'\n    ROUTING_KEY = 'orders'\n    def go(self):\n        self.topic = os.getenv('KAFKA_TOPIC', 'orders')\n        if a == 'x':\n            pass\n        y = foo('a').bar()\n").unwrap();
        assert_eq!(
            settings(&py.facts),
            vec![("EXCHANGE", "robot-shop"), ("ROUTING_KEY", "orders"), ("topic", "orders")]
        );
        let java = extract("a/Queues.java", "public class Queues {\n    public final static String queueName = \"email\";\n}\n").unwrap();
        assert_eq!(settings(&java.facts), vec![("queueName", "email")]);
        let cs = extract("a/Bus.cs", "class Bus {\n    private const string ExchangeName = \"eshop_event_bus\";\n    private static readonly string TopicName = Environment.GetEnvironmentVariable(\"KAFKA_TOPIC\") ?? \"orders\";\n}\n").unwrap();
        assert_eq!(settings(&cs.facts), vec![("ExchangeName", "eshop_event_bus"), ("TopicName", "orders")]);
        let go = extract("a/p.go", "package p\nconst KafkaTopic = \"orders\"\nvar Topic = getTopic()\n").unwrap();
        assert_eq!(settings(&go.facts), vec![("KafkaTopic", "orders")]);
        let js = extract("a/p.js", "const topic = 'orders';\nlet n = 3;\nx => 'y';\n").unwrap();
        assert_eq!(settings(&js.facts), vec![("topic", "orders")]);
        let kt = extract("a/main.kt", "val topic: String = System.getenv(\"KAFKA_TOPIC\") ?: \"orders\"\nconst val groupID = \"fraud-detection\"\n").unwrap();
        assert_eq!(kt.parser, Parser::Regex);
        assert_eq!(settings(&kt.facts), vec![("topic", "orders"), ("groupID", "fraud-detection")]);
        let php = extract("a/p.php", "<?php\n$topic = 'orders';\n").unwrap();
        assert_eq!(settings(&php.facts), vec![("topic", "orders")]);
    }

    #[test]
    fn go_composite_literals_are_calls() {
        let go = extract("a/main.go", "package main\nfunc f() {\n\tmsg := &sarama.ProducerMessage{Topic: \"orders\", Value: sarama.StringEncoder(\"x\")}\n\tcg.Consume(ctx, []string{\"orders\"}, h)\n}\n").unwrap();
        let Fact::Call { args, .. } = go
            .facts
            .iter()
            .find(|f| matches!(f, Fact::Call { callee, .. } if callee == "sarama.ProducerMessage"))
            .unwrap_or_else(|| panic!("{:?}", go.facts))
        else {
            unreachable!()
        };
        assert_eq!(args[0], Arg::Str("orders".into()));
        assert!(calls(&go.facts).contains(&"cg.Consume"));
    }

    #[test]
    fn annotation_values_that_are_symbols_become_other_args() {
        let java = extract("a/R.java", "class R {\n  @RabbitListener(queues = Queues.queueName)\n  void a() {}\n  @KafkaListener(topics = \"orders\", groupId = \"g\")\n  void b() {}\n}\n").unwrap();
        let args_of = |name: &str| -> Vec<Arg> {
            java.facts
                .iter()
                .find_map(|f| match f {
                    Fact::Annotation { name: n, args, .. } if n == name => Some(args.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{:?}", java.facts))
        };
        assert_eq!(args_of("RabbitListener"), vec![Arg::Other("Queues.queueName".into())]);
        assert_eq!(args_of("KafkaListener"), vec![Arg::Str("orders".into()), Arg::Str("g".into())]);
        let cs = extract("a/F.cs", "class F {\n  [ServiceBusTrigger(Queues.Name)]\n  void a() {}\n}\n").unwrap();
        assert!(
            cs.facts.iter().any(|f| matches!(f, Fact::Annotation { name, args, .. } if name == "ServiceBusTrigger" && args == &[Arg::Other("Queues.Name".into())])),
            "{:?}",
            cs.facts
        );
    }

    #[test]
    fn csharp_configuration_index_is_an_env_ref() {
        let cs = extract("a/Program.cs", "string valkeyAddress = builder.Configuration[\"VALKEY_ADDR\"];\nvar x = Configuration[\"REDIS_ADDR\"] ?? \"redis:6379\";\nvar y = dict[\"k\"];\n").unwrap();
        assert_eq!(envs(&cs.facts), vec![("VALKEY_ADDR", None), ("REDIS_ADDR", Some("redis:6379"))]);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `export PATH="/opt/homebrew/opt/rustup/bin:$PATH" && cargo test -p blastradius-core --lib facts 2>&1 | tail -30`
Expected: compile error `no variant named Setting` (the helper references `Fact::Setting`).

- [ ] **Step 3: Add the fact and the regex passes**

In `facts/mod.rs`, add to `enum Fact` after `EnvRef`:

```rust
    /// A name bound to a literal value. Configuration files: `host: mongodb`,
    /// `spring.data.mongodb.host=mongodb`, `<artifactId>ts-common</artifactId>`,
    /// `ENV REDIS_HOST redis`. Code: `EXCHANGE = 'robot-shop'`,
    /// `public final static String queueName = "email";`. Quotes are removed.
    Setting {
        key: String,
        value: String,
        line: u32,
    },
```

Add `| Self::Setting { line, .. }` to `Fact::line`. In `extract`, the tree-sitter branch becomes:

```rust
    if language != Language::Other {
        if let Some(mut facts) = treesitter::extract(language, text) {
            facts.extend(regex::assignments(text));
            return Some(Extraction {
                facts,
                parser: Parser::TreeSitter,
                language: label_for(language).to_string(),
            });
        }
    }
```

In `facts/regex.rs`, add the regexes after `IMPORT`:

```rust
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
    Regex::new(r#"^\s*(?:ENV|ARG)\s+([A-Za-z_]\w*)(?:\s*=\s*|\s+)(?:"([^"]*)"|'([^']*)'|(\S+))\s*$"#)
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
```

Add the two functions and wire them into `extract`:

```rust
pub(super) fn extract(text: &str) -> Vec<Fact> {
    let mut facts = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = u32::try_from(i + 1).unwrap_or(u32::MAX);
        let trimmed = raw.trim_start();
        if trimmed.starts_with('#') && !trimmed.starts_with("#{") || trimmed.starts_with("//") {
            continue;
        }
        extract_line(raw, line, &mut facts);
        let mut bound = setting_line(raw, line);
        if let Some(assigned) = assignment_line(raw, line) {
            if bound.as_ref() != Some(&assigned) {
                bound = bound.or(Some(assigned));
            }
        }
        facts.extend(bound);
    }
    facts
}

/// Key/value settings from configuration files, one per line at most.
pub(super) fn settings(text: &str) -> Vec<Fact> {
    text.lines()
        .enumerate()
        .filter_map(|(i, raw)| setting_line(raw, u32::try_from(i + 1).unwrap_or(u32::MAX)))
        .collect()
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
    let rest = caps[2].trim_end().trim_end_matches([';', ',', ')', ']', '}']).trim_end();
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
```

Note `setting_line` runs the Dockerfile shape first because `ENV KEY=value` also matches the key/value shape with key `ENV KEY`... it does not (spaces are not allowed in a key), but the explicit order keeps the intent readable.

- [ ] **Step 4: Refine the tree-sitter walk**

In `treesitter.rs`, GO table: `calls: &["call_expression", "composite_literal"]`, `args: &["argument_list", "literal_value"]`. In `args_of`, unwrap `keyed_element` and `literal_element` as well:

```rust
    fn args_of(&self, args: Node<'_>) -> Vec<Arg> {
        let mut out = Vec::new();
        let mut cursor = args.walk();
        for child in args.named_children(&mut cursor) {
            out.push(self.arg_of(Self::unwrap_argument(child)));
        }
        out
    }

    /// `argument`, `keyword_argument`, `named_argument` and Go's
    /// `keyed_element` wrap the value they pass; `literal_element` wraps once
    /// more. The value is the last named child at each level.
    fn unwrap_argument(node: Node<'_>) -> Node<'_> {
        let mut node = node;
        for _ in 0..3 {
            match node.kind() {
                "argument" | "keyword_argument" | "named_argument" | "keyed_element"
                | "literal_element" => {
                    let mut cursor = node.walk();
                    match node.named_children(&mut cursor).last() {
                        Some(inner) => node = inner,
                        None => return node,
                    }
                }
                _ => return node,
            }
        }
        node
    }
```

In `on_annotation`, after computing `args` from `string_args_within(inner)`, add the identifier values. Replace the `Some(inner) => self.string_args_within(inner),` arm with:

```rust
            Some(inner) => {
                let mut args = self.string_args_within(inner);
                args.extend(self.symbol_args_within(inner));
                args
            }
```

and add the method next to `string_args_within`:

```rust
    /// Annotation values that are identifiers or member paths, not strings:
    /// `queues = Queues.queueName`, `[Trigger(Queues.Name)]`. Kept as Other
    /// so a matcher can look the symbol up.
    fn symbol_args_within(&self, node: Node<'_>) -> Vec<Arg> {
        let mut out = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            let value = match child.kind() {
                "element_value_pair" | "attribute_argument" => {
                    let mut inner = child.walk();
                    child.named_children(&mut inner).last()
                }
                _ => None,
            };
            let Some(value) = value else { continue };
            let text = self.text(value).trim();
            let is_symbol = !text.is_empty()
                && text
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$'))
                && text.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
            if is_symbol {
                out.push(Arg::Other(text.to_string()));
            }
        }
        out
    }
```

In `walk`, add a C# branch before the JavaScript one:

```rust
        } else if self.language == Language::CSharp && kind == "element_access_expression" {
            self.on_cs_configuration(node);
        } else if self.language == Language::JavaScript
```

and the method, next to `on_js_env`:

```rust
    /// `Configuration["KEY"]`, `builder.Configuration["KEY"]`: .NET reads
    /// environment variables through configuration.
    fn on_cs_configuration(&mut self, node: Node<'_>) {
        let text = self.text(node);
        let Some((receiver, index)) = text.split_once('[') else {
            return;
        };
        let receiver = receiver.trim().to_lowercase();
        if !(receiver.ends_with("configuration") || receiver.ends_with("config")) {
            return;
        }
        let Some(name) = unquote(index.trim_end_matches(']')) else {
            return;
        };
        if !super::is_var_name(&name) {
            return;
        }
        let default = self.default_after(node);
        self.facts.push(Fact::EnvRef {
            name,
            default,
            line: Self::line(node),
        });
    }
```

`default_after` accepts `binary_expression`; C# `??` parses as `binary_expression`, so `Configuration["REDIS_ADDR"] ?? "redis:6379"` yields the default.

In `matchers/http.rs`, the last match arm becomes `Fact::Annotation { .. } | Fact::Import { .. } | Fact::Extends { .. } | Fact::Setting { .. } => {}`.

- [ ] **Step 5: Run the facts tests until they pass**

Run: `cargo test -p blastradius-core --lib facts 2>&1 | tail -40`
Expected: all `facts::tests` pass, including the five new ones and the four existing ones. If `go_composite_literals_are_calls` fails with `Other("\"orders\"")`, the value node is wrapped once more than expected: print `node.kind()` chain in `unwrap_argument` and extend the match with the kind you see. If `assignments_bind_names_to_literals_in_every_language` picks up `("a", ...)` from `if a == 'x'`, the `[^=>\s]` guard is missing.

- [ ] **Step 6: Run the whole suite, format, lint**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E '^test result|FAILED|panicked' `
Expected: every `test result: ok`. The existing `http_edges_from_literals_env_templates_and_config_files` must still pass: Setting facts are ignored by every matcher until Task 2.

- [ ] **Step 7: Commit**

```bash
git add crates/blastradius-core/src/map/facts crates/blastradius-core/src/map/matchers/http.rs
git commit -m "feat(facts): settings, assignments, Go struct literals, annotation symbols, .NET configuration reads" -m "A Setting fact binds a name to a literal: key/value lines, XML elements and Dockerfile ENV in configuration files, and name = \"literal\" assignments in every language, so constants such as EXCHANGE = 'robot-shop' and Queues.queueName = \"email\" become visible to matchers. Go composite literals are calls with their keyed values as arguments, so sarama.ProducerMessage{Topic: \"orders\"} names its topic. Annotation values that are identifiers are kept as Other arguments. builder.Configuration[\"KEY\"] is an environment reference."
```

---

### Task 2: Settings reach the resolver, and configured values reach the code that reads them

Three gaps the corpus showed. Spring's `spring.data.mongodb.host: ts-order-mongo` names a datastore without a URL. The OpenTelemetry demo's cart reads `VALKEY_ADDR` whose compose entry is the bare `- VALKEY_ADDR` (value taken from `.env`, where it is `valkey-cart:${VALKEY_PORT}`, a reference to another dotenv line). Its product catalog reads `DB_CONNECTION_STRING`, a name no hostish suffix covers. This task also creates the `edges-db-app` fixture in full; Task 5 adds tests on it for shared databases.

**Files:**
- Modify: `crates/blastradius-core/src/map/resolve.rs` (`HOSTISH_SUFFIXES`, `is_hostish_key`, `ado_connection`, tests)
- Modify: `crates/blastradius-core/src/map/matchers/http.rs` (`from_setting`, ADO.NET strings, tests)
- Modify: `crates/blastradius-core/src/discover/env.rs` (`load_compose_env` interpolates against itself)
- Modify: `crates/blastradius-core/src/map/config.rs` (`read_dotenv` interpolation, `read_environment_node` bare keys, tests)
- Create: `test/fixtures/edges-db-app/**` (listed below)
- Modify: `crates/blastradius-core/tests/edges.rs` (new test)

**Interfaces:**
- Produces:
  ```rust
  // resolve.rs
  pub fn is_hostish_key(key: &str) -> bool          // `spring.data.mongodb.host`, `bootstrap-servers`, `DB_CONNECTION_STRING`
  pub fn ado_connection(text: &str) -> Option<(String, String)>  // ("host", "database"), both lowercased, from `Host=x;Database=y;...`
  ```
- `Fact::Setting` with a hostish key and a hostname or `host:port` value yields `Target::Host` / `Target::HostPort` with detail `key=value`.
- `ConfigIndex::lookup("cart", "VALKEY_ADDR")` returns `valkey-cart:6379` for compose `environment: [VALKEY_ADDR]` plus `.env` lines `VALKEY_PORT=6379` and `VALKEY_ADDR=valkey-cart:${VALKEY_PORT}`.

- [ ] **Step 1: Create the fixture**

```
test/fixtures/edges-db-app/docker-compose.yml
services:
  catalogue:
    build: ./catalogue
  user:
    build: ./user
  orders:
    build: ./orders
  reports:
    build: ./reports
    environment:
      - DB_CONNECTION_STRING=postgres://app:${POSTGRES_PASSWORD}@${POSTGRES_HOST}/shop?sslmode=disable
  cart:
    build: ./cart
    environment:
      - VALKEY_ADDR
  ledger:
    build: ./ledger
  audit:
    build: ./audit
  inventory:
    build: ./inventory
  mongodb:
    image: mongo:6
  mysql:
    image: mysql:8
  postgres:
    image: postgres:16
  valkey-cart:
    image: valkey/valkey:8

test/fixtures/edges-db-app/.env
POSTGRES_HOST=postgres
POSTGRES_PASSWORD=secret
VALKEY_PORT=6379
VALKEY_ADDR=valkey-cart:${VALKEY_PORT}

test/fixtures/edges-db-app/catalogue/go.mod
module catalogue

go 1.22

test/fixtures/edges-db-app/catalogue/main.go
package main

func main() {
	mongo := "mongodb://mongodb:27017/catalogue"
	_ = mongo
}

test/fixtures/edges-db-app/user/package.json
{ "name": "user", "main": "server.js" }

test/fixtures/edges-db-app/user/server.js
const url = process.env.MONGO_URL || 'mongodb://mongodb:27017/users';

test/fixtures/edges-db-app/orders/pom.xml
<project><artifactId>orders</artifactId></project>

test/fixtures/edges-db-app/orders/src/main/resources/application.yml
spring:
  datasource:
    url: jdbc:mysql://${DB_HOST:mysql}:3306/${DB_NAME:shop}?useSSL=false

test/fixtures/edges-db-app/reports/requirements.txt
psycopg2

test/fixtures/edges-db-app/reports/report.py
import os
conn = os.getenv("DB_CONNECTION_STRING")
legacy = os.getenv("LEGACY_DB_URL", "mysql://mysql:3306/shop")

test/fixtures/edges-db-app/cart/cart.csproj
<Project Sdk="Microsoft.NET.Sdk.Web"></Project>

test/fixtures/edges-db-app/cart/Program.cs
var builder = WebApplication.CreateBuilder(args);
string valkeyAddress = builder.Configuration["VALKEY_ADDR"];

test/fixtures/edges-db-app/ledger/ledger.csproj
<Project Sdk="Microsoft.NET.Sdk.Web"></Project>

test/fixtures/edges-db-app/ledger/Program.cs
builder.AddNpgsqlDbContext<LedgerContext>("ledgerdb");

test/fixtures/edges-db-app/ledger/appsettings.Development.json
{ "ConnectionStrings": { "ledgerdb": "Host=localhost;Database=LedgerDB;Username=postgres;Password=x" } }

test/fixtures/edges-db-app/audit/audit.csproj
<Project Sdk="Microsoft.NET.Sdk.Web"></Project>

test/fixtures/edges-db-app/audit/Program.cs
builder.AddNpgsqlDataSource("ledgerdb");
builder.Services.AddDbContext<AuditContext>(o => o.UseNpgsql(builder.Configuration.GetConnectionString("ledgerdb")));

test/fixtures/edges-db-app/audit/appsettings.Development.json
{ "ConnectionStrings": { "postgres": "Host=localhost;Username=postgres;Password=x;Database=LedgerDB" } }

test/fixtures/edges-db-app/inventory/pom.xml
<project><artifactId>inventory</artifactId></project>

test/fixtures/edges-db-app/inventory/src/main/resources/application.properties
spring.data.mongodb.host=mongodb
spring.data.mongodb.database=inventory
```

Create each file with exactly that content (one `cat > path <<'EOF'` per file, or the Write tool). `go.mod` and the Go file use a tab before `mongo`.

- [ ] **Step 2: Write the failing tests**

In `resolve.rs` tests, add to `templates_and_protos` (or a new test `hostish_keys_and_connection_strings`):

```rust
    #[test]
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
```

In `matchers/http.rs`, add a test module at the end of the file:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::config::ConfigIndex;
    use crate::map::symbols::Symbols;
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
        let symbols = Symbols::default();
        let ctx = FileContext {
            service: "inventory",
            file: "inventory/src/main/resources/application.properties",
            facts: &facts,
            config: &cfg,
            symbols: &symbols,
        };
        let out = Http.candidates(&ctx);
        let targets: Vec<(&Target, &str)> = out
            .iter()
            .map(|c| (&c.target, c.evidence.detail.as_deref().unwrap()))
            .collect();
        assert_eq!(
            targets,
            vec![
                (&Target::Host("mongodb".into()), "spring.data.mongodb.host=mongodb"),
                (&Target::HostPort("kafka:9092".into()), "bootstrap-servers=kafka:9092"),
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
        let symbols = Symbols::default();
        let ctx = FileContext {
            service: "ledger",
            file: "ledger/appsettings.json",
            facts: &facts,
            config: &cfg,
            symbols: &symbols,
        };
        let out = Http.candidates(&ctx);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].target, Target::Host("sql".into()));
    }
}
```

`FileContext.symbols` and `crate::map::symbols::Symbols` do not exist until Task 4. For this task, leave those two lines out of the tests and add them in Task 4 when `FileContext` gains the field. (The test bodies above show the final shape so Task 4 knows what to add.)

In `config.rs` tests, add:

```rust
    #[test]
    fn dotenv_values_interpolate_and_bare_compose_keys_pass_through() {
        let cfg = build("edges-db-app");
        assert_eq!(
            cfg.lookup("cart", "VALKEY_ADDR").map(|v| v.value.as_str()),
            Some("valkey-cart:6379")
        );
        assert_eq!(
            cfg.lookup("cart", "VALKEY_ADDR").map(|v| v.evidence.file.as_str()),
            Some(".env")
        );
        assert_eq!(
            cfg.lookup("reports", "DB_CONNECTION_STRING")
                .map(|v| v.value.as_str()),
            Some("postgres://app:secret@postgres/shop?sslmode=disable")
        );
        assert_eq!(
            cfg.lookup("nobody", "VALKEY_ADDR").map(|v| v.value.as_str()),
            Some("valkey-cart:6379"),
            "dotenv values are global and interpolated against their own file"
        );
    }
```

In `tests/edges.rs`, add:

```rust
#[test]
fn database_edges_from_settings_connection_strings_and_dotenv() {
    let (edges, json) = edges_of("edges-db-app");
    let e = find(&edges, "inventory", "mongodb", EdgeType::Database);
    assert_eq!(e.confidence, Confidence::Static);
    assert_eq!(
        e.evidence[0].detail.as_deref(),
        Some("spring.data.mongodb.host=mongodb")
    );
    let e = find(&edges, "cart", "valkey-cart", EdgeType::Database);
    assert_eq!(e.confidence, Confidence::Static);
    assert!(
        e.evidence[0]
            .detail
            .as_deref()
            .unwrap()
            .starts_with("VALKEY_ADDR=valkey-cart:6379 via .env:"),
        "{:?}",
        e.evidence
    );
    let e = find(&edges, "reports", "postgres", EdgeType::Database);
    assert!(
        e.evidence[0]
            .detail
            .as_deref()
            .unwrap()
            .starts_with("DB_CONNECTION_STRING=postgres://app:secret@postgres/shop"),
        "{:?}",
        e.evidence
    );
    find(&edges, "reports", "mysql", EdgeType::Database);
    find(&edges, "orders", "mysql", EdgeType::Database);
    find(&edges, "catalogue", "mongodb", EdgeType::Database);
    find(&edges, "user", "mongodb", EdgeType::Database);
    assert!(
        !edges.iter().any(|e| e.target == "localhost"),
        "{:?}",
        edges.iter().map(triple).collect::<Vec<_>>()
    );
    assert_eq!(json.mapping.unresolved_targets, Vec::<String>::new(), "{:?}", json.mapping.unresolved_targets);
}
```

The last assertion may need adjusting once the fixture runs (for instance `MONGO_URL` from `user/server.js` resolves through its default, so it is not unresolved). Keep whatever the honest list is, but it must not contain `VALKEY_ADDR`, `DB_CONNECTION_STRING`, `VALKEY_PORT` or `localhost`.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p blastradius-core 2>&1 | grep -E 'error\[|FAILED|panicked|test result' | head`
Expected: compile errors for `is_hostish_key`, `ado_connection`; after stubbing them, `database_edges_from_settings_connection_strings_and_dotenv` fails on `inventory -> mongodb`.

- [ ] **Step 4: Resolver helpers**

In `resolve.rs`, extend `HOSTISH_SUFFIXES` (keep the longest suffixes first, as the list is today) by inserting at the top:

```rust
    "_CONNECTION_STRING",
    "_CONNECTIONSTRING",
    "_BOOTSTRAP_SERVERS",
    "_CONNECTION",
    "_BROKERS",
    "_DSN",
```

Add after `is_hostish_var`:

```rust
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
```

The `is_hostish_key("eureka.client.serviceUrl.defaultZone")` case is false because `defaultzone` is not in the list; its URL value is found through `BARE_URL` anyway.

- [ ] **Step 5: The http matcher reads settings and connection strings**

In `matchers/http.rs`, import `ado_connection, is_hostish_key` from `crate::map::resolve`, and add:

```rust
/// A configuration key that names a host: `spring.data.mongodb.host: mongodb`,
/// `bootstrap-servers: kafka:9092`, Dockerfile `ENV REDIS_HOST redis`. URLs and
/// `${VAR}` values are found by the string and template paths instead.
fn from_setting(ctx: &FileContext<'_>, key: &str, value: &str, line: u32, out: &mut Vec<Candidate>) {
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
```

Import `is_hostname` from `crate::map::resolve` too. In `from_string`, before the `PDO_DSN` branch:

```rust
    let target = if let Some((host, _)) = ado_connection(v) {
        Target::Host(host)
    } else if let Some(dsn) = PDO_DSN.captures(v) {
```

In `candidates`, replace the ignore arm with:

```rust
                Fact::Setting { key, value, line } => from_setting(ctx, key, value, *line, &mut out),
                Fact::Annotation { .. } | Fact::Import { .. } | Fact::Extends { .. } => {}
```

- [ ] **Step 6: Configuration completeness**

In `discover/env.rs`, `load_compose_env` interpolates each value against the lines already read:

```rust
/// `.env` beside the compose file wins over `.env.example`. A value may refer
/// to an earlier line (`VALKEY_ADDR=valkey-cart:${VALKEY_PORT}`), as compose
/// resolves it.
pub fn load_compose_env(compose_dir: &Path) -> Env {
    let mut env = Env::new();
    for file in [".env.example", ".env"] {
        if let Some(text) = read_text(&compose_dir.join(file)) {
            for (key, value) in parse_dotenv(&text) {
                let value = interpolate(&value, &env);
                env.insert(key, value);
            }
        }
    }
    env
}
```

Add a unit test in `env.rs` tests:

```rust
    #[test]
    fn dotenv_lines_may_reference_earlier_lines() {
        let env: Env = [("VALKEY_PORT", "6379")]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        assert_eq!(interpolate("valkey-cart:${VALKEY_PORT}", &env), "valkey-cart:6379");
    }
```

(`load_compose_env` itself is covered by the `config.rs` fixture test.)

In `map/config.rs`, `read_dotenv` interpolates the same way:

```rust
    fn read_dotenv(&mut self, root: &Path) {
        let mut seen = Env::new();
        for file in [".env.example", ".env"] {
            let Some(text) = read_text(&root.join(file)) else {
                continue;
            };
            for (var, value, line) in parse_dotenv_lines(&text) {
                let value = interpolate(&value, &seen);
                seen.insert(var.clone(), value.clone());
                self.global.insert(
                    var,
                    EnvValue {
                        value,
                        evidence: Evidence {
                            file: file.to_string(),
                            line: Some(line),
                            detail: None,
                        },
                    },
                );
            }
        }
    }
```

In `read_environment_node`, the list branch takes a bare key from the compose environment:

```rust
        for item in node.items() {
            let Some(text) = item.as_scalar_string() else {
                continue;
            };
            let (var, value) = match text.split_once('=') {
                Some((var, value)) => (var.trim().to_string(), interpolate(value.trim(), env)),
                None => {
                    let var = text.trim();
                    let Some(value) = env.get(var) else {
                        continue;
                    };
                    (var.to_string(), value.clone())
                }
            };
            self.insert_service(
                service,
                var,
                EnvValue {
                    value,
                    evidence: Evidence {
                        file: file.to_string(),
                        line: Some(item.line),
                        detail: None,
                    },
                },
            );
        }
```

The evidence for a bare key is the compose line, which is where the pass-through is declared. The `cart -> valkey-cart` test above expects `via .env:` because the value is actually recorded twice: once for `cart` from the compose pass-through and once globally from `.env`. `lookup` prefers the service entry, so the detail will read `via docker-compose.yml:<line>`. Change that assertion to `starts_with("VALKEY_ADDR=valkey-cart:6379 via docker-compose.yml:")` and, in the `config.rs` test, the `evidence.file` for `cart` to `"docker-compose.yml"`.

- [ ] **Step 7: Run everything**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E '^test result|FAILED|panicked'`
Expected: all green, including `parity` (the discovery fixtures' dotenv files have no `${...}` references, so their output is unchanged) and `corpus`. Then run `CORPUS_VERBOSE=1 cargo test --test corpus -- --nocapture 2>&1 | grep -E '^\S+ +[0-9]+ code|valkey-cart|astronomy-db|-> .* database'` and confirm `cart -> valkey-cart database static` and `product-catalog -> astronomy-db database static` appear for opentelemetry-demo, and that train-ticket gains `ts-*-service -> ts-*-mysql` edges only where they are real (the `spring.datasource.url` templates were already resolving; new edges come from `spring.rabbitmq.host` and similar settings). Look at every new edge; anything pointing at a plain code service through a setting key such as `server` or `nodes` is a false positive to fix by tightening `HOSTISH_KEYS`.

- [ ] **Step 8: Commit**

```bash
git add crates/blastradius-core/src test/fixtures/edges-db-app
git commit -m "feat(map): settings name hosts; dotenv and compose pass-through values reach the resolver" -m "A configuration setting whose key says it holds a host (spring.data.mongodb.host, bootstrap-servers, ENV REDIS_HOST) is a Static candidate for the value. ADO.NET connection strings name their host. _CONNECTION_STRING and _DSN are hostish suffixes. Dotenv values interpolate against earlier lines of the same file, and a compose environment entry without a value takes the value the .env beside it declares, which is how the OpenTelemetry demo wires VALKEY_ADDR. The edges-db-app fixture covers each shape."
```

---

### Task 3: Resolver joins and fan-out

The resolver learns five targets. Three need repository-wide knowledge the pre-pass supplies through `Joins`: who produces and consumes each topic, who uses each database key, who owns each proto service (moved in from the bare `HashMap`). Two need the package index `ConfigIndex` will hold from Task 6; this task adds the field and resolution so Task 6 only has to fill it.

**Files:**
- Modify: `crates/blastradius-core/src/map/mod.rs` (`Target`, `TopicRole`, `owner_of_path`, `collect_outcomes`, `partition_files`, `run`)
- Modify: `crates/blastradius-core/src/map/resolve.rs` (`Joins`, `TopicSides`, `Resolved.source`, `Resolver::new`, `resolve_all`, five resolutions, tests)
- Modify: `crates/blastradius-core/src/map/config.rs` (`Package`, `packages` field)

**Interfaces:**
- Produces:
  ```rust
  // map/mod.rs
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum TopicRole { Producer, Consumer, Unknown }
  // new Target variants
  Target::Topic { key: String, role: TopicRole }
  Target::Broker { family: String, how: String }       // "rabbitmq", `imports pika (rabbitmq client)`
  Target::Database(String)                             // `mysql/shop`, `ledgerdb`
  Target::Package { name: String, how: String }        // `@acme/shared/utils`, `import @acme/shared/utils`
  Target::PackagePath { path: String, how: String }    // `src/EventBus/EventBus.csproj`, `ProjectReference ..\EventBus\EventBus.csproj`
  /// The code service whose root is the longest prefix of `path`.
  pub fn owner_of_path<'a>(services: &'a [Service], path: &str) -> Option<&'a str>

  // resolve.rs
  #[derive(Debug, Default, Clone, PartialEq, Eq)]
  pub struct TopicSides { pub producers: BTreeSet<String>, pub consumers: BTreeSet<String>, pub unknown: BTreeSet<String> }
  #[derive(Debug, Default)]
  pub struct Joins {
      pub proto_owner: HashMap<String, String>,
      pub topics: HashMap<String, TopicSides>,
      pub databases: HashMap<String, BTreeSet<String>>,
  }
  pub struct Resolved { pub target, pub edge_type, pub confidence, pub detail, pub source: Option<String> }
  impl Resolver { pub fn new(services, config, joins: Joins) -> Self;
                  pub fn resolve_all(&self, source: &str, candidate: &Candidate) -> Vec<Result<Resolved, Unresolved>>; }

  // config.rs
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct Package { pub name: String, pub service: String, pub evidence: Evidence }
  pub struct ConfigIndex { ..., pub packages: Vec<Package> }
  ```
- Consumes: `Service`, `ConfigIndex`, `normalise`, `image_basename` as today.

- [ ] **Step 1: Write the failing tests in `resolve.rs`**

Change the two existing `Resolver::new(&s, &cfg, HashMap::new())` calls to `Resolver::new(&s, &cfg, Joins::default())` and the `owner` one to `Resolver::new(&s, &cfg, Joins { proto_owner: owner, ..Joins::default() })`. Add:

```rust
    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
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
        let topic = |key: &str, role: TopicRole| {
            Candidate {
                target: Target::Topic {
                    key: key.into(),
                    role,
                },
                kind_hint: Some(EdgeType::Event),
                evidence: ev(),
            }
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
                ("audit".into(), None, EdgeType::Event, Confidence::Inferred, "publishes \"orders\", consumed by audit".into()),
                ("dispatch".into(), None, EdgeType::Event, Confidence::Inferred, "publishes \"orders\", consumed by dispatch".into()),
            ]
        );
        let out = r.resolve_all("dispatch", &topic("orders", TopicRole::Consumer));
        assert_eq!(out.len(), 1);
        let o = out[0].clone().unwrap();
        assert_eq!(
            (o.target.as_str(), o.source.as_deref(), o.detail.as_str()),
            ("dispatch", Some("payment"), "consumes \"orders\", published by payment")
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
            ("dispatch", Some("payment"), "mentions \"robot-shop\", published by payment")
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
        let s = services(&["payment"], &[("rabbitmq", "rabbitmq:3-management"), ("redis", "redis:7")]);
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
            (o.target.as_str(), o.edge_type, o.confidence, o.detail.as_str()),
            ("rabbitmq", EdgeType::Event, Confidence::Inferred, "imports pika (rabbitmq client)")
        );
        assert_eq!(r.resolve_all("payment", &broker("kafka")), vec![Err(Unresolved::Ignored)]);
        assert_eq!(r.resolve_all("rabbitmq", &broker("rabbitmq")), vec![Err(Unresolved::SelfEdge)]);
    }

    #[test]
    fn shared_databases_join_every_other_user() {
        let s = services(&["orders", "reports", "ledger"], &[]);
        let cfg = ConfigIndex::default();
        let mut joins = Joins::default();
        joins.databases.insert("mysql/shop".into(), set(&["orders", "reports"]));
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
        assert_eq!(r.resolve_all("ledger", &db("ledgerdb")), vec![Err(Unresolved::Ignored)]);
    }

    #[test]
    fn packages_resolve_exact_prefix_and_path() {
        let mut s = services(&["web", "shared", "cart", "checkout", "Basket.API", "EventBus"], &[]);
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
        cfg.packages.push(pkg("github.com/acme/demo/services/cart", "cart"));
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
            (o.target.as_str(), o.edge_type, o.confidence, o.detail.as_str()),
            ("shared", EdgeType::Import, Confidence::Static, "import @acme/shared/utils")
        );
        assert_eq!(
            r.resolve("checkout", &package("github.com/acme/demo/services/cart/genproto")).unwrap().target,
            "cart"
        );
        assert_eq!(
            r.resolve("checkout", &package("github.com/acme/demo/services/cartography")),
            Err(Unresolved::Ignored),
            "a prefix must end at a separator"
        );
        assert_eq!(r.resolve("web", &package("express")), Err(Unresolved::Ignored));
        assert_eq!(r.resolve("shared", &package("@acme/shared")), Err(Unresolved::SelfEdge));
        let path = |path: &str| Candidate {
            target: Target::PackagePath {
                path: path.into(),
                how: "ProjectReference ..\\EventBus\\EventBus.csproj".into(),
            },
            kind_hint: Some(EdgeType::Import),
            evidence: ev(),
        };
        let o = r.resolve("Basket.API", &path("src/EventBus/EventBus.csproj")).unwrap();
        assert_eq!((o.target.as_str(), o.confidence), ("EventBus", Confidence::Static));
        assert_eq!(r.resolve("Basket.API", &path("src/Nowhere/X.csproj")), Err(Unresolved::Ignored));
        assert_eq!(
            owner_of_path(&s, "src/EventBus/EventBus.csproj"),
            Some("EventBus")
        );
        assert_eq!(owner_of_path(&s, "src/EventBusRabbitMQ/x.cs"), None, "a root prefix ends at a slash");
    }
```

Imports needed at the top of the test module: `use crate::map::{Target, TopicRole, owner_of_path};` and `use std::collections::BTreeSet;`. `Resolved` needs `#[derive(Clone)]` (it has `Debug, Clone, PartialEq, Eq` already).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p blastradius-core --lib resolve 2>&1 | grep -E 'error|FAILED|test result' | head`
Expected: compile errors: `Joins`, `TopicSides`, `TopicRole`, `Target::Topic` not found.

- [ ] **Step 3: Targets, roles and path ownership in `map/mod.rs`**

Add to `Target`:

```rust
    /// A message topic, queue, exchange or event type, with the part the
    /// mentioning code plays.
    Topic { key: String, role: TopicRole },
    /// A message broker known through the client library a file imports;
    /// `how` is the detail, e.g. `imports pika (rabbitmq client)`.
    Broker { family: String, how: String },
    /// A database key another service may share: `host/dbname`, or a named
    /// resource such as `orderingdb`.
    Database(String),
    /// A package, module or artifact as imported or declared; `how` is the
    /// detail, e.g. `import @acme/shared/utils`, `dependency @acme/shared`.
    Package { name: String, how: String },
    /// A repository-relative path another manifest refers to, already
    /// normalised: `src/EventBus/EventBus.csproj`, `services/core-rs`.
    PackagePath { path: String, how: String },
```

and after `Target`:

```rust
/// Which side of a topic a mention is on. `Unknown` is a declaration or a
/// binding: `queue_declare("orders")`, `QueueBind(...)`, an SQS ARN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopicRole {
    Producer,
    Consumer,
    Unknown,
}
```

Add `owner_of_path` and use it from `partition_files`:

```rust
/// The code service whose root is the longest prefix of `path`. A root of
/// `.` owns everything.
pub fn owner_of_path<'a>(services: &'a [Service], path: &str) -> Option<&'a str> {
    let mut owners: Vec<(&str, &str)> = services
        .iter()
        .filter(|s| s.role == ServiceRole::Code)
        .filter_map(|s| s.root.as_deref().map(|r| (s.name.as_str(), r)))
        .collect();
    owners.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(b.0)));
    owners
        .iter()
        .find(|(_, r)| *r == "." || path.starts_with(*r) && path[r.len()..].starts_with('/'))
        .map(|(name, _)| *name)
}
```

In `partition_files`, replace the local `owners` vector and `owner_of` closure with `let owner_of = |file: &str| owner_of_path(services, file);`. (Sorting per file is O(services) per call; with at most a few hundred services and files this stays well inside the budget. If the corpus timing test shows train-ticket near 500 ms, sort once and pass the sorted list instead.)

- [ ] **Step 4: Joins, `resolve_all` and the five resolutions in `resolve.rs`**

Replace the `proto_owner: HashMap<String, String>` parameter and field with `joins: Joins`, keep the `proto_owners: HashSet<String>` cache built from `joins.proto_owner.values()`, and add the types:

```rust
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
```

`Resolved` gains `pub source: Option<String>` with a doc comment: "When the edge starts somewhere other than the file's own service: a consumer's mention yields producer → consumer." `finish` sets `source: None`. `resolve` gains the arms:

```rust
            Target::Topic { .. } | Target::Broker { .. } | Target::Database(_) => self
                .resolve_all(source, candidate)
                .into_iter()
                .next()
                .unwrap_or(Err(Unresolved::Ignored)),
            Target::Package { name, how } => self.resolve_package(source, name, how, hint),
            Target::PackagePath { path, how } => {
                let owner = self.service_for_path(path).ok_or(Unresolved::Ignored)?;
                finish(source, owner, EdgeType::Import, Confidence::Static, how.clone(), hint)
            }
```

and the new methods:

```rust
    /// Every edge a candidate yields. Hosts, variables and packages give at
    /// most one; topics, brokers and shared databases fan out to every
    /// counterpart.
    pub fn resolve_all(&self, source: &str, candidate: &Candidate) -> Vec<Result<Resolved, Unresolved>> {
        match &candidate.target {
            Target::Topic { key, role } => self.resolve_topic(source, key, *role),
            Target::Broker { family, how } => self.resolve_broker(source, family, how),
            Target::Database(key) => self.resolve_database(source, key),
            _ => vec![self.resolve(source, candidate)],
        }
    }

    fn resolve_topic(&self, source: &str, key: &str, role: TopicRole) -> Vec<Result<Resolved, Unresolved>> {
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
            set.iter().filter(|s| s.as_str() != source).cloned().collect()
        };
        match role {
            TopicRole::Producer => {
                for c in others(&sides.consumers) {
                    out.push(Ok(inferred(&c, None, format!("publishes \"{key}\", consumed by {c}"))));
                }
                for u in others(&sides.unknown) {
                    out.push(Ok(inferred(&u, None, format!("publishes \"{key}\", mentioned by {u}"))));
                }
            }
            TopicRole::Consumer => {
                for p in others(&sides.producers) {
                    out.push(Ok(inferred(source, Some(&p), format!("consumes \"{key}\", published by {p}"))));
                }
                for u in others(&sides.unknown) {
                    out.push(Ok(inferred(source, Some(&u), format!("consumes \"{key}\", mentioned by {u}"))));
                }
            }
            TopicRole::Unknown => {
                for p in others(&sides.producers) {
                    out.push(Ok(inferred(source, Some(&p), format!("mentions \"{key}\", published by {p}"))));
                }
                for c in others(&sides.consumers) {
                    out.push(Ok(inferred(&c, None, format!("mentions \"{key}\", consumed by {c}"))));
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

    /// Discovered services whose name or image names the broker family.
    fn resolve_broker(&self, source: &str, family: &str, how: &str) -> Vec<Result<Resolved, Unresolved>> {
        let aliases: &[&str] = match family {
            "rabbitmq" => &["rabbitmq", "amqp"],
            "kafka" => &["kafka", "redpanda"],
            "mqtt" => &["mqtt", "mosquitto", "emqx", "hivemq"],
            "sqs" | "sns" => &[family, "localstack"],
            "activemq" => &["activemq", "artemis"],
            other => std::slice::from_ref(&other),
        };
        let mut out = Vec::new();
        for s in self.services {
            let mut haystack = s.name.to_lowercase();
            if let Some(image) = image_basename(s.image.as_deref()) {
                haystack.push(' ');
                haystack.push_str(&image.to_lowercase());
            }
            if aliases.iter().any(|a| haystack.contains(a)) {
                out.push(finish(source, &s.name, EdgeType::Event, Confidence::Inferred, how.to_string(), None));
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
    fn resolve_package(&self, source: &str, name: &str, how: &str, hint: Option<EdgeType>) -> Result<Resolved, Unresolved> {
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
        finish(source, owner, EdgeType::Import, Confidence::Static, how.to_string(), hint)
    }

    pub fn service_for_path(&self, path: &str) -> Option<&str> {
        crate::map::owner_of_path(self.services, path)
    }
```

Note `resolve_broker` returns `SelfEdge` errors for the broker itself through `finish`, as the test expects. Import `BTreeSet` and `TopicRole`.

- [ ] **Step 5: `config.rs` gains the package index shape**

```rust
/// A name a service can be imported by: its package.json name, Go module
/// path, Cargo or Maven artifact, .csproj stem, or Python directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    pub service: String,
    pub evidence: Evidence,
}
```

and `pub packages: Vec<Package>,` on `ConfigIndex` (kept empty until Task 6; `Default` derives it).

- [ ] **Step 6: Wire `map::run` and `collect_outcomes`**

In `run`, build the joins (topics and databases stay empty until Tasks 4 and 5):

```rust
    let joins = Joins {
        proto_owner: matchers::grpc::proto_owners(&extractions, &config, services),
        ..Joins::default()
    };
    let resolver = Resolver::new(services, &config, joins);
```

In `collect_outcomes`, the inner loop becomes:

```rust
                for candidate in matcher.candidates(&ctx) {
                    for outcome in resolver.resolve_all(service, &candidate) {
                        results.push(outcome.map(|resolved| Edge {
                            source: resolved.source.clone().unwrap_or_else(|| service.clone()),
                            target: resolved.target,
                            edge_type: resolved.edge_type,
                            confidence: resolved.confidence,
                            evidence: vec![Evidence {
                                file: candidate.evidence.file.clone(),
                                line: candidate.evidence.line,
                                detail: Some(resolved.detail),
                            }],
                        }));
                    }
                }
```

Import `Joins` from `resolve`.

- [ ] **Step 7: Run everything**

Run: `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E '^test result|FAILED|panicked'`
Expected: all green. No fixture output changes (nothing emits the new targets yet); `corpus` unchanged.

- [ ] **Step 8: Commit**

```bash
git add crates/blastradius-core/src/map
git commit -m "feat(map): resolver joins for topics, brokers, shared databases, packages and project paths" -m "The resolver takes a Joins index from the pre-pass over every file's facts and gains resolve_all, which fans one candidate out to every counterpart: a producer's topic to each consumer (Inferred, producer to consumer), a consumer's to each producer with the edge's source overridden, a database key to every other service using it, a client library to each discovered broker of its family. Packages resolve by exact name or a prefix ending at a separator, project paths by the service whose root owns them, both Static. A topic with one side only is listed as topic:<key>; an import that matches no discovered package is an external library and is dropped."
```

---

### Task 4: Symbol table and the event matcher

The corpus almost never writes a topic as a literal at the call. robot-shop's payment publishes `exchange=self.EXCHANGE, routing_key=self.ROUTING_KEY` with `EXCHANGE = 'robot-shop'` a few lines up; train-ticket listens with `@RabbitListener(queues = Queues.queueName)` and `queueName = "email"` sits in a sibling file; eShop publishes a variable and constructs `new OrderStartedIntegrationEvent(...)` somewhere else. So: a per-service symbol table from `Setting` facts, and "constructs an event type" counts as producing it.

**Files:**
- Create: `crates/blastradius-core/src/map/symbols.rs`
- Create: `crates/blastradius-core/src/map/matchers/event.rs`
- Modify: `crates/blastradius-core/src/map/matchers/mod.rs` (`FileContext.symbols`, `all()`)
- Modify: `crates/blastradius-core/src/map/mod.rs` (`pub mod symbols;`, `run` builds symbols and `joins.topics`)
- Modify: `crates/blastradius-core/src/map/matchers/http.rs` tests (add `symbols` to `FileContext`)
- Create: `test/fixtures/edges-events-app/**`
- Modify: `crates/blastradius-core/tests/edges.rs`

**Interfaces:**
- Produces:
  ```rust
  // symbols.rs
  /// service -> symbol name -> distinct literal values (at most 8, file order).
  pub type Symbols = HashMap<String, HashMap<String, Vec<String>>>;
  pub fn build(extractions: &[(String, String, Extraction)]) -> Symbols
  /// `self.EXCHANGE`, `Queues.queueName`, `queues = Queues.queueName`, `kafka.Topic`
  /// -> the values bound to the last identifier segment in this service. Empty for
  /// anything that is not an identifier path.
  pub fn lookup<'a>(symbols: &'a Symbols, service: &str, expr: &str) -> Vec<&'a str>

  // matchers/mod.rs
  pub struct FileContext<'a> { pub service, pub file, pub facts, pub config, pub symbols: &'a Symbols }

  // matchers/event.rs
  pub struct Event;
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct Mention { pub key: String, pub role: TopicRole, pub evidence: Evidence }
  pub fn mentions(ctx: &FileContext<'_>) -> Vec<Mention>
  pub fn broker_family(import_path: &str) -> Option<&'static str>
  pub fn is_topic_key(s: &str) -> bool
  pub fn topic_index(extractions: &[(String, String, Extraction)], config: &ConfigIndex, symbols: &Symbols) -> HashMap<String, TopicSides>
  ```
- Consumes: `Target::Topic`, `Target::Broker`, `TopicRole`, `TopicSides` from Task 3; `Fact::Setting`, `Arg::Other` symbol values from Task 1.

- [ ] **Step 1: Create the fixture**

```
test/fixtures/edges-events-app/docker-compose.yml
services:
  payment:
    build: ./payment
  dispatch:
    build: ./dispatch
  checkout:
    build: ./checkout
  accounting:
    build: ./accounting
  notifications:
    build: ./notifications
  webhooks:
    build: ./webhooks
  rabbitmq:
    image: rabbitmq:3-management
  kafka:
    image: confluentinc/cp-kafka:7.6.0

test/fixtures/edges-events-app/payment/requirements.txt
pika

test/fixtures/edges-events-app/payment/rabbitmq.py
import pika


class Publisher:
    EXCHANGE = 'robot-shop'
    ROUTING_KEY = 'orders'

    def publish(self, body):
        self._channel.exchange_declare(exchange=self.EXCHANGE, exchange_type='direct', durable=True)
        self._channel.basic_publish(exchange=self.EXCHANGE,
                                    routing_key=self.ROUTING_KEY,
                                    body=body)

    def notify(self, body):
        self._channel.basic_publish(exchange='', routing_key='email', body=body)

test/fixtures/edges-events-app/dispatch/go.mod
module dispatch

go 1.22

test/fixtures/edges-events-app/dispatch/main.go
package main

import "github.com/streadway/amqp"

func main() {
	ch.ExchangeDeclare("robot-shop", "direct", true, false, false, false, nil)
	ch.QueueDeclare("orders", true, false, false, false, nil)
	ch.QueueBind("orders", "orders", "robot-shop", false, nil)
	msgs, _ := ch.Consume("orders", "", true, false, false, false, nil)
	_ = msgs
}

test/fixtures/edges-events-app/checkout/package.json
{ "name": "checkout", "main": "producer.js" }

test/fixtures/edges-events-app/checkout/producer.js
const { Kafka } = require('kafkajs');
await producer.send({ topic: 'order-created', messages: [{ value: 'hi' }] });
await producer.send({ topic: 'audit-log', messages: [] });
res.send('ok');

test/fixtures/edges-events-app/accounting/accounting.csproj
<Project Sdk="Microsoft.NET.Sdk.Worker"></Project>

test/fixtures/edges-events-app/accounting/Consumer.cs
using Confluent.Kafka;

public class Consumer
{
    private static readonly string TopicName = Environment.GetEnvironmentVariable("KAFKA_TOPIC") ?? "order-created";

    public async Task Run()
    {
        _consumer.Subscribe(TopicName);
        await eventBus.PublishAsync(new OrderPaidIntegrationEvent(id));
    }
}

test/fixtures/edges-events-app/notifications/pom.xml
<project><artifactId>notifications</artifactId></project>

test/fixtures/edges-events-app/notifications/src/main/java/Queues.java
public class Queues {
    public final static String queueName = "email";
}

test/fixtures/edges-events-app/notifications/src/main/java/Listener.java
import org.springframework.amqp.rabbit.annotation.RabbitListener;
import org.springframework.kafka.annotation.KafkaListener;

public class Listener {
    @KafkaListener(topics = "order-created", groupId = "notifications")
    public void onOrder(String m) {}

    @RabbitListener(queues = Queues.queueName)
    public void onEmail(String m) {}
}

test/fixtures/edges-events-app/webhooks/webhooks.csproj
<Project Sdk="Microsoft.NET.Sdk.Web"></Project>

test/fixtures/edges-events-app/webhooks/Program.cs
eventBus.AddSubscription<OrderPaidIntegrationEvent, OrderPaidIntegrationEventHandler>();

public class OrderPaidIntegrationEventHandler : IIntegrationEventHandler<OrderPaidIntegrationEvent>
{
}
```

- [ ] **Step 2: Write the failing tests**

Unit tests in `symbols.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::facts::{Fact, Parser};
    use pretty_assertions::assert_eq;

    fn ex(service: &str, file: &str, facts: Vec<Fact>) -> (String, String, Extraction) {
        (
            service.into(),
            file.into(),
            Extraction {
                facts,
                parser: Parser::Regex,
                language: "x".into(),
            },
        )
    }
    fn setting(key: &str, value: &str) -> Fact {
        Fact::Setting {
            key: key.into(),
            value: value.into(),
            line: 1,
        }
    }

    #[test]
    fn symbols_are_per_service_and_resolve_member_paths() {
        let symbols = build(&[
            ex("payment", "payment/rabbitmq.py", vec![setting("EXCHANGE", "robot-shop"), setting("ROUTING_KEY", "orders")]),
            ex("notifications", "notifications/Queues.java", vec![setting("queueName", "email")]),
            ex("notifications", "notifications/Other.java", vec![setting("queueName", "email"), setting("queueName", "sms")]),
        ]);
        assert_eq!(lookup(&symbols, "payment", "self.EXCHANGE"), vec!["robot-shop"]);
        assert_eq!(lookup(&symbols, "payment", "ROUTING_KEY"), vec!["orders"]);
        assert_eq!(lookup(&symbols, "notifications", "queues = Queues.queueName"), vec!["email", "sms"]);
        assert_eq!(lookup(&symbols, "payment", "queueName"), Vec::<&str>::new());
        assert_eq!(lookup(&symbols, "payment", "foo(EXCHANGE)"), Vec::<&str>::new());
        assert_eq!(lookup(&symbols, "payment", "['orders']"), Vec::<&str>::new());
    }
}
```

Unit tests in `event.rs` (on `mentions` through a hand-built `FileContext`, and on the pure helpers):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::config::ConfigIndex;
    use crate::map::facts::Arg;
    use pretty_assertions::assert_eq;

    fn call(callee: &str, args: Vec<Arg>) -> Fact {
        Fact::Call {
            callee: callee.into(),
            args,
            line: 5,
        }
    }
    fn keys(ctx: &FileContext<'_>) -> Vec<(String, TopicRole)> {
        mentions(ctx).into_iter().map(|m| (m.key, m.role)).collect()
    }

    #[test]
    fn roles_and_keys_from_calls_annotations_and_base_types() {
        let mut symbols = Symbols::default();
        symbols.entry("payment".into()).or_default().insert("EXCHANGE".into(), vec!["robot-shop".into()]);
        let facts = vec![
            call("self._channel.basic_publish", vec![Arg::Other("self.EXCHANGE".into()), Arg::Str("orders".into()), Arg::Other("body".into())]),
            call("ch.QueueDeclare", vec![Arg::Str("orders".into()), Arg::Other("true".into())]),
            call("ch.Consume", vec![Arg::Str("orders".into())]),
            call("producer.send", vec![Arg::Other("{ topic: 'order-created', messages: [{ value: 'hi' }] }".into())]),
            call("consumer.subscribe", vec![Arg::Other("{ topics: ['a', 'b'] }".into())]),
            call("consumer.subscribe", vec![Arg::Other("['orders']".into())]),
            call("res.send", vec![Arg::Str("ok".into())]),
            call("kafkaTemplate.send", vec![Arg::Str("orders".into()), Arg::Other("m".into())]),
            call("new ProducerRecord<>", vec![Arg::Str("orders".into())]),
            call("sarama.ProducerMessage", vec![Arg::Str("orders".into())]),
            call("eventBus.AddSubscription<OrderPaidIntegrationEvent, OrderPaidIntegrationEventHandler>", vec![]),
            call("eventBus.PublishAsync", vec![Arg::Other("new OrderPaidIntegrationEvent(id)".into())]),
            call("new OrderStartedIntegrationEvent", vec![Arg::Other("userId".into())]),
            call("new OrderStartedDomainEvent", vec![]),
            call("observable.subscribe", vec![Arg::Other("x => go(x)".into())]),
            Fact::Annotation { name: "KafkaListener".into(), args: vec![Arg::Str("order-created".into()), Arg::Str("g".into())], line: 9 },
            Fact::Annotation { name: "RabbitListener".into(), args: vec![Arg::Other("Queues.queueName".into())], line: 10 },
            Fact::Extends { name: "IIntegrationEventHandler<OrderPaidIntegrationEvent>".into(), line: 11 },
            Fact::Extends { name: "INotificationHandler<OrderStartedDomainEvent>".into(), line: 12 },
            Fact::Extends { name: "IRequestHandler<CreateOrderCommand, bool>".into(), line: 13 },
        ];
        let cfg = ConfigIndex::default();
        let ctx = FileContext { service: "payment", file: "payment/x.py", facts: &facts, config: &cfg, symbols: &symbols };
        assert_eq!(
            keys(&ctx),
            vec![
                ("robot-shop".into(), TopicRole::Producer),
                ("orders".into(), TopicRole::Producer),
                ("orders".into(), TopicRole::Unknown),
                ("orders".into(), TopicRole::Consumer),
                ("order-created".into(), TopicRole::Producer),
                ("a".into(), TopicRole::Consumer),
                ("b".into(), TopicRole::Consumer),
                ("orders".into(), TopicRole::Consumer),
                ("orders".into(), TopicRole::Producer),
                ("orders".into(), TopicRole::Producer),
                ("orders".into(), TopicRole::Producer),
                ("OrderPaidIntegrationEvent".into(), TopicRole::Consumer),
                ("OrderPaidIntegrationEvent".into(), TopicRole::Producer),
                ("OrderStartedIntegrationEvent".into(), TopicRole::Producer),
                ("order-created".into(), TopicRole::Consumer),
                ("OrderPaidIntegrationEvent".into(), TopicRole::Consumer),
            ]
        );
    }

    #[test]
    fn topic_keys_and_broker_families() {
        assert!(is_topic_key("orders") && is_topic_key("order.created") && is_topic_key("robot-shop") && is_topic_key("OrderPaidIntegrationEvent") && is_topic_key("arn:aws:sns:us-east-1:1:orders"));
        assert!(!is_topic_key("") && !is_topic_key("x") && !is_topic_key("ok") && !is_topic_key("/orders") && !is_topic_key("http://x") && !is_topic_key("hello world") && !is_topic_key("123") && !is_topic_key("utf-8") && !is_topic_key("${TOPIC}"));
        assert_eq!(broker_family("pika"), Some("rabbitmq"));
        assert_eq!(broker_family("github.com/streadway/amqp"), Some("rabbitmq"));
        assert_eq!(broker_family("org.springframework.amqp.rabbit.annotation.RabbitListener"), Some("rabbitmq"));
        assert_eq!(broker_family("kafkajs"), Some("kafka"));
        assert_eq!(broker_family("Confluent.Kafka"), Some("kafka"));
        assert_eq!(broker_family("github.com/IBM/sarama"), Some("kafka"));
        assert_eq!(broker_family("nats"), Some("nats"));
        assert_eq!(broker_family("@aws-sdk/client-sqs"), Some("sqs"));
        assert_eq!(broker_family("express"), None);
        assert_eq!(broker_family("redis"), None);
    }
}
```

Integration test in `tests/edges.rs`:

```rust
#[test]
fn event_edges_join_producers_to_consumers_and_brokers_to_libraries() {
    let (edges, json) = edges_of("edges-events-app");
    let e = find(&edges, "payment", "dispatch", EdgeType::Event);
    assert_eq!(e.confidence, Confidence::Inferred);
    let files: Vec<&str> = e.evidence.iter().map(|v| v.file.as_str()).collect();
    assert!(files.contains(&"payment/rabbitmq.py") && files.contains(&"dispatch/main.go"), "{:?}", e.evidence);
    assert!(
        e.evidence.iter().any(|v| v.detail.as_deref() == Some("publishes \"orders\", consumed by dispatch")),
        "{:?}",
        e.evidence
    );
    assert!(
        e.evidence.iter().any(|v| v.detail.as_deref() == Some("consumes \"orders\", published by payment")),
        "{:?}",
        e.evidence
    );
    find(&edges, "payment", "notifications", EdgeType::Event); // 'email' through Queues.queueName
    find(&edges, "checkout", "accounting", EdgeType::Event); // kafkajs object literal -> Confluent Subscribe(TopicName)
    find(&edges, "checkout", "notifications", EdgeType::Event); // @KafkaListener
    find(&edges, "accounting", "webhooks", EdgeType::Event); // new OrderPaidIntegrationEvent -> AddSubscription<...>
    for (s, t) in [
        ("payment", "rabbitmq"),
        ("dispatch", "rabbitmq"),
        ("notifications", "rabbitmq"),
        ("checkout", "kafka"),
        ("accounting", "kafka"),
        ("notifications", "kafka"),
    ] {
        let e = find(&edges, s, t, EdgeType::Event);
        assert_eq!(e.confidence, Confidence::Inferred, "{s} -> {t}");
        assert!(e.evidence[0].detail.as_deref().unwrap().starts_with("imports "), "{:?}", e.evidence);
    }
    assert!(!edges.iter().any(|e| e.source == e.target));
    assert!(!edges.iter().any(|e| e.source == "dispatch" && e.target == "payment"), "no reverse edge");
    assert!(
        json.mapping.unresolved_targets.contains(&"topic:audit-log".to_string()),
        "{:?}",
        json.mapping.unresolved_targets
    );
    assert!(!json.mapping.unresolved_targets.iter().any(|t| t == "topic:ok"), "res.send is not a producer");
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p blastradius-core 2>&1 | grep -E 'error\[|FAILED|test result' | head`
Expected: compile errors (`symbols` module, `FileContext.symbols`, `event` module missing).

- [ ] **Step 4: `symbols.rs`**

```rust
//! Constants a service binds to literals, so a topic written as
//! `self.EXCHANGE` or `Queues.queueName` can be read back. Built from the
//! Setting facts of every file the service owns.
use std::collections::HashMap;

use super::facts::{Extraction, Fact};

/// service -> symbol name -> distinct literal values, in file order, at most
/// eight per name so a common name such as `url` cannot fan out wildly.
pub type Symbols = HashMap<String, HashMap<String, Vec<String>>>;

const MAX_VALUES: usize = 8;

pub fn build(extractions: &[(String, String, Extraction)]) -> Symbols {
    let mut symbols: Symbols = HashMap::new();
    for (service, _, ex) in extractions {
        let table = symbols.entry(service.clone()).or_default();
        for fact in &ex.facts {
            if let Fact::Setting { key, value, .. } = fact {
                let values = table.entry(key.clone()).or_default();
                if values.len() < MAX_VALUES && !values.contains(value) {
                    values.push(value.clone());
                }
            }
        }
    }
    symbols
}

/// The literals bound to an identifier path in this service. `expr` may be
/// `NAME`, `self.NAME`, `Queues.name`, or an annotation pair `key = Path.name`;
/// the last segment is the symbol. Anything with brackets, quotes or spaces
/// beyond that shape is not an identifier and resolves to nothing.
pub fn lookup<'a>(symbols: &'a Symbols, service: &str, expr: &str) -> Vec<&'a str> {
    let expr = expr.trim();
    let expr = expr.split_once('=').map_or(expr, |(_, rhs)| rhs.trim());
    let is_path = !expr.is_empty()
        && expr
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$'))
        && expr.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$');
    if !is_path {
        return Vec::new();
    }
    let name = expr.rsplit('.').next().unwrap_or(expr).trim_start_matches('$');
    symbols
        .get(service)
        .and_then(|table| table.get(name))
        .map(|values| values.iter().map(String::as_str).collect())
        .unwrap_or_default()
}
```

Add `pub mod symbols;` to `map/mod.rs`. Add `pub symbols: &'a Symbols` to `FileContext` (import `crate::map::symbols::Symbols`), and in `http.rs` tests construct `let symbols = Symbols::default();` and pass `symbols: &symbols`.

- [ ] **Step 5: `matchers/event.rs`**

```rust
//! Message producers and consumers, joined on what they name: a topic, a
//! queue, an exchange, a routing key, or the type of a typed event. The
//! resolver's topic index says who is on the other side. A client library
//! import also names the broker family, which gives the edge to the broker
//! itself when the repository declares one.
use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use super::{FileContext, Matcher};
use crate::map::config::ConfigIndex;
use crate::map::facts::{Arg, Extraction, Fact};
use crate::map::resolve::TopicSides;
use crate::map::symbols::{self, Symbols};
use crate::map::{Candidate, Target, TopicRole};
use crate::model::{EdgeType, Evidence};

pub struct Event;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    pub key: String,
    pub role: TopicRole,
    pub evidence: Evidence,
}

/// `send`/`sendAsync` alone is too common (`res.send`); it counts only when
/// the receiver names a messaging object.
const SEND_RECEIVERS: &[&str] = &[
    "producer", "kafka", "template", "publisher", "topic", "queue", "sqs", "sns", "pubsub",
    "eventhub", "bus", "stream", "bridge",
];
const PRODUCER_METHODS: &[&str] = &[
    "publish", "publishasync", "basic_publish", "basicpublish", "basicpublishasync", "produce",
    "produceasync", "sendtoqueue", "convertandsend", "sendmessage", "sendmessageasync",
    "sendmessagebatch", "putevents", "putrecord", "publishbatch",
];
const CONSUMER_METHODS: &[&str] = &[
    "subscribe", "subscribeasync", "psubscribe", "consume", "consumeasync", "basic_consume",
    "basicconsume", "basicconsumeasync", "receivemessage", "receivemessageasync",
    "addsubscription", "queuesubscribe", "subscribesync", "chansubscribe",
];
const DECLARE_METHODS: &[&str] = &[
    "queue_declare", "queuedeclare", "queuedeclareasync", "assertqueue", "queue_bind",
    "queuebind", "queuebindasync", "bindqueue", "exchange_declare", "exchangedeclare",
    "exchangedeclareasync", "assertexchange", "createtopic", "createqueue", "topicarn",
    "queueurl", "topic_arn", "queue_url", "topic", "subscription",
];
static PRODUCER_TYPES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(ProducerMessage|WriterConfig|kafka\.Writer|ProducerRecord|PublishRequest|SendMessageRequest|PublishCommand|SendMessageCommand)$").unwrap()
});
static CONSUMER_TYPES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(ConsumerMessage|ReaderConfig|kafka\.Reader|KafkaConsumer|SubscribeRequest|ReceiveMessageRequest|ReceiveMessageCommand)$").unwrap()
});
const CONSUMER_ANNOTATIONS: &[&str] = &[
    "KafkaListener", "RabbitListener", "JmsListener", "SqsListener", "StreamListener",
    "PulsarListener", "NatsListener", "ServiceBusTrigger", "QueueTrigger", "EventHubTrigger",
    "KafkaTrigger", "RabbitMQTrigger", "Incoming",
];
const PRODUCER_ANNOTATIONS: &[&str] = &["SendTo", "Outgoing"];
/// `new OrderPaidIntegrationEvent(...)`: constructing a typed event is
/// producing it. Domain events stay in-process.
static NEW_EVENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bnew\s+(?:[\w.]+\.)?([A-Z]\w*Event)\b").unwrap());
static ADD_SUBSCRIPTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"AddSubscription<\s*([A-Za-z_]\w*)").unwrap());
/// A handler's base type names what it consumes: `IIntegrationEventHandler<X>`,
/// `IConsumer<X>`. Single type argument only, so `IRequestHandler<A, B>` and
/// MediatR's `INotificationHandler` (in-process) are left out.
static HANDLER_OF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^I?[A-Za-z_]\w*?(?:IntegrationEventHandler|EventHandler|MessageHandler|Consumer|HandleMessages)<\s*([A-Za-z_]\w*)\s*>$").unwrap()
});
static OBJECT_KEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(?:topics?|queue(?:name|url)?|exchange|routing_?key|subject|channel|topic_?arn|destination)\b\s*[:=]\s*\[?\s*['"]([^'"]+)['"]"#).unwrap()
});
static QUOTED_IN_LIST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"['"]([^'"]+)['"]"#).unwrap());
static LIST_START: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:\[|listOf\s*\(|List\.of\s*\(|Arrays\.asList\s*\(|\[\]string\s*\{|new\s*(?:string)?\s*\[\]|vec!\s*\[|Set\.of\s*\()").unwrap()
});
const STOP_KEYS: &[&str] = &[
    "true", "false", "null", "none", "nil", "utf-8", "utf8", "json", "application/json",
    "text/plain", "get", "post", "put", "delete", "patch", "error", "message", "data", "ok",
    "close", "connect", "end", "open", "ready", "exit", "string", "number", "object",
    "default", "id", "name", "type", "value", "key", "topic", "queue", "exchange", "test",
    "direct", "fanout", "headers", "localhost",
];

/// A plausible topic, queue, exchange, routing key or event type name.
pub fn is_topic_key(s: &str) -> bool {
    let n = s.len();
    (2..=120).contains(&n)
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | ':' | '*' | '#'))
        && s.chars().any(|c| c.is_ascii_alphabetic())
        && !s.starts_with(['/', '-', '.'])
        && !s.contains("://")
        && !STOP_KEYS.contains(&s.to_lowercase().as_str())
}

/// The broker a client library speaks to, from an import path.
pub fn broker_family(import_path: &str) -> Option<&'static str> {
    let p = import_path.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| p.contains(w));
    if has(&["rabbitmq", "amqp", "pika", "bunny", "kombu", "masstransit.rabbit"]) {
        Some("rabbitmq")
    } else if has(&["kafka", "sarama", "confluent"]) {
        Some("kafka")
    } else if has(&["nats"]) {
        Some("nats")
    } else if has(&["pulsar"]) {
        Some("pulsar")
    } else if has(&["mqtt", "paho"]) {
        Some("mqtt")
    } else if has(&["sqs"]) {
        Some("sqs")
    } else if has(&["sns"]) {
        Some("sns")
    } else if has(&["activemq", "artemis", "javax.jms", "jakarta.jms"]) {
        Some("activemq")
    } else if has(&["servicebus"]) {
        Some("servicebus")
    } else if has(&["eventhub"]) {
        Some("eventhub")
    } else {
        None
    }
}

/// `new ProducerRecord<>` -> (`new producerrecord`, `producerrecord`, ``):
/// the callee without generics, its last segment and its receiver, lowercased.
fn split_callee(callee: &str) -> (String, String, String) {
    let base = callee.split('<').next().unwrap_or(callee);
    let base = base.trim().trim_start_matches(['&', '*', '(']).trim();
    let base = base.strip_prefix("await ").unwrap_or(base).trim();
    let lower = base.to_lowercase();
    let last = lower.rsplit(['.', ':', '>']).next().unwrap_or(&lower).to_string();
    let receiver = lower[..lower.len() - last.len()]
        .trim_end_matches(['.', ':', '>'])
        .to_string();
    (lower, last, receiver)
}

fn role_of_callee(callee: &str) -> Option<TopicRole> {
    let (base, last, receiver) = split_callee(callee);
    if PRODUCER_METHODS.contains(&last.as_str()) {
        return Some(TopicRole::Producer);
    }
    if (last == "send" || last == "sendasync") && SEND_RECEIVERS.iter().any(|r| receiver.contains(r)) {
        return Some(TopicRole::Producer);
    }
    if CONSUMER_METHODS.contains(&last.as_str()) {
        return Some(TopicRole::Consumer);
    }
    if DECLARE_METHODS.contains(&last.as_str()) {
        return Some(TopicRole::Unknown);
    }
    if PRODUCER_TYPES.is_match(&base) {
        return Some(TopicRole::Producer);
    }
    if CONSUMER_TYPES.is_match(&base) {
        return Some(TopicRole::Consumer);
    }
    None
}

/// Topic keys named by a call's or annotation's arguments: string literals,
/// `new XEvent(...)`, object literals with a topic-ish key, lists of strings,
/// and identifiers looked up in the service's symbols.
fn keys_from_args(ctx: &FileContext<'_>, args: &[Arg]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |k: &str| {
        if is_topic_key(k) && !out.iter().any(|o| o == k) {
            out.push(k.to_string());
        }
    };
    for arg in args {
        match arg {
            Arg::Str(s) => push(s),
            Arg::Template(_) => {}
            Arg::Other(text) => {
                let text = text.trim();
                if let Some(caps) = NEW_EVENT.captures(text) {
                    if !caps[1].ends_with("DomainEvent") {
                        push(&caps[1]);
                    }
                } else if text.starts_with('{') {
                    for caps in OBJECT_KEY.captures_iter(text) {
                        push(&caps[1]);
                    }
                } else if LIST_START.is_match(text) {
                    for caps in QUOTED_IN_LIST.captures_iter(text) {
                        push(&caps[1]);
                    }
                } else {
                    for value in symbols::lookup(ctx.symbols, ctx.service, text) {
                        push(value);
                    }
                }
            }
        }
    }
    out
}

fn evidence(ctx: &FileContext<'_>, line: u32, detail: String) -> Evidence {
    Evidence {
        file: ctx.file.to_string(),
        line: Some(line),
        detail: Some(detail),
    }
}

/// Every topic mention in one file, in fact order.
pub fn mentions(ctx: &FileContext<'_>) -> Vec<Mention> {
    let mut out = Vec::new();
    let mut add = |key: String, role: TopicRole, line: u32, detail: String| {
        out.push(Mention {
            key,
            role,
            evidence: evidence(ctx, line, detail),
        });
    };
    for fact in ctx.facts {
        match fact {
            Fact::Call { callee, args, line } => {
                if let Some(caps) = ADD_SUBSCRIPTION.captures(callee) {
                    add(caps[1].to_string(), TopicRole::Consumer, *line, callee.clone());
                    continue;
                }
                let (base, _, _) = split_callee(callee);
                if let Some(caps) = NEW_EVENT.captures(&format!("new {}", base.trim_start_matches("new ").trim())) {
                    // `new OrderPaidIntegrationEvent(...)` as a call of its own.
                    if callee.trim_start().starts_with("new ") && !caps[1].ends_with("DomainEvent") {
                        // Re-read the name in its original case from the callee.
                        let name = callee.trim().trim_start_matches("new ").trim();
                        let name = name.split('<').next().unwrap_or(name).rsplit('.').next().unwrap_or(name);
                        if name.ends_with("Event") && !name.ends_with("DomainEvent") {
                            add(name.to_string(), TopicRole::Producer, *line, callee.clone());
                        }
                        continue;
                    }
                }
                let Some(role) = role_of_callee(callee) else {
                    continue;
                };
                for key in keys_from_args(ctx, args) {
                    add(key, role, *line, callee.clone());
                }
            }
            Fact::Annotation { name, args, line } => {
                let short = name.rsplit('.').next().unwrap_or(name);
                let role = if CONSUMER_ANNOTATIONS.contains(&short) {
                    TopicRole::Consumer
                } else if PRODUCER_ANNOTATIONS.contains(&short) {
                    TopicRole::Producer
                } else {
                    continue;
                };
                for key in keys_from_args(ctx, args) {
                    add(key, role, *line, format!("@{short}"));
                }
            }
            Fact::Extends { name, line } => {
                if name.starts_with("INotificationHandler") || name.starts_with("IRequestHandler") {
                    continue;
                }
                if let Some(caps) = HANDLER_OF.captures(name.trim()) {
                    add(caps[1].to_string(), TopicRole::Consumer, *line, name.clone());
                }
            }
            _ => {}
        }
    }
    out
}

/// key -> which services produce, consume or mention it.
pub fn topic_index(
    extractions: &[(String, String, Extraction)],
    config: &ConfigIndex,
    symbols: &Symbols,
) -> HashMap<String, TopicSides> {
    let mut index: HashMap<String, TopicSides> = HashMap::new();
    for (service, file, ex) in extractions {
        let ctx = FileContext {
            service,
            file,
            facts: &ex.facts,
            config,
            symbols,
        };
        for m in mentions(&ctx) {
            let sides = index.entry(m.key).or_default();
            let set = match m.role {
                TopicRole::Producer => &mut sides.producers,
                TopicRole::Consumer => &mut sides.consumers,
                TopicRole::Unknown => &mut sides.unknown,
            };
            set.insert(service.clone());
        }
    }
    index
}

impl Matcher for Event {
    fn name(&self) -> &'static str {
        "event"
    }

    fn candidates(&self, ctx: &FileContext<'_>) -> Vec<Candidate> {
        let mut out: Vec<Candidate> = mentions(ctx)
            .into_iter()
            .map(|m| Candidate {
                target: Target::Topic {
                    key: m.key,
                    role: m.role,
                },
                kind_hint: Some(EdgeType::Event),
                evidence: m.evidence,
            })
            .collect();
        let mut families: Vec<&str> = Vec::new();
        for fact in ctx.facts {
            if let Fact::Import { path, line } = fact {
                let Some(family) = broker_family(path) else {
                    continue;
                };
                if families.contains(&family) {
                    continue;
                }
                families.push(family);
                out.push(Candidate {
                    target: Target::Broker {
                        family: family.to_string(),
                        how: format!("imports {path} ({family} client)"),
                    },
                    kind_hint: Some(EdgeType::Event),
                    evidence: evidence(ctx, *line, format!("imports {path}")),
                });
            }
        }
        out
    }
}
```

The `new XEvent` branch inside `mentions` is written the long way above; simplify it to this when implementing (the intent is: a callee of the form `new [Ns.]NameEvent[<...>]`, not a `DomainEvent`, is a Producer mention of `NameEvent`):

```rust
                if let Some(rest) = callee.trim().strip_prefix("new ") {
                    let name = rest.split(['<', '(']).next().unwrap_or(rest).trim();
                    let name = name.rsplit('.').next().unwrap_or(name).trim_end_matches("<>");
                    if name.ends_with("Event") && !name.ends_with("DomainEvent") && name.starts_with(|c: char| c.is_ascii_uppercase()) {
                        add(name.to_string(), TopicRole::Producer, *line, callee.clone());
                        continue;
                    }
                }
```

Java's `new ProducerRecord<>` does not end with `Event`, so it falls through to `role_of_callee`, where `PRODUCER_TYPES` catches it.

In `matchers/mod.rs`: `pub mod event;` and `all()` returns `vec![Box::new(http::Http), Box::new(grpc::Grpc), Box::new(event::Event)]`.

In `map/mod.rs::run`, after `extract_all`:

```rust
    let symbols = symbols::build(&extractions);
    let joins = Joins {
        proto_owner: matchers::grpc::proto_owners(&extractions, &config, services),
        topics: matchers::event::topic_index(&extractions, &config, &symbols),
        ..Joins::default()
    };
    let resolver = Resolver::new(services, &config, joins);
    let outcomes = collect_outcomes(&extractions, &config, &symbols, &resolver);
```

and `collect_outcomes` takes `symbols: &Symbols` and sets `symbols` on each `FileContext`.

- [ ] **Step 6: Run the tests until green**

Run: `cargo test -p blastradius-core 2>&1 | grep -E 'FAILED|panicked|test result'`
Expected: all pass. Likely first failures and their causes:
- `payment -> notifications` missing: `basic_publish(exchange='', routing_key='email', ...)` gives `Str("")` (skipped) and `Str("email")`; check `is_topic_key("email")` is true (it is: not in `STOP_KEYS`).
- `checkout -> accounting` missing: `_consumer.Subscribe(TopicName)` needs `TopicName` in accounting's symbols, which needs the `ASSIGN` regex to accept `private static readonly string TopicName = ... ?? "order-created";`.
- `notifications -> kafka` missing: the Java import path is `org.springframework.kafka.annotation.KafkaListener`; `broker_family` matches on `kafka`.
- An unexpected `topic:ok`: `res.send` must not be a producer (receiver `res`).

- [ ] **Step 7: Check the corpus**

Run: `CORPUS_VERBOSE=1 cargo test --test corpus -- --nocapture 2>&1 | grep -E '^\S+ +[0-9]+ code| event |topic:'`
Expected, at least: robot-shop `payment -> dispatch event inferred`; train-ticket `ts-preserve-service -> ts-notification-service event`, `ts-preserve-other-service -> ts-notification-service event`, `ts-food-service -> ts-delivery-service event`; eShop `Ordering.API -> Basket.API event`, `Catalog.API -> Ordering.API event`, `PaymentProcessor -> Ordering.API event`, `OrderProcessor -> Ordering.API event`; opentelemetry-demo lists `topic:orders` as unresolved (its consumers are not discovered services). Read every event edge and every `topic:` entry. A key that is obviously not a topic (a MIME type, a log level, a word from a `res.send`) goes into `STOP_KEYS` or tightens `is_topic_key`; write down what you changed for the docs task. All eight repositories must stay under 500 ms.

- [ ] **Step 8: Format, lint, commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E '^test result|FAILED'
git add crates/blastradius-core/src/map test/fixtures/edges-events-app crates/blastradius-core/tests/edges.rs
git commit -m "feat(map): event edges from producers to consumers, joined on topics, queues and event types" -m "The event matcher reads publish, send, produce, subscribe, consume and declare calls by library (pika, amqplib, streadway/amqp, kafkajs, sarama, Confluent.Kafka, Spring templates), listener annotations, typed event buses (new XIntegrationEvent, AddSubscription<X, H>, IIntegrationEventHandler<X>) and Go struct literals. A topic written as a constant resolves through a per-service symbol table built from Setting facts, which is how robot-shop's self.EXCHANGE and train-ticket's Queues.queueName are read. Producers join consumers through the resolver's topic index, Inferred, producer to consumer, with evidence from both sides. A client library import gives an Inferred edge to the discovered broker of its family. A topic with only one side is listed as topic:<key>."
```

---

### Task 5: Database matcher: shared databases

The http matcher already gives every service its edge to the datastore it names. What is missing is the join the spec calls "an important edge": two code services on the same database. The key is `host/dbname` from a URL, a connection string, or a host setting paired with a database setting in the same file; or a named database resource (`AddNpgsqlDbContext<X>("orderingdb")`, `GetConnectionString("orderingdb")`). A bare host with no database name is not a key: two services on the same MySQL server with different schemas do not share data, and the spec's key is "host plus database name".

**Files:**
- Create: `crates/blastradius-core/src/map/matchers/database.rs`
- Modify: `crates/blastradius-core/src/map/matchers/mod.rs` (`pub mod database;`, `all()`)
- Modify: `crates/blastradius-core/src/map/mod.rs` (`joins.databases`)
- Modify: `crates/blastradius-core/tests/edges.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct Database;
  /// `mongodb://mongodb:27017/catalogue` -> `mongodb/catalogue`; `jdbc:mysql://mysql:3306/shop?useSSL=false` -> `mysql/shop`;
  /// `redis://redis:6379/0` -> `redis/0`; a URL without a path, or a non-database scheme, -> None.
  pub fn key_from_url(text: &str) -> Option<String>
  /// ADO.NET `Host=x;Database=y` -> `x/y`; PDO `mysql:host=mysql;dbname=ratings` -> `mysql/ratings`.
  pub fn key_from_connection_string(text: &str) -> Option<String>
  /// Every database key one file uses, with the evidence for each.
  pub fn keys(ctx: &FileContext<'_>) -> Vec<(String, Evidence)>
  pub fn database_index(extractions, config: &ConfigIndex, symbols: &Symbols) -> HashMap<String, BTreeSet<String>>
  ```
- Consumes: `Target::Database`, `Joins.databases` (Task 3); `parse_url`, `DATABASE_SCHEMES` (make `DATABASE_SCHEMES` `pub(crate)` in `resolve.rs`), `ado_connection` (Task 2); `env_default`, `Part`.

- [ ] **Step 1: Write the failing tests**

Unit tests at the end of `database.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::config::ConfigIndex;
    use crate::map::facts::{Arg, Part};
    use crate::map::symbols::Symbols;
    use pretty_assertions::assert_eq;

    #[test]
    fn keys_from_urls_and_connection_strings() {
        assert_eq!(key_from_url("mongodb://mongodb:27017/catalogue"), Some("mongodb/catalogue".into()));
        assert_eq!(key_from_url("jdbc:mysql://mysql:3306/shop?useSSL=false"), Some("mysql/shop".into()));
        assert_eq!(key_from_url("postgres://app:secret@postgres/shop?sslmode=disable"), Some("postgres/shop".into()));
        assert_eq!(key_from_url("redis://redis:6379/0"), Some("redis/0".into()));
        assert_eq!(key_from_url("mongodb://user:pw@ts-order-mongo:27017/ts-order?authSource=admin"), Some("ts-order-mongo/ts-order".into()));
        assert_eq!(key_from_url("redis://redis:6379"), None);
        assert_eq!(key_from_url("http://catalogue:8080/products"), None);
        assert_eq!(key_from_url("mongodb://mongodb:27017/"), None);
        assert_eq!(key_from_connection_string("Host=localhost;Database=LedgerDB;Username=postgres"), Some("localhost/ledgerdb".into()));
        assert_eq!(key_from_connection_string("mysql:host=mysql;dbname=ratings;charset=utf8mb4"), Some("mysql/ratings".into()));
        assert_eq!(key_from_connection_string("Host=localhost;Username=postgres"), None);
    }

    #[test]
    fn keys_of_a_file_from_every_shape() {
        let mut cfg = ConfigIndex::default();
        cfg.insert_for_test("reports", "DB_CONNECTION_STRING", "postgres://app:secret@postgres/shop", "docker-compose.yml", 9);
        let symbols = Symbols::default();
        let facts = vec![
            Fact::Str { value: "mongodb://mongodb:27017/catalogue".into(), line: 1 },
            Fact::Template { parts: vec![Part::Lit("jdbc:mysql://".into()), Part::Var("DB_HOST:mysql".into()), Part::Lit(":3306/".into()), Part::Var("DB_NAME:shop".into()), Part::Lit("?useSSL=false".into())], line: 2 },
            Fact::Template { parts: vec![Part::Lit("jdbc:mysql://".into()), Part::Var("UNKNOWN_HOST".into()), Part::Lit(":3306/x".into())], line: 3 },
            Fact::EnvRef { name: "DB_CONNECTION_STRING".into(), default: None, line: 4 },
            Fact::EnvRef { name: "LEGACY_DB_URL".into(), default: Some("mysql://mysql:3306/shop".into()), line: 5 },
            Fact::Setting { key: "spring.data.mongodb.host".into(), value: "mongodb".into(), line: 6 },
            Fact::Setting { key: "spring.data.mongodb.database".into(), value: "inventory".into(), line: 7 },
            Fact::Call { callee: "builder.AddNpgsqlDbContext<LedgerContext>".into(), args: vec![Arg::Str("ledgerdb".into())], line: 8 },
            Fact::Call { callee: "builder.Configuration.GetConnectionString".into(), args: vec![Arg::Str("OrderingDb".into())], line: 9 },
            Fact::Call { callee: "builder.AddRedisClient".into(), args: vec![Arg::Str("redis".into())], line: 10 },
            Fact::Call { callee: "mysql.createConnection".into(), args: vec![Arg::Other("{ host: 'mysql', user: 'x', database: 'cities' }".into())], line: 11 },
            Fact::Str { value: "Host=localhost;Database=LedgerDB;Username=postgres".into(), line: 12 },
        ];
        let ctx = FileContext { service: "reports", file: "reports/x", facts: &facts, config: &cfg, symbols: &symbols };
        let got: Vec<(String, u32, String)> = keys(&ctx)
            .into_iter()
            .map(|(k, e)| (k, e.line.unwrap(), e.detail.unwrap()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("mongodb/catalogue".into(), 1, "mongodb://mongodb:27017/catalogue".into()),
                ("mysql/shop".into(), 2, "jdbc:mysql://mysql:3306/shop?useSSL=false".into()),
                ("postgres/shop".into(), 4, "DB_CONNECTION_STRING=postgres://app:secret@postgres/shop via docker-compose.yml:9".into()),
                ("mysql/shop".into(), 5, "LEGACY_DB_URL default mysql://mysql:3306/shop".into()),
                ("ledgerdb".into(), 8, "builder.AddNpgsqlDbContext<LedgerContext>(\"ledgerdb\")".into()),
                ("orderingdb".into(), 9, "builder.Configuration.GetConnectionString(\"OrderingDb\")".into()),
                ("mysql/cities".into(), 11, "mysql.createConnection host=mysql database=cities".into()),
                ("localhost/ledgerdb".into(), 12, "Host=localhost;Database=LedgerDB;Username=postgres".into()),
                ("mongodb/inventory".into(), 7, "spring.data.mongodb.host=mongodb, spring.data.mongodb.database=inventory".into()),
            ]
        );
    }
}
```

The paired setting comes last because it is emitted after the loop. `insert_for_test` is `#[cfg(test)] pub(crate)` already.

Integration test in `tests/edges.rs`:

```rust
#[test]
fn shared_databases_join_services_in_both_directions() {
    let (edges, _) = edges_of("edges-db-app");
    for (s, t, key) in [
        ("orders", "reports", "mysql/shop"),
        ("reports", "orders", "mysql/shop"),
        ("ledger", "audit", "ledgerdb"),
        ("audit", "ledger", "ledgerdb"),
    ] {
        let e = find(&edges, s, t, EdgeType::Database);
        assert_eq!(e.confidence, Confidence::Inferred, "{s} -> {t}");
        assert!(
            e.evidence.iter().any(|v| v.detail.as_deref() == Some(&format!("shared database {key} with {t}"))),
            "{s} -> {t}: {:?}",
            e.evidence
        );
    }
    let e = find(&edges, "ledger", "audit", EdgeType::Database);
    assert!(
        e.evidence.iter().any(|v| v.detail.as_deref() == Some("shared database localhost/ledgerdb with audit")),
        "the development connection strings share the same key too: {:?}",
        e.evidence
    );
    assert!(
        !edges.iter().any(|e| e.edge_type == EdgeType::Database && e.source == "catalogue" && e.target == "user"),
        "same host, different databases: not shared"
    );
    assert!(
        !edges.iter().any(|e| e.edge_type == EdgeType::Database && e.source == "reports" && e.target == "ledger"),
        "postgres host alone is not a key"
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p blastradius-core 2>&1 | grep -E 'error\[|FAILED|test result' | head`
Expected: compile error, `database` module missing.

- [ ] **Step 3: `matchers/database.rs`**

```rust
//! Which database each service uses, keyed by host and database name, so two
//! services on the same key get a database edge between them. The edge to the
//! datastore itself comes from the http matcher, which types edges by target.
use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;

use regex::Regex;

use super::{FileContext, Matcher};
use crate::map::config::ConfigIndex;
use crate::map::facts::{Arg, Extraction, Fact, Part, env_default};
use crate::map::resolve::{DATABASE_SCHEMES, ado_connection, is_hostish_key, is_hostname, parse_url};
use crate::map::symbols::Symbols;
use crate::map::{Candidate, Target};
use crate::model::{EdgeType, Evidence};

pub struct Database;

static PDO_DSN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:mysql|pgsql|sqlsrv|oci|dblib|odbc):host=([A-Za-z0-9.-]+).*?dbname=([^;\s]+)").unwrap()
});
/// `AddNpgsqlDbContext<X>("name")`, `AddNpgsqlDataSource("name")`,
/// `AddSqlServerDbContext<X>("name")`, `AddMongoDBClient("name")`: the first
/// string argument is an Aspire database resource. Redis and brokers carry no
/// database name and are left out.
static NAMED_RESOURCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\.(Add(?:Npgsql|SqlServer|MySql|Oracle|MongoDB|Cosmos)\w*(?:DbContext|DataSource|Client)|GetConnectionString)$").unwrap()
});
static OBJECT_HOST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\bhost(?:name)?\s*[:=]\s*['"]([^'"]+)['"]"#).unwrap());
static OBJECT_DATABASE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\b(?:database|db|dbname)\s*[:=]\s*['"]([^'"]+)['"]"#).unwrap());
const DATABASE_KEYS: &[&str] = &[
    "database", "dbname", "db", "database-name", "databasename", "database_name", "schema",
    "catalog", "initial-catalog",
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
    let path = after_scheme.split(['?', '#']).next().unwrap_or(after_scheme);
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
    Some(format!("{}/{}", caps[1].to_lowercase(), caps[2].to_lowercase()))
}

fn key_from_value(text: &str) -> Option<String> {
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

/// Every database key one file uses, in fact order, one entry per key.
pub fn keys(ctx: &FileContext<'_>) -> Vec<(String, Evidence)> {
    let mut out: Vec<(String, Evidence)> = Vec::new();
    let mut host_setting: Option<(String, String)> = None;
    let mut db_setting: Option<(String, String, u32)> = None;
    let mut push = |key: String, ev: Evidence| {
        if !out.iter().any(|(k, _)| *k == key) {
            out.push((key, ev));
        }
    };
    for fact in ctx.facts {
        match fact {
            Fact::Str { value, line } => {
                if let Some(key) = key_from_value(value) {
                    push(key, evidence(ctx, *line, value.clone()));
                }
            }
            Fact::Template { parts, line } => {
                if let Some(rendered) = render(ctx, parts) {
                    if let Some(key) = key_from_value(&rendered) {
                        push(key, evidence(ctx, *line, rendered));
                    }
                }
            }
            Fact::EnvRef { name, default, line } => {
                if let Some(configured) = ctx.config.lookup(ctx.service, name) {
                    if let Some(key) = key_from_value(&configured.value) {
                        let at = match configured.evidence.line {
                            Some(l) => format!("{}:{l}", configured.evidence.file),
                            None => configured.evidence.file.clone(),
                        };
                        push(key, evidence(ctx, *line, format!("{name}={} via {at}", configured.value)));
                        continue;
                    }
                }
                if let Some(default) = default {
                    if let Some(key) = key_from_value(default) {
                        push(key, evidence(ctx, *line, format!("{name} default {default}")));
                    }
                }
            }
            Fact::Setting { key, value, line } => {
                if let Some(k) = key_from_value(value) {
                    push(k, evidence(ctx, *line, format!("{key}={value}")));
                    continue;
                }
                let last = key.rsplit('.').next().unwrap_or(key).to_lowercase();
                if DATABASE_KEYS.contains(&last.as_str()) && db_setting.is_none() && !value.contains('$') {
                    db_setting = Some((key.clone(), value.clone(), *line));
                } else if is_hostish_key(key) && host_setting.is_none() && is_hostname(value) {
                    host_setting = Some((key.clone(), value.clone()));
                }
            }
            Fact::Call { callee, args, line } => {
                let plain = strip_generics(callee);
                if NAMED_RESOURCE.is_match(&plain) {
                    if let Some(Arg::Str(name)) = args.iter().find(|a| matches!(a, Arg::Str(_))) {
                        push(name.to_lowercase(), evidence(ctx, *line, format!("{callee}(\"{name}\")")));
                    }
                    continue;
                }
                for arg in args {
                    let Arg::Other(text) = arg else { continue };
                    if !text.trim_start().starts_with('{') {
                        continue;
                    }
                    if let (Some(h), Some(d)) = (OBJECT_HOST.captures(text), OBJECT_DATABASE.captures(text)) {
                        let (host, db) = (h[1].to_lowercase(), d[1].to_lowercase());
                        push(format!("{host}/{db}"), evidence(ctx, *line, format!("{callee} host={host} database={db}")));
                    }
                }
            }
            _ => {}
        }
    }
    if let (Some((hk, host)), Some((dk, db, line))) = (host_setting, db_setting) {
        push(
            format!("{}/{}", host.to_lowercase(), db.to_lowercase()),
            evidence(ctx, line, format!("{hk}={host}, {dk}={db}")),
        );
    }
    out
}

/// key -> services using it.
pub fn database_index(
    extractions: &[(String, String, Extraction)],
    config: &ConfigIndex,
    symbols: &Symbols,
) -> HashMap<String, BTreeSet<String>> {
    let mut index: HashMap<String, BTreeSet<String>> = HashMap::new();
    for (service, file, ex) in extractions {
        let ctx = FileContext {
            service,
            file,
            facts: &ex.facts,
            config,
            symbols,
        };
        for (key, _) in keys(&ctx) {
            index.entry(key).or_default().insert(service.clone());
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
            .map(|(key, evidence)| Candidate {
                target: Target::Database(key),
                kind_hint: Some(EdgeType::Database),
                evidence,
            })
            .collect()
    }
}
```

`parse_url` lowercases the host, so keys compare case-insensitively on both halves. Make `DATABASE_SCHEMES` `pub(crate)` in `resolve.rs`. Register `pub mod database;` and `Box::new(database::Database)` in `matchers/mod.rs`, and in `run` set `databases: matchers::database::database_index(&extractions, &config, &symbols)` in `Joins`.

- [ ] **Step 4: Run the tests until green**

Run: `cargo test -p blastradius-core 2>&1 | grep -E 'FAILED|panicked|test result'`
Expected: all pass. If `orders <-> reports` is missing, the template in `orders/.../application.yml` did not render: `env_default("DB_HOST:mysql")` must return `("DB_HOST", Some("mysql"))` (it does), and the `Template` fact must come from the regex extractor's `push_string`. If `catalogue -> user` appears, `key_from_url` is dropping the database name.

- [ ] **Step 5: Check the corpus**

Run: `CORPUS_VERBOSE=1 cargo test --test corpus -- --nocapture 2>&1 | grep -E '^\S+ +[0-9]+ code|shared database'`
Expected: eShop `Ordering.API -> OrderProcessor database inferred` and the reverse (keys `orderingdb` and `localhost/orderingdb`); train-ticket edges among `ts-order-service`, `ts-price-service`, `ts-route-service`, `ts-station-food-service` (they all default to `10.176.122.1/ts`); nothing in robot-shop, microservices-demo, petclinic or ewolff. Every shared-database edge must name two code services that really do read the same database name; anything else means a key is too loose.

- [ ] **Step 6: Format, lint, commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E '^test result|FAILED'
git add crates/blastradius-core/src/map crates/blastradius-core/tests/edges.rs
git commit -m "feat(map): database edges between services that share a database" -m "Each file's database keys come from database URLs, JDBC and ADO.NET connection strings, PDO DSNs, environment variables through their configured value or default, templates rendered with their defaults, a host setting paired with a database setting in the same file, Aspire named resources (AddNpgsqlDbContext(\"orderingdb\"), GetConnectionString) and client options objects. The key is host plus database name, or the resource name; a bare host is not a key. Two code services on one key get an Inferred database edge each way naming the key, which is how eShop's Ordering.API and OrderProcessor, and four train-ticket services defaulting to one MySQL schema, show up."
```

---

### Task 6: Package index and the import matcher

**Files:**
- Modify: `crates/blastradius-core/src/map/config.rs` (`read_packages`, tests)
- Create: `crates/blastradius-core/src/map/matchers/import.rs`
- Modify: `crates/blastradius-core/src/map/matchers/mod.rs` (`pub mod import;`, `all()`)
- Create: `test/fixtures/edges-import-app/**`
- Modify: `crates/blastradius-core/tests/edges.rs`

**Interfaces:**
- Produces: `ConfigIndex.packages` filled for every code service from its manifest; `pub struct Import;` implementing `Matcher`.
- Consumes: `Target::Package`, `Target::PackagePath`, `Package` (Task 3); `Fact::Import`, `Fact::Str`, `Fact::Setting`; `crate::fs::{normalize, posix}`.

- [ ] **Step 1: Create the fixture**

```
test/fixtures/edges-import-app/apps/web/package.json
{ "name": "web", "dependencies": { "@acme/shared": "workspace:*", "express": "^4" } }

test/fixtures/edges-import-app/apps/web/src/index.js
import { fmt } from '@acme/shared/utils';
import express from 'express';

test/fixtures/edges-import-app/packages/shared/package.json
{ "name": "@acme/shared" }

test/fixtures/edges-import-app/services/cart/go.mod
module github.com/acme/demo/services/cart

go 1.22

test/fixtures/edges-import-app/services/cart/main.go
package main

func main() {}

test/fixtures/edges-import-app/services/checkout/go.mod
module github.com/acme/demo/services/checkout

go 1.22

test/fixtures/edges-import-app/services/checkout/main.go
package main

import (
	pb "github.com/acme/demo/services/cart/genproto"
	"github.com/acme/demo/services/checkout/money"
	"google.golang.org/grpc"
)

func main() {}

test/fixtures/edges-import-app/services/basket-api/Basket.API.csproj
<Project Sdk="Microsoft.NET.Sdk.Web">
  <ItemGroup>
    <ProjectReference Include="..\eventbus\EventBus.csproj" />
    <PackageReference Include="Grpc.AspNetCore" />
  </ItemGroup>
</Project>

test/fixtures/edges-import-app/services/eventbus/EventBus.csproj
<Project Sdk="Microsoft.NET.Sdk"></Project>

test/fixtures/edges-import-app/services/orders/pom.xml
<project>
  <parent>
    <groupId>demo</groupId>
    <artifactId>demo-parent</artifactId>
  </parent>
  <artifactId>orders</artifactId>
  <dependencies>
    <dependency>
      <groupId>demo</groupId>
      <artifactId>common</artifactId>
    </dependency>
    <dependency>
      <groupId>org.springframework.boot</groupId>
      <artifactId>spring-boot-starter-web</artifactId>
    </dependency>
  </dependencies>
</project>

test/fixtures/edges-import-app/services/common/pom.xml
<project>
  <artifactId>common</artifactId>
</project>

test/fixtures/edges-import-app/services/indexer/Cargo.toml
[package]
name = "indexer"

[dependencies]
core-rs = { path = "../core-rs" }
serde = "1"

test/fixtures/edges-import-app/services/core-rs/Cargo.toml
[package]
name = "core-rs"

test/fixtures/edges-import-app/services/worker/requirements.txt
requests

test/fixtures/edges-import-app/services/worker/main.py
from shared_py.tasks import run
import requests

test/fixtures/edges-import-app/services/shared_py/requirements.txt

test/fixtures/edges-import-app/services/shared_py/tasks.py
def run():
    pass
```

`shared_py/requirements.txt` is an empty file (the manifest that makes the directory a service). The Go import block uses tabs.

- [ ] **Step 2: Write the failing tests**

`config.rs` tests:

```rust
    #[test]
    fn packages_are_read_from_every_manifest_kind() {
        let cfg = build("edges-import-app");
        let mut got: Vec<(&str, &str)> = cfg
            .packages
            .iter()
            .map(|p| (p.name.as_str(), p.service.as_str()))
            .collect();
        got.sort_unstable();
        assert_eq!(
            got,
            vec![
                ("@acme/shared", "shared"),
                ("Basket.API", "basket-api"),
                ("EventBus", "eventbus"),
                ("common", "common"),
                ("core-rs", "core-rs"),
                ("github.com/acme/demo/services/cart", "cart"),
                ("github.com/acme/demo/services/checkout", "checkout"),
                ("indexer", "indexer"),
                ("orders", "orders"),
                ("shared_py", "shared_py"),
                ("web", "web"),
                ("worker", "worker"),
            ]
        );
        let p = cfg.packages.iter().find(|p| p.name == "orders").unwrap();
        assert_eq!(p.evidence.file, "services/orders/pom.xml");
    }
```

`tests/edges.rs`:

```rust
#[test]
fn import_edges_from_imports_dependencies_and_project_references() {
    let (edges, json) = edges_of("edges-import-app");
    let e = find(&edges, "web", "shared", EdgeType::Import);
    assert_eq!(e.confidence, Confidence::Static);
    let details: Vec<&str> = e.evidence.iter().filter_map(|v| v.detail.as_deref()).collect();
    assert!(details.contains(&"import @acme/shared/utils"), "{details:?}");
    assert!(details.contains(&"dependency @acme/shared"), "{details:?}");
    assert_eq!(
        find(&edges, "checkout", "cart", EdgeType::Import).evidence[0].detail.as_deref(),
        Some("import github.com/acme/demo/services/cart/genproto")
    );
    assert_eq!(
        find(&edges, "basket-api", "eventbus", EdgeType::Import).evidence[0].detail.as_deref(),
        Some("ProjectReference ..\\eventbus\\EventBus.csproj")
    );
    assert_eq!(
        find(&edges, "orders", "common", EdgeType::Import).evidence[0].detail.as_deref(),
        Some("artifactId common")
    );
    assert_eq!(
        find(&edges, "indexer", "core-rs", EdgeType::Import).evidence[0].detail.as_deref(),
        Some("path ../core-rs")
    );
    assert_eq!(
        find(&edges, "worker", "shared_py", EdgeType::Import).evidence[0].detail.as_deref(),
        Some("import shared_py.tasks")
    );
    assert!(edges.iter().all(|e| e.edge_type == EdgeType::Import), "{:?}", edges.iter().map(triple).collect::<Vec<_>>());
    assert_eq!(edges.len(), 6, "{:?}", edges.iter().map(triple).collect::<Vec<_>>());
    assert_eq!(json.mapping.unresolved, 0, "external libraries are not unresolved targets: {:?}", json.mapping.unresolved_targets);
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p blastradius-core 2>&1 | grep -E 'error\[|FAILED|test result' | head`
Expected: `packages_are_read_from_every_manifest_kind` fails with an empty list; the edges test fails on `web -> shared`.

- [ ] **Step 4: `read_packages` in `config.rs`**

Call `cfg.read_packages(root, services);` from `build`, after `read_protos`. Add:

```rust
static GO_MODULE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^module\s+(\S+)").unwrap());
static POM_PARENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<parent>.*?</parent>").unwrap());
static POM_ARTIFACT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<artifactId>\s*([^<\s]+)\s*</artifactId>").unwrap());

impl ConfigIndex {
    /// The names each code service can be imported by, from its manifest.
    fn read_packages(&mut self, root: &Path, services: &[Service]) {
        for s in services.iter().filter(|s| s.role == ServiceRole::Code) {
            let Some(dir) = s.root.as_deref() else { continue };
            let mut add = |name: String, file: String| {
                let name = name.trim().to_string();
                if name.is_empty()
                    || self
                        .packages
                        .iter()
                        .any(|p| p.name == name && p.service == s.name)
                {
                    return;
                }
                self.packages.push(Package {
                    name,
                    service: s.name.clone(),
                    evidence: Evidence {
                        file,
                        line: None,
                        detail: None,
                    },
                });
            };
            let at = |file: &str| {
                if dir == "." {
                    file.to_string()
                } else {
                    format!("{dir}/{file}")
                }
            };
            if let Some(name) = &s.package_name {
                add(name.clone(), at("package.json"));
            }
            if let Some(pkg) = crate::fs::read_json(&root.join(dir).join("package.json")) {
                if let Some(name) = pkg.get("name").and_then(|v| v.as_str()) {
                    add(name.to_string(), at("package.json"));
                }
            }
            if let Some(text) = read_text(&root.join(dir).join("go.mod")) {
                if let Some(caps) = GO_MODULE.captures(&text) {
                    add(caps[1].to_string(), at("go.mod"));
                }
            }
            if let Some(text) = read_text(&root.join(dir).join("Cargo.toml")) {
                if let Some(name) = toml::from_str::<toml::Table>(&text)
                    .ok()
                    .and_then(|t| t.get("package")?.as_table()?.get("name")?.as_str().map(str::to_string))
                {
                    add(name, at("Cargo.toml"));
                }
            }
            if let Some(text) = read_text(&root.join(dir).join("pom.xml")) {
                let own = POM_PARENT.replace(&text, "");
                if let Some(caps) = POM_ARTIFACT.captures(&own) {
                    add(caps[1].to_string(), at("pom.xml"));
                }
            }
            if let Some(pkg) = crate::fs::read_json(&root.join(dir).join("composer.json")) {
                if let Some(name) = pkg.get("name").and_then(|v| v.as_str()) {
                    add(name.to_string(), at("composer.json"));
                }
            }
            if let Some(text) = read_text(&root.join(dir).join("pyproject.toml")) {
                if let Ok(t) = toml::from_str::<toml::Table>(&text) {
                    let name = t
                        .get("project")
                        .and_then(|p| p.get("name"))
                        .or_else(|| t.get("tool")?.get("poetry")?.get("name"))
                        .and_then(|v| v.as_str());
                    if let Some(name) = name {
                        add(name.to_string(), at("pyproject.toml"));
                    }
                }
            }
            if let Ok(entries) = std::fs::read_dir(root.join(dir)) {
                let mut projects: Vec<String> = entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n.ends_with(".csproj") || n.ends_with(".fsproj"))
                    .collect();
                projects.sort();
                for project in projects {
                    let stem = project.rsplit_once('.').map_or(project.as_str(), |(s, _)| s).to_string();
                    add(stem, at(&project));
                }
            }
            if s.language.as_deref() == Some("python") {
                let base = dir.rsplit('/').next().unwrap_or(dir).to_string();
                add(base, at(""));
            }
        }
    }
}
```

The `add` closure borrows `self.packages` mutably while `s` is borrowed from `services`; that compiles because `services` is a separate slice. If the borrow checker objects to `self` inside the closure, collect into a local `Vec<Package>` and `self.packages.extend(...)` after the loop. For the Python directory case use `file: at("")` trimmed of its trailing slash (write `at("requirements.txt")` when that file exists, else the directory).

- [ ] **Step 5: `matchers/import.rs`**

```rust
//! Cross-service imports: a package another discovered service declares,
//! imported in code or listed as a dependency; a project or path reference
//! from one manifest to another service's directory. These are the most
//! reliable edges there are, so they are Static. An import that matches no
//! discovered package is an external library and is dropped without a trace.
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::{FileContext, Matcher};
use crate::fs::{normalize, posix};
use crate::map::facts::{Fact};
use crate::map::{Candidate, Target};
use crate::model::{EdgeType, Evidence};

pub struct Import;

/// npm, composer, PyPI and Cargo names: `@acme/shared`, `vendor/pkg`, `core-rs`.
static PACKAGE_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(@[a-z0-9][\w.-]*/)?[a-z0-9][\w.-]*$").unwrap());

fn evidence(ctx: &FileContext<'_>, line: u32, detail: &str) -> Evidence {
    Evidence {
        file: ctx.file.to_string(),
        line: Some(line),
        detail: Some(detail.to_string()),
    }
}

fn package(ctx: &FileContext<'_>, name: &str, how: String, line: u32) -> Candidate {
    Candidate {
        target: Target::Package {
            name: name.to_string(),
            how: how.clone(),
        },
        kind_hint: Some(EdgeType::Import),
        evidence: evidence(ctx, line, &how),
    }
}

/// `..\EventBus\EventBus.csproj` next to `src/Basket.API/Basket.API.csproj`
/// -> `src/EventBus/EventBus.csproj`.
fn resolve_relative(file: &str, relative: &str) -> String {
    let dir = file.rsplit_once('/').map_or("", |(d, _)| d);
    let joined = Path::new(dir).join(relative.replace('\\', "/"));
    let clean = posix(&normalize(&joined));
    clean.trim_start_matches("./").to_string()
}

fn path_candidate(ctx: &FileContext<'_>, relative: &str, how: String, line: u32) -> Candidate {
    Candidate {
        target: Target::PackagePath {
            path: resolve_relative(ctx.file, relative),
            how: how.clone(),
        },
        kind_hint: Some(EdgeType::Import),
        evidence: evidence(ctx, line, &how),
    }
}

impl Matcher for Import {
    fn name(&self) -> &'static str {
        "import"
    }

    fn candidates(&self, ctx: &FileContext<'_>) -> Vec<Candidate> {
        let mut out = Vec::new();
        let basename = ctx.file.rsplit('/').next().unwrap_or(ctx.file).to_lowercase();
        let is_project = basename.ends_with(".csproj") || basename.ends_with(".fsproj");
        for fact in ctx.facts {
            match fact {
                Fact::Import { path, line } => {
                    let p = path.trim();
                    if p.is_empty() || p.starts_with(['.', '/']) || p.contains("://") {
                        continue;
                    }
                    out.push(package(ctx, p, format!("import {p}"), *line));
                }
                Fact::Str { value, line } => {
                    let v = value.trim();
                    let lower = v.to_lowercase();
                    if is_project && (lower.ends_with(".csproj") || lower.ends_with(".fsproj")) {
                        out.push(path_candidate(ctx, v, format!("ProjectReference {v}"), *line));
                    } else if basename == "cargo.toml" && (v.starts_with("./") || v.starts_with("../")) {
                        out.push(path_candidate(ctx, v, format!("path {v}"), *line));
                    } else if matches!(basename.as_str(), "package.json" | "composer.json") && PACKAGE_NAME.is_match(v) {
                        out.push(package(ctx, v, format!("dependency {v}"), *line));
                    } else if basename == "pyproject.toml" {
                        let name = v.split([' ', '<', '>', '=', '!', '~', ';', '[']).next().unwrap_or(v);
                        if PACKAGE_NAME.is_match(name) {
                            out.push(package(ctx, name, format!("dependency {v}"), *line));
                        }
                    }
                }
                Fact::Setting { key, value, line } if basename == "pom.xml" => {
                    if key == "artifactId" || key == "module" {
                        out.push(package(ctx, value, format!("{key} {value}"), *line));
                    }
                }
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_project_paths_resolve_against_the_manifest_directory() {
        assert_eq!(
            resolve_relative("src/Basket.API/Basket.API.csproj", "..\\EventBus\\EventBus.csproj"),
            "src/EventBus/EventBus.csproj"
        );
        assert_eq!(
            resolve_relative("src/HybridApp/HybridApp.csproj", "..\\..\\src\\WebAppComponents\\WebAppComponents.csproj"),
            "src/WebAppComponents/WebAppComponents.csproj"
        );
        assert_eq!(resolve_relative("services/indexer/Cargo.toml", "../core-rs"), "services/core-rs");
        assert_eq!(resolve_relative("Cargo.toml", "./crates/core"), "crates/core");
    }
}
```

Note the `pom.xml` arm: the parent's `artifactId` and the file's own `artifactId` are candidates too; the own name is a self-edge (dropped) and the parent matches no package (dropped). `PackagePath` for a directory (`services/core-rs`) resolves because `owner_of_path` treats a path equal to a root as owned once the check is `path == root || path.starts_with(root + "/")`: adjust `owner_of_path` to `*r == "." || path == *r || path.starts_with(*r) && path[r.len()..].starts_with('/')`.

Register `pub mod import;` and `Box::new(import::Import)` in `matchers/mod.rs`.

- [ ] **Step 6: Run the tests until green**

Run: `cargo test -p blastradius-core 2>&1 | grep -E 'FAILED|panicked|test result'`
Expected: all pass. If `edges.len()` is 7 with a `checkout -> checkout`, self-edges are not being dropped for `Package` (they are, through `finish`). If `orders -> common` is missing, `Fact::Setting` for `<artifactId>common</artifactId>` is not produced: the XML regex needs the line to hold the whole element.

- [ ] **Step 7: Check the corpus and this repository**

Run: `CORPUS_VERBOSE=1 cargo test --test corpus -- --nocapture 2>&1 | grep -E '^\S+ +[0-9]+ code| import '`
Expected: eShop gains its ProjectReference edges (Basket.API → eShop.ServiceDefaults and EventBusRabbitMQ, EventBusRabbitMQ → EventBus, Ordering.API → Ordering.Domain and Ordering.Infrastructure, WebApp → WebAppComponents, HybridApp → WebAppComponents, eShop.AppHost → each API); nothing elsewhere (train-ticket's `ts-common` is not a discovered service; petclinic and ewolff poms reference only parents). Then `cargo run -q -- analyze . --no-color` on this repository must show `blastradius-cli -> blastradius-core import static` from the CLI crate's path dependency.

- [ ] **Step 8: Format, lint, commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E '^test result|FAILED'
git add crates/blastradius-core/src/map test/fixtures/edges-import-app crates/blastradius-core/tests/edges.rs
git commit -m "feat(map): import edges from packages, modules, artifacts and project references" -m "Every code service declares the names it can be imported by: package.json and composer names, Go module paths, Cargo and Maven artifacts, .csproj stems, and for Python its directory. Imports in code resolve by exact name or a prefix ending at a separator; dependencies in manifests, Maven sibling artifacts, Cargo path dependencies and .csproj ProjectReferences resolve too, the last two by the service whose root owns the referenced path. All Static. Imports of anything else are external libraries and are dropped, not counted as unresolved."
```

---

### Task 7: Report: shared databases and the no-edges line

**Files:**
- Modify: `crates/blastradius-core/src/report.rs` (`structure`, `findings`)
- Modify: `crates/blastradius-core/tests/edges.rs`

**Interfaces:**
- Consumes: database edges whose evidence detail starts with `shared database <key> with ` (Task 5).

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn report_lists_shared_databases() {
    let r = format_repo_report(&analyze(&fixture("edges-db-app")).unwrap(), false);
    assert!(
        regex::Regex::new(r"Shared databases\s+2").unwrap().is_match(&r),
        "{r}"
    );
    assert!(
        regex::Regex::new(r"ledgerdb\s+audit, ledger").unwrap().is_match(&r),
        "{r}"
    );
    assert!(
        regex::Regex::new(r"mysql/shop\s+orders, reports").unwrap().is_match(&r),
        "{r}"
    );
    assert!(!r.contains("localhost/ledgerdb"), "one key per pair, the named resource wins: {r}");
}

#[test]
fn report_without_shared_databases_says_so_and_names_every_edge_type() {
    let r = format_repo_report(&analyze(&fixture("edges-http-app")).unwrap(), false);
    assert!(
        regex::Regex::new(r"Shared databases\s+0").unwrap().is_match(&r),
        "{r}"
    );
    let r = format_repo_report(&analyze(&fixture("compose-app")).unwrap(), false);
    assert!(
        r.contains("No static edges found: no HTTP, gRPC, event, database or import edge to another discovered service was recognised."),
        "{r}"
    );
}
```

The `ledgerdb` pair is shared under two keys (`ledgerdb` and `localhost/ledgerdb`). The finding shows one line per pair of services, choosing the key of the edge's first evidence entry; evidence is ordered strongest first, then by file and line, so make the named resource win by listing pairs under the first key seen for that pair.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p blastradius-core --test edges report 2>&1 | grep -E 'FAILED|panicked|test result'`
Expected: both new tests fail (no "Shared databases" line; old wording).

- [ ] **Step 3: Implement**

In `structure`, change the no-edges line to:

```rust
            c.dim("No static edges found: no HTTP, gRPC, event, database or import edge to another discovered service was recognised.")
```

In `findings`, after the "Most connected" block:

```rust
    out.push(String::new());
    // One line per pair of code services that read the same database, under
    // the key their strongest evidence names.
    let code_names: BTreeSet<&str> = services
        .iter()
        .filter(|s| s.role == ServiceRole::Code)
        .map(|s| s.name.as_str())
        .collect();
    let mut shared: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
    let mut seen_pairs: BTreeSet<(&str, &str)> = BTreeSet::new();
    for e in edges.iter().filter(|e| {
        e.edge_type == EdgeType::Database
            && code_names.contains(e.source.as_str())
            && code_names.contains(e.target.as_str())
    }) {
        let pair = if e.source <= e.target {
            (e.source.as_str(), e.target.as_str())
        } else {
            (e.target.as_str(), e.source.as_str())
        };
        if !seen_pairs.insert(pair) {
            continue;
        }
        let key = e.evidence.iter().find_map(|v| {
            v.detail
                .as_deref()?
                .strip_prefix("shared database ")?
                .split(" with ")
                .next()
                .map(str::to_string)
        });
        if let Some(key) = key {
            shared.entry(key).or_default().extend([pair.0, pair.1]);
        }
    }
    out.push(format!("  {:<38} {}", "Shared databases", shared.len()));
    for (key, names) in &shared {
        let list = names.iter().copied().collect::<Vec<_>>().join(", ");
        out.push(format!("    {key:<36} {}", c.dim(&list)));
    }
```

Import `EdgeType` in `report.rs`. Because both directions of a pair carry the same evidence details in a different order, the reverse edge is skipped by `seen_pairs`; the first edge in sorted order (`audit -> ledger`) decides the key. Its evidence is sorted strongest first then by file; for `audit`, `Program.cs` (`ledgerdb`) sorts before `appsettings.Development.json`? No: `appsettings.Development.json` < `Program.cs` alphabetically, and both are Inferred, so `localhost/ledgerdb` would win. Fix in `merge_edges` is wrong (evidence order is a contract); instead, when collecting the key here, prefer a key without a `/` when the edge's evidence offers both:

```rust
        let keys: Vec<String> = e
            .evidence
            .iter()
            .filter_map(|v| {
                v.detail
                    .as_deref()?
                    .strip_prefix("shared database ")?
                    .split(" with ")
                    .next()
                    .map(str::to_string)
            })
            .collect();
        let key = keys
            .iter()
            .find(|k| !k.contains('/'))
            .or_else(|| keys.first())
            .cloned();
```

- [ ] **Step 4: Run, format, lint, commit**

```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E '^test result|FAILED'
git add crates/blastradius-core/src/report.rs crates/blastradius-core/tests/edges.rs
git commit -m "feat(report): shared databases finding" -m "FINDINGS lists how many databases two or more code services read, and under each key which services. The no-edges line names every edge type the mapping stage now recognises."
```

---

### Task 8: Corpus expected edges and a junk review

**Files:**
- Modify: `test/expected/corpus/robot-shop.json`, `eshop.json`, `train-ticket.json`, `opentelemetry-demo.json` (add to / create `edges`)

- [ ] **Step 1: Run the corpus verbosely and save the output**

```bash
CORPUS_VERBOSE=1 cargo test --test corpus -- --nocapture > /tmp/corpus-after.txt 2>&1; grep -E '^\S+ +[0-9]+ code' /tmp/corpus-after.txt
```

Compare the count line per repository with the baseline in the previous plan's commit (`eshop 14 edges`, `robot-shop 19`, `train-ticket 108`, `opentelemetry-demo 28`, `spring-petclinic 17`, `microservices-demo 15`, `ewolff 5`). Every repository must still be under 500 ms.

- [ ] **Step 2: Read every new edge**

`grep -E ' (event|database|import) ' /tmp/corpus-after.txt` and, for each edge, open the file and line in `corpus/` and confirm the detail describes what is there. Junk to fix before adding expectations:
- an `event` edge whose key is not a topic (a MIME type, a log message): add the word to `STOP_KEYS` in `event.rs`, or tighten `is_topic_key`;
- a `database` share on a key two services do not really share (a host without a database name slipped through): fix `key_from_url` / `keys`;
- an `import` edge to a service that is not the package's owner: check `read_packages` for that manifest kind;
- a `topic:` entry in `unresolved:` that is not a topic: same fix as the first point.

Record each fix in one line for the docs task.

- [ ] **Step 3: Add the expected edges**

Append to the `edges` array of `test/expected/corpus/robot-shop.json`:

```json
    { "source": "payment", "target": "dispatch", "type": "event" }
```

Append to `eshop.json`:

```json
    { "source": "Basket.API", "target": "eShop.ServiceDefaults", "type": "import" },
    { "source": "Basket.API", "target": "EventBusRabbitMQ", "type": "import" },
    { "source": "EventBusRabbitMQ", "target": "EventBus", "type": "import" },
    { "source": "Ordering.API", "target": "Ordering.Infrastructure", "type": "import" },
    { "source": "Ordering.Infrastructure", "target": "Ordering.Domain", "type": "import" },
    { "source": "WebApp", "target": "WebAppComponents", "type": "import" },
    { "source": "HybridApp", "target": "WebAppComponents", "type": "import" },
    { "source": "eShop.AppHost", "target": "Ordering.API", "type": "import" },
    { "source": "Ordering.API", "target": "Basket.API", "type": "event" },
    { "source": "Ordering.API", "target": "Catalog.API", "type": "event" },
    { "source": "Ordering.API", "target": "WebApp", "type": "event" },
    { "source": "Ordering.API", "target": "Webhooks.API", "type": "event" },
    { "source": "Ordering.API", "target": "PaymentProcessor", "type": "event" },
    { "source": "Catalog.API", "target": "Ordering.API", "type": "event" },
    { "source": "Catalog.API", "target": "Webhooks.API", "type": "event" },
    { "source": "OrderProcessor", "target": "Ordering.API", "type": "event" },
    { "source": "PaymentProcessor", "target": "Ordering.API", "type": "event" },
    { "source": "Ordering.API", "target": "OrderProcessor", "type": "database" },
    { "source": "OrderProcessor", "target": "Ordering.API", "type": "database" }
```

Add an `edges` array to `train-ticket.json` (it has none yet):

```json
  "edges": [
    { "source": "ts-preserve-service", "target": "ts-notification-service", "type": "event" },
    { "source": "ts-preserve-other-service", "target": "ts-notification-service", "type": "event" },
    { "source": "ts-food-service", "target": "ts-delivery-service", "type": "event" },
    { "source": "ts-order-service", "target": "ts-price-service", "type": "database" },
    { "source": "ts-price-service", "target": "ts-order-service", "type": "database" },
    { "source": "ts-order-service", "target": "ts-route-service", "type": "database" },
    { "source": "ts-station-food-service", "target": "ts-order-service", "type": "database" },
    { "source": "ts-travel-service", "target": "ts-order-service", "type": "http" },
    { "source": "ts-gateway-service", "target": "ts-order-service", "type": "http" }
  ]
```

Add an `edges` array to `opentelemetry-demo.json`:

```json
  "edges": [
    { "source": "checkout", "target": "cart", "type": "grpc" },
    { "source": "frontend", "target": "checkout", "type": "grpc" },
    { "source": "recommendation", "target": "product-catalog", "type": "grpc" },
    { "source": "frontend-proxy", "target": "frontend", "type": "http" },
    { "source": "cart", "target": "valkey-cart", "type": "database" },
    { "source": "product-catalog", "target": "astronomy-db", "type": "database" }
  ]
```

Each of these was verified against the source in the corpus survey: robot-shop `payment/rabbitmq.py:8-32` and `dispatch/main.go:76-219`; eShop `src/*/*.csproj` ProjectReferences, `src/*/Extensions/Extensions.cs` `AddSubscription<...>` lines, `new *IntegrationEvent(` in `src/Ordering.API/Application`, `src/Catalog.API`, `src/OrderProcessor/Services/GracePeriodManagerService.cs:56`, `src/PaymentProcessor/.../OrderStatusChangedToStockConfirmedIntegrationEventHandler.cs:23-27`, and `orderingdb` in `src/Ordering.API/Extensions/Extensions.cs:17` and `src/OrderProcessor/Extensions/Extensions.cs:13`; train-ticket `*/mq/RabbitSend.java` and `RabbitReceive.java` with `config/Queues.java`, and `application.yml` datasource defaults `10.176.122.1` / `ts` in ts-order, ts-price, ts-route, ts-station-food; opentelemetry-demo `.env:160`, `compose.yaml:98,537`.

If an expected edge is missing, that is a bug to fix in the matcher, not an expectation to delete, unless reading the source shows the survey was wrong; say which in the commit body.

- [ ] **Step 4: Run the corpus test**

Run: `cargo test --test corpus -- --nocapture 2>&1 | grep -E 'missing edge|^\S+ +[0-9]+ code|test result'`
Expected: no `missing edge` lines, `test result: ok`, every repository under 500 ms.

- [ ] **Step 5: Commit**

```bash
git add test/expected/corpus crates/blastradius-core/src
git commit -m "test(corpus): expected event, database and import edges for four repositories" -m "robot-shop: payment publishes to the exchange dispatch consumes. eShop: eight ProjectReference imports, nine integration-event edges from the AddSubscription wiring, and the orderingdb database Ordering.API shares with OrderProcessor. train-ticket: the email and food_delivery queues through Queues.queueName, and four services whose datasource defaults name one MySQL schema. OpenTelemetry demo: cart to valkey-cart through the compose pass-through of VALKEY_ADDR, product-catalog to astronomy-db through DB_CONNECTION_STRING. <one line per fix made during the junk review>"
```

---

### Task 9: Documentation

**Files:**
- Modify: `README.md` (Status table, "How mapping works", "What it looks like")
- Modify: `docs/superpowers/specs/2026-09-06-rust-engine-stage2-design.md` ("Decisions made while building")

- [ ] **Step 1: README status and mapping section**

In the Status table, the Stage 2 row becomes:

```
| 2. Map | Static edges: HTTP calls, gRPC stubs, message topics and typed events, shared databases, cross-package imports; datastore and broker hosts found on the way | Built |
```

and the paragraph under "## Status" changes "Service discovery and the first static edges are built." to "Service discovery and static dependency mapping are built."

In "## How mapping works", after the paragraph beginning "Two matchers read those facts.", replace it with:

```
Five matchers read those facts. The HTTP matcher takes URLs, `host:port`
literals, templates around them, Feign clients, environment variables whose
name says they hold a host, and configuration settings whose key does
(`spring.data.mongodb.host: ts-order-mongo`). The gRPC matcher takes
generated client stubs and works out which service owns each proto service
from its server registration, or from its name when nothing registers it.
The event matcher takes publish, send, produce, subscribe, consume and
declare calls by library, listener annotations, typed event buses
(`new OrderStartedIntegrationEvent(...)`, `AddSubscription<X, H>`) and Go
struct literals; a topic written as a constant (`self.EXCHANGE`,
`Queues.queueName`) is read back from the literal the service assigns to it.
The database matcher keys every connection string, URL, rendered template
and host-plus-database setting by host and database name. The import
matcher reads imports in code and dependencies, Maven sibling artifacts,
Cargo path dependencies and `.csproj` project references in manifests.
```

Add rows to the resolver table:

```
| `spring.data.mongodb.host: ts-order-mongo` in a service's config | the setting's value | Static |
| `basic_publish(routing_key='orders')` and `Consume("orders")` in two services | the topic, producer → consumer | Inferred |
| `import pika` with a `rabbitmq` image declared | the client library's broker family | Inferred |
| `jdbc:mysql://mysql/shop` in two services | the shared `host/database` key, both directions | Inferred |
| `import '@acme/shared/utils'` with `packages/shared` declaring `@acme/shared` | the package name, or a prefix ending at a separator | Static |
| `<ProjectReference Include="..\EventBus\EventBus.csproj" />` | the service whose root holds the path | Static |
```

Replace the paragraph starting "An edge's type follows its target" with:

```
An edge's type follows its target: a variable pointing at Redis is a
`database` edge, one pointing at RabbitMQ is an `event` edge, and an HTTP
URL on a pair that also has a gRPC stub folds into the gRPC edge. Anything
that matches no discovered service is counted and listed under `mapping` in
the JSON and in the report, never guessed; a topic with a producer but no
consumer in the repository is listed as `topic:<name>`. An import that
matches no discovered package is an external library and is not counted.
```

- [ ] **Step 2: Refresh the sample output**

Run `cargo run -q --release -- analyze corpus/robot-shop --no-color` and replace the block under "## What it looks like" with the real output, keeping the EDGES table to its first ten rows followed by `  ...`, and keeping FINDINGS in full (it now ends with `Shared databases 0`). Update the sentence after the block if the counts it quotes changed ("the nginx template resolved ... so that edge is Uncertain").

- [ ] **Step 3: Spec decisions**

Append to "### Decisions made while building" in the spec:

```
- Settings are facts. `key: value`, `key=value`, `<key>value</key>` and
  Dockerfile `ENV` lines in configuration files, and `name = "literal"`
  assignments in every language, become `Setting` facts. A hostish key
  (`host`, `bootstrap-servers`, `DB_CONNECTION_STRING`) makes the value a
  Static host candidate; the rest feed a per-service symbol table.
- Topics are mostly constants. `self.EXCHANGE`, `Queues.queueName` and
  `TopicName` resolve through the symbol table of the service, never across
  services. A topic with a producer or consumer but no counterpart is
  listed as `topic:<key>`; a bare declaration (`queue_declare`) with no
  counterpart is dropped.
- Constructing a typed event is producing it. eShop publishes variables, so
  `new OrderStartedIntegrationEvent(...)` anywhere in a service makes it a
  producer of that event; `*DomainEvent` types stay in-process and are
  ignored, as is MediatR's `INotificationHandler`.
- A client library import is an Inferred edge to the broker of its family
  when the repository declares one (`import pika` → the `rabbitmq` image).
- A shared-database key needs a database name: `host/dbname` from a URL or
  connection string, a host setting paired with a database setting in the
  same file, or an Aspire resource name. A bare host is not a key: two
  services on one MySQL server with different schemas do not share data.
- Import edges never count as unresolved. Almost every import is an
  external library; listing them would bury the hostnames the reader needs.
- Compose `environment: - KEY` without a value takes the value from the
  `.env` beside the compose file, and dotenv values interpolate against
  earlier lines of the same file. Both came from the OpenTelemetry demo's
  `VALKEY_ADDR=valkey-cart:${VALKEY_PORT}`.
- <the junk-review fixes from Task 8, one line each>
```

- [ ] **Step 4: Final check and commit**

```bash
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test 2>&1 | grep -E '^test result|FAILED'
cargo run -q -- analyze . --no-color | head -40
git add README.md docs/superpowers/specs/2026-09-06-rust-engine-stage2-design.md
git commit -m "docs: mapping stage complete, event, database and import matchers, decisions made while building" -m "Stage 2 is built. The README describes the five matchers and the resolver rows they add, shows real robot-shop output including the shared databases finding, and the spec records the decisions the corpus forced."
```

---

## Self-review

**Spec coverage.** Matchers `event`, `database`, `import` (spec "Matchers"): Tasks 4, 5, 6. Resolver rows Topic → Inferred, Database shared → Inferred, Package → Static: Task 3. Broker edges for producers and consumers when the broker is discovered: Task 4 (library family). "Every service using a key gets an edge to the infrastructure service": already produced by the http matcher's typed-by-target rule, extended to settings in Task 2. Terminal report "shared databases" finding: Task 7. Fixtures `edges-events-app`, `edges-db-app`, `edges-import-app`: Tasks 4, 2, 6. Corpus expected edges: Task 8. The spec's `edges-mixed-app` fixture for the resolver confidence table is covered by resolver unit tests in Task 3 and the three fixtures together; not built separately. `mapping.parsers` and the JSON contract: unchanged, asserted by the existing `json_contract_gains_mapping_between_edges_and_runtime` test.

**Placeholders.** The only deliberately open text is `<one line per fix made during the junk review>` in Tasks 8 and 9, filled from the review the executor performs.

**Type consistency.** `TopicRole` lives in `map/mod.rs` (Task 3) and is used by `event.rs` (Task 4) and `resolve.rs`. `TopicSides` and `Joins` live in `resolve.rs` (Task 3), consumed by `event::topic_index` and `database::database_index` (Tasks 4, 5) and built in `map::run`. `Package` and `ConfigIndex.packages` are declared in Task 3 and filled in Task 6. `FileContext.symbols: &Symbols` is added in Task 4; Task 2's `http.rs` tests note the two lines to add then. `Resolved.source` is `Option<String>`; `collect_outcomes` reads it in Task 3. `owner_of_path` is defined in Task 3 and its `path == root` case added in Task 6. `is_hostish_key`, `is_hostname`, `ado_connection`, `parse_url`, `DATABASE_SCHEMES` are all `pub`/`pub(crate)` in `resolve.rs` and imported by name in `http.rs` and `database.rs`.
