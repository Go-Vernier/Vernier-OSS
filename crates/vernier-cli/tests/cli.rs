use std::path::PathBuf;
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_vernier"))
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../test/fixtures")
        .join(name)
}

#[test]
fn analyze_json_prints_the_contract() {
    let out = bin()
        .args([
            "analyze",
            fixture("compose-app").to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["discovery"]["strategy"], "docker-compose");
    assert_eq!(json["services"].as_array().unwrap().len(), 3);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.starts_with("{\n  \"repository\""),
        "pretty printed with two spaces"
    );
}

#[test]
fn analyze_report_defaults_to_the_current_directory_and_has_no_colour_when_piped() {
    let out = bin()
        .arg("analyze")
        .current_dir(fixture("monorepo-app"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("3 detected  (monorepo)"), "{text}");
    assert!(!text.contains('\u{1b}'));
}

#[test]
fn errors_go_to_stderr_with_exit_code_1() {
    let out = bin()
        .args([
            "analyze",
            fixture("compose-app")
                .join("docker-compose.yml")
                .to_str()
                .unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.starts_with("vernier: not a directory"), "{err}");
    assert!(out.stdout.is_empty());
}

#[test]
fn version_flag() {
    let out = bin().arg("--version").output().unwrap();
    let expected = concat!("vernier ", env!("CARGO_PKG_VERSION"));
    assert!(String::from_utf8_lossy(&out.stdout).starts_with(expected));
}

fn runtime_fixture(file: &str) -> String {
    fixture("runtime-app")
        .join("runtime")
        .join(file)
        .to_str()
        .unwrap()
        .to_string()
}

#[test]
fn otel_flag_joins_a_servicegraph_scrape() {
    let out = bin()
        .args([
            "analyze",
            fixture("runtime-app").to_str().unwrap(),
            "--otel",
            &runtime_fixture("traces.prom"),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["runtime"]["connected"], true);
    assert_eq!(json["runtime"]["source"], "otel");
    assert_eq!(json["runtime"]["services"]["matched"], 8);
    let report = bin()
        .args([
            "analyze",
            fixture("runtime-app").to_str().unwrap(),
            "--otel",
            &runtime_fixture("traces.prom"),
            "--full",
        ])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&report.stdout);
    assert!(
        text.contains("RUNTIME") && text.contains("8 of 10 runtime services matched"),
        "{text}"
    );
}

#[test]
fn datadog_flag_reads_a_saved_response() {
    let out = bin()
        .args([
            "analyze",
            fixture("runtime-app").to_str().unwrap(),
            "--datadog",
            &runtime_fixture("datadog.json"),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["runtime"]["source"], "datadog");
    assert_eq!(json["runtime"]["edges"]["runtimeOnly"], 1);
}

#[test]
fn runtime_errors_exit_1_and_print_no_report() {
    let missing = bin()
        .args([
            "analyze",
            fixture("runtime-app").to_str().unwrap(),
            "--otel",
            "/nonexistent/traces.prom",
        ])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("/nonexistent/traces.prom"));
    assert!(missing.stdout.is_empty());

    let both = bin()
        .args([
            "analyze",
            fixture("runtime-app").to_str().unwrap(),
            "--otel",
            "a",
            "--datadog",
            "b",
        ])
        .output()
        .unwrap();
    assert_eq!(both.status.code(), Some(2), "clap rejects the conflict");

    let no_env = bin()
        .args([
            "analyze",
            fixture("runtime-app").to_str().unwrap(),
            "--datadog",
        ])
        .output()
        .unwrap();
    assert_eq!(no_env.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&no_env.stderr).contains("--dd-env"),
        "{}",
        String::from_utf8_lossy(&no_env.stderr)
    );

    let no_keys = bin()
        .args([
            "analyze",
            fixture("runtime-app").to_str().unwrap(),
            "--datadog",
            "--dd-env",
            "prod",
        ])
        .env_remove("DD_API_KEY")
        .env_remove("DD_APP_KEY")
        .output()
        .unwrap();
    assert_eq!(no_keys.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&no_keys.stderr).contains("DD_API_KEY"),
        "{}",
        String::from_utf8_lossy(&no_keys.stderr)
    );
}

// ---------------------------------------------------------------- stage 4

