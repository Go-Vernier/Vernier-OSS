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
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("vernier 0.0.1"));
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
