//! Integration tests for shell completion generation.

use std::process::Command;

use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_cmx");

fn run(args: &[&str]) -> std::process::Output {
    let temp = TempDir::new().unwrap();
    Command::new(BIN)
        .args(args)
        .current_dir(temp.path())
        .env("HOME", temp.path().join("home"))
        .env("OPENAI_API_KEY", "")
        .output()
        .unwrap()
}

#[test]
fn zsh_completions_start_with_compdef() {
    let output = run(&["completions", "zsh"]);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.is_empty(), "expected completion script on stdout");
    assert!(stdout.starts_with("#compdef"), "{stdout}");
    assert!(output.stderr.is_empty(), "stderr should stay empty on success");
}

#[test]
fn bash_completions_register_cmx() {
    let output = run(&["completions", "bash"]);
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.is_empty(), "expected completion script on stdout");
    assert!(
        stdout.lines().any(|line| line.contains("complete ") && line.contains(" cmx"))
            || stdout.lines().any(|line| line.contains("compgen") && line.contains(" cmx")),
        "{stdout}"
    );
    assert!(output.stderr.is_empty(), "stderr should stay empty on success");
}

#[test]
fn invalid_shell_lists_possible_values() {
    let output = run(&["completions", "bogus"]);
    assert!(!output.status.success(), "invalid shell should fail");

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("possible values"), "{stderr}");
    assert!(stderr.contains("bash"), "{stderr}");
    assert!(stderr.contains("zsh"), "{stderr}");
    assert!(stderr.contains("fish"), "{stderr}");
    assert!(stderr.contains("elvish"), "{stderr}");
    assert!(stderr.contains("powershell"), "{stderr}");
}