fn temp_dir(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("vernier-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn git(root: &std::path::Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "user.name=vernier",
            "-c",
            "user.email=vernier@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// `edges-http-app` with two commits: everything, then a squash-merged PR #8
/// touching catalogue.
fn repo(tag: &str) -> PathBuf {
    let root = temp_dir(tag);
    copy_dir(&fixture("edges-http-app"), &root);
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "initial"]);
    std::fs::write(root.join("catalogue/handlers.go"), "package main\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "catalogue: handler (#8)"]);
    root
}

#[test]
fn files_flag_prints_the_change_report_and_json() {
    let out = bin()
        .args([
            "analyze",
            fixture("edges-http-app").to_str().unwrap(),
            "--files",
            "catalogue/main.go",
            "README.md",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("BLAST RADIUS"), "{text}");
    assert!(
        text.contains("1 service changed -> 4 services in the blast radius"),
        "{text}"
    );
    assert!(text.contains("Not in the computed blast radius"), "{text}");

    let out = bin()
        .args([
            "analyze",
            fixture("edges-http-app").to_str().unwrap(),
            "--files",
            "catalogue/main.go",
            "--depth",
            "1",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["blast"]["depth"], 1);
    assert_eq!(json["blast"]["summary"]["reached"], 3);
    assert_eq!(json["blast"]["notReached"], serde_json::json!(["payment"]));
}

#[test]
fn pr_diff_and_history_flags_read_the_local_repository() {
    let root = repo("pr");
    let out = bin()
        .args(["analyze", root.to_str().unwrap(), "--pr", "8", "--json"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["blast"]["change"]["kind"], "pr");
    assert_eq!(json["blast"]["change"]["reference"], "#8");
    assert_eq!(
        json["blast"]["change"]["files"],
        serde_json::json!(["catalogue/handlers.go"])
    );
    assert_eq!(json["blast"]["summary"]["reached"], 4);

    let out = bin()
        .args([
            "analyze",
            root.to_str().unwrap(),
            "--diff",
            "HEAD~1",
            "--history",
            "5",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Change        diff HEAD~1"), "{text}");
    assert!(
        text.contains("CHANGE HISTORY  (last 1 PR, 5 asked for)"),
        "{text}"
    );

    let missing = bin()
        .args(["analyze", root.to_str().unwrap(), "--pr", "404"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
    let err = String::from_utf8_lossy(&missing.stderr);
    assert!(
        err.contains("pull request #404 is not in the local history"),
        "{err}"
    );
    assert!(err.contains("git fetch origin pull/404/head"), "{err}");
    assert!(missing.stdout.is_empty());

    let both = bin()
        .args([
            "analyze",
            root.to_str().unwrap(),
            "--pr",
            "8",
            "--files",
            "a",
        ])
        .output()
        .unwrap();
    assert_eq!(both.status.code(), Some(2), "clap rejects the conflict");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn html_flag_writes_a_self_contained_file_and_still_prints_the_report() {
    let dir = temp_dir("html");
    let file = dir.join("report.html");
    let out = bin()
        .args([
            "analyze",
            fixture("edges-http-app").to_str().unwrap(),
            "--files",
            "catalogue/main.go",
            "--html",
            file.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("BLAST RADIUS"));
    assert!(String::from_utf8_lossy(&out.stderr).contains("wrote"));
    let html = std::fs::read_to_string(&file).unwrap();
    assert!(html.contains(r#"<script id="vernier-data""#));
    assert!(html.contains("\"blast\":{"));
    assert!(!html.contains("src=\"http"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn tui_refuses_to_start_without_a_terminal() {
    let out = bin()
        .args(["tui", fixture("edges-http-app").to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "vernier: tui needs a terminal; use vernier analyze for a report\n"
    );
}

#[test]
fn tui_shares_the_change_flags_with_analyze() {
    let out = bin()
        .args(["tui", ".", "--pr", "1", "--diff", "HEAD~1"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot be used with"));
}

#[test]
fn bare_vernier_walks_your_changes_or_prints_a_short_summary() {
    let root = repo("bare");
    let run = |args: &[&str]| {
        let out = bin().args(args).current_dir(&root).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    let clean = run(&[]);
    assert!(clean.contains("NO CHANGES"), "{clean}");
    assert!(clean.contains("vernier --full"), "{clean}");
    assert!(!clean.contains("EDGES"), "{clean}");

    std::fs::write(root.join("catalogue/main.go"), "package main // edited\n").unwrap();
    let dirty = run(&[]);
    assert!(
        dirty.contains("Change        your changes on main  uncommitted edits"),
        "{dirty}"
    );
    assert!(
        dirty.contains("1 service changed -> 4 services in the blast radius"),
        "{dirty}"
    );
    assert_eq!(run(&["analyze"]), dirty, "analyze is the same command");

    let full = run(&["--full"]);
    assert!(
        full.contains("SERVICES") && full.contains("EDGES"),
        "{full}"
    );
    assert!(!full.contains("BLAST RADIUS"), "{full}");

    let json: serde_json::Value = serde_json::from_str(&run(&["--json"])).unwrap();
    assert_eq!(json["blast"]["change"]["kind"], "working");
    assert_eq!(
        json["blast"]["change"]["files"],
        serde_json::json!(["catalogue/main.go"])
    );

    let both = bin()
        .args(["--full", "--files", "a"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert_eq!(both.status.code(), Some(2), "--full names no change");
    std::fs::remove_dir_all(&root).unwrap();
}

/// A one-request HTTP server on a free port. Returns its base URL and a
/// handle yielding the raw request it received.
fn fake_llm(status: u16, body: &'static str) -> (String, std::thread::JoinHandle<String>) {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut head = String::new();
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = v.trim().parse().unwrap();
            }
            head.push_str(&line);
            if line == "\r\n" {
                break;
            }
        }
        let mut payload = vec![0; length];
        reader.read_exact(&mut payload).unwrap();
        write!(
            stream,
            "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        head + &String::from_utf8(payload).unwrap()
    });
    (url, handle)
}

fn explain_cmd(url: &str, provider: &str) -> Command {
    let mut cmd = bin();
    cmd.args([
        "analyze",
        fixture("edges-http-app").to_str().unwrap(),
        "--files",
        "catalogue/main.go",
        "--explain",
    ])
    .env_remove("ANTHROPIC_API_KEY")
    .env_remove("OPENAI_API_KEY")
    .env_remove("GEMINI_API_KEY")
    .env_remove("VERNIER_LLM_MODEL")
    .env("VERNIER_LLM", provider)
    .env("VERNIER_LLM_URL", url);
    cmd
}

#[test]
fn explain_sends_the_blast_radius_to_an_openai_compatible_server() {
    let (url, server) = fake_llm(
        200,
        r#"{"choices":[{"message":{"role":"assistant","content":"- catalogue changed\n- web, ratings and payment call it"}}]}"#,
    );
    let out = explain_cmd(&url, "openai-compatible")
        .env("VERNIER_LLM_MODEL", "local-model")
        .env("VERNIER_LLM_API_KEY", "secret-k")
        .output()
        .unwrap();
    let request = server.join().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        request.starts_with("POST /v1/chat/completions "),
        "{request}"
    );
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer secret-k"),
        "{request}"
    );
    assert!(request.contains("\"model\":\"local-model\""), "{request}");
    assert!(request.contains("catalogue/main.go"), "{request}");
    assert!(
        !request.contains("http://catalogue:8080"),
        "no evidence snippets leave the machine: {request}"
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("BLAST RADIUS"), "{text}");
    assert!(
        text.contains("EXPLANATION  written by openai-compatible local-model"),
        "{text}"
    );
    assert!(
        text.contains("  - web, ratings and payment call it"),
        "{text}"
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("no source code"),
        "the user is told what is sent"
    );
}

#[test]
fn explain_uses_the_messages_api_for_anthropic_and_reports_http_errors() {
    let (url, server) = fake_llm(
        200,
        r#"{"content":[{"type":"text","text":"Catalogue changed."}],"stop_reason":"end_turn"}"#,
    );
    let out = explain_cmd(&url, "anthropic")
        .env("ANTHROPIC_API_KEY", "sk-test")
        .arg("--json")
        .output()
        .unwrap();
    let request = server.join().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(request.starts_with("POST /v1/messages "), "{request}");
    assert!(request.contains("x-api-key: sk-test"), "{request}");
    assert!(
        request.contains("\"model\":\"claude-opus-5-5\""),
        "{request}"
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["explanation"]["provider"], "anthropic");
    assert_eq!(json["explanation"]["text"], "Catalogue changed.");

    let (url, server) = fake_llm(401, r#"{"error":{"message":"invalid x-api-key"}}"#);
    let out = explain_cmd(&url, "anthropic")
        .env("ANTHROPIC_API_KEY", "sk-wrong")
        .output()
        .unwrap();
    server.join().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("anthropic (claude-opus-5-5): HTTP 401: invalid x-api-key"),
        "{err}"
    );
    assert!(out.stdout.is_empty());

    let missing = explain_cmd("http://127.0.0.1:9/v1", "anthropic")
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("needs ANTHROPIC_API_KEY"),
        "{}",
        String::from_utf8_lossy(&missing.stderr)
    );
}
