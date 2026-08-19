//! Launches the actual compiled `jass-rs` binary as a subprocess and checks
//! its behavior end-to-end (argument parsing, exit codes, stdout/stderr) —
//! as opposed to the other tests, which call the library functions
//! directly and never touch `main.rs`.

use std::path::Path;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_jass-rs");

fn sample_path() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/sample.j")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn lint_mode_runs_clean_file_and_exits_success() {
    let output = Command::new(BIN)
        .arg(sample_path())
        .output()
        .expect("failed to launch jass-rs binary");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
    assert!(stdout.contains("no issues found"), "stdout was: {stdout}");
}

#[test]
fn html_mode_runs_and_prints_a_self_contained_page() {
    let output = Command::new(BIN)
        .arg("--html")
        .arg(sample_path())
        .output()
        .expect("failed to launch jass-rs binary");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
    assert!(stdout.starts_with("<!doctype html>"));
    assert!(stdout.contains("id=\"ast-data\""));
    assert!(stdout.contains("id=\"L1\""));
    assert!(stdout.ends_with("</html>\n"));
}

#[test]
fn ast_and_dot_modes_still_run() {
    for flag in ["--ast", "--dot"] {
        let output = Command::new(BIN)
            .arg(flag)
            .arg(sample_path())
            .output()
            .unwrap_or_else(|e| panic!("failed to launch jass-rs binary with {flag}: {e}"));
        assert!(
            output.status.success(),
            "{flag} stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!output.stdout.is_empty(), "{flag} produced no output");
    }
}

#[test]
fn exits_with_failure_on_a_parse_error() {
    let dir = std::env::temp_dir().join(format!("jass-rs-cli-launch-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("failed to create temp dir");
    let broken = dir.join("broken.j");
    std::fs::write(&broken, "function Broken takes\nendfunction").expect("failed to write fixture");

    let output = Command::new(BIN)
        .arg(&broken)
        .output()
        .expect("failed to launch jass-rs binary");

    std::fs::remove_dir_all(&dir).ok();

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf-8");
    assert!(stderr.contains("error"), "stderr was: {stderr}");
}

#[test]
fn exits_with_failure_when_no_file_is_given() {
    let output = Command::new(BIN)
        .output()
        .expect("failed to launch jass-rs binary");
    assert!(!output.status.success());
}
