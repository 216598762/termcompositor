//! CLI integration tests for the `termcompositor` binary.
//!
//! These tests exercise the command-line interface by spawning
//! the binary with various flags and verifying its exit code,
//! stdout content, and stderr output.

use std::process::Command;

/// Helper: run the binary with the given args and return
/// (exit_code, stdout, stderr).
fn run_binary(args: &[&str]) -> (i32, Vec<u8>, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_termcompositor"))
        .args(args)
        .output()
        .expect("failed to execute termcompositor binary");

    let exit_code = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (exit_code, output.stdout, stderr)
}

// ── Basic invocation ──────────────────────────────────────────

#[test]
fn binary_runs_without_args() {
    let (exit_code, _stdout, stderr) = run_binary(&[]);
    assert_eq!(exit_code, 0, "binary should exit 0 with no args");
    assert!(
        stderr.contains("termcompositor"),
        "stderr should mention the binary name"
    );
}

#[test]
fn binary_has_help_output() {
    let (exit_code, _stdout, stderr) = run_binary(&["--help"]);
    // The binary doesn't implement --help via clap yet; it will
    // fall through to normal execution. Just verify it exits 0.
    // TODO: update when clap is added to check for usage information.
    assert_eq!(exit_code, 0, "binary should exit 0 with --help");
    assert!(
        stderr.contains("termcompositor"),
        "stderr should mention the binary name"
    );
}

#[test]
fn binary_outputs_version_info_in_stderr() {
    let (_, _, stderr) = run_binary(&[]);
    assert!(
        stderr.contains("v0.11.0") || stderr.contains("termcompositor"),
        "stderr should contain version info: {stderr}"
    );
}

#[test]
fn binary_reports_terminal_size() {
    let (_, _, stderr) = run_binary(&[]);
    assert!(
        stderr.contains("cols") && stderr.contains("rows"),
        "stderr should report terminal dimensions: {stderr}"
    );
}

#[test]
fn binary_reports_protocol_resolution() {
    let (_, _, stderr) = run_binary(&[]);
    assert!(
        stderr.contains("resolved"),
        "stderr should report resolved protocol: {stderr}"
    );
}

#[test]
fn binary_reports_layers_and_pixels() {
    let (_, _, stderr) = run_binary(&[]);
    assert!(
        stderr.contains("layer") || stderr.contains("rendered"),
        "stderr should mention layer count or rendering: {stderr}"
    );
}

// ── --protocol flag ──────────────────────────────────────────

#[test]
fn protocol_kitty_exits_cleanly() {
    let (exit_code, stdout, stderr) = run_binary(&["--protocol", "kitty"]);
    assert_eq!(exit_code, 0, "binary should exit 0 with --protocol kitty");
    // Kitty output starts with ESC_G
    assert!(
        stdout.starts_with(b"\x1b_G") || stdout.is_empty(),
        "Kitty stdout should start with ESC_G or be empty if no encoder feature: got {:?}",
        &stdout[..20.min(stdout.len())]
    );
    assert!(
        stderr.contains("kitty"),
        "stderr should mention kitty protocol: {stderr}"
    );
}

#[test]
fn protocol_sixel_exits_cleanly() {
    let (exit_code, _stdout, stderr) = run_binary(&["--protocol", "sixel"]);
    assert_eq!(exit_code, 0, "binary should exit 0 with --protocol sixel");
    assert!(
        stderr.contains("sixel"),
        "stderr should mention sixel protocol: {stderr}"
    );
}

#[test]
fn protocol_auto_exits_cleanly() {
    let (exit_code, _, stderr) = run_binary(&["--protocol", "auto"]);
    assert_eq!(exit_code, 0, "binary should exit 0 with --protocol auto");
    assert!(
        stderr.contains("auto"),
        "stderr should mention auto protocol: {stderr}"
    );
}

#[test]
fn protocol_unknown_warns_and_falls_back() {
    let (exit_code, _, stderr) = run_binary(&["--protocol", "nonexistent"]);
    assert_eq!(
        exit_code, 0,
        "binary should exit 0 even with unknown protocol"
    );
    assert!(
        stderr.contains("warning") || stderr.contains("unknown") || stderr.contains("falling back"),
        "stderr should warn about unknown protocol: {stderr}"
    );
}

#[test]
fn protocol_flag_without_value_warns() {
    let (exit_code, _, stderr) = run_binary(&["--protocol"]);
    assert_eq!(exit_code, 0, "binary should exit 0 even with missing value");
    assert!(
        stderr.contains("warning: --protocol missing value"),
        "stderr should contain exact warning about missing --protocol value: {stderr}"
    );
}

// ── --probe flag ─────────────────────────────────────────────

#[test]
fn probe_flag_exits_cleanly() {
    let (exit_code, _, stderr) = run_binary(&["--probe"]);
    assert_eq!(exit_code, 0, "binary should exit 0 with --probe");
    assert!(
        stderr.contains("probe") || stderr.contains("resolved"),
        "stderr should mention probe or protocol resolution: {stderr}"
    );
}

#[test]
fn probe_with_explicit_protocol_ignores_probe() {
    let (exit_code, _, stderr) = run_binary(&["--protocol", "sixel", "--probe"]);
    assert_eq!(
        exit_code, 0,
        "binary should exit 0 with --probe + --protocol sixel"
    );
    // When --protocol is explicit (not auto), --probe is ignored.
    assert!(
        stderr.contains("sixel"),
        "stderr should mention sixel when --protocol is explicit: {stderr}"
    );
    // The resolution should show sixel, not probe-based detection.
    assert!(
        stderr.contains("resolved: sixel"),
        "stderr should show resolved protocol as sixel: {stderr}"
    );
}

// ── --tmux-passthrough flag ──────────────────────────────────

#[test]
fn tmux_passthrough_flag_exits_cleanly() {
    let (exit_code, _, stderr) = run_binary(&["--tmux-passthrough"]);
    assert_eq!(exit_code, 0, "binary should exit 0 with --tmux-passthrough");
    assert!(
        stderr.contains("passthrough") || stderr.contains("tmux"),
        "stderr should mention tmux passthrough: {stderr}"
    );
}

#[test]
fn tmux_passthrough_with_kitty_protocol() {
    let (exit_code, _, stderr) = run_binary(&["--tmux-passthrough", "--protocol", "kitty"]);
    assert_eq!(
        exit_code, 0,
        "binary should exit 0 with --tmux-passthrough + --protocol kitty"
    );
    assert!(
        stderr.contains("passthrough") || stderr.contains("enabled"),
        "stderr should report passthrough status: {stderr}"
    );
}

// ── Combined flags ───────────────────────────────────────────

#[test]
fn combined_flags_exits_cleanly() {
    let (exit_code, _, _) = run_binary(&["--protocol", "auto", "--probe", "--tmux-passthrough"]);
    assert_eq!(exit_code, 0, "binary should exit 0 with all flags combined");
}

// ── Stdout content ───────────────────────────────────────────

#[cfg(feature = "kitty-encoder")]
#[test]
fn kitty_protocol_outputs_escape_sequences() {
    let (_, stdout, _) = run_binary(&["--protocol", "kitty"]);
    assert!(
        !stdout.is_empty(),
        "Kitty output must not be empty when feature is enabled"
    );
    assert!(
        stdout.starts_with(b"\x1b_G"),
        "Kitty output must start with ESC_G"
    );
    assert!(
        stdout.ends_with(b"\x1b\\"),
        "Kitty output must end with ESC_backslash"
    );
}

#[cfg(feature = "sixel-encoder")]
#[test]
fn sixel_protocol_outputs_data() {
    let (_, stdout, _) = run_binary(&["--protocol", "sixel"]);
    assert!(
        !stdout.is_empty(),
        "Sixel output must not be empty when feature is enabled"
    );
    assert!(
        stdout.len() > 10,
        "Sixel output should be substantial (got {} bytes)",
        stdout.len()
    );
}

// ── Error handling ───────────────────────────────────────────

#[test]
fn binary_exits_zero_regardless_of_features() {
    // The binary should ALWAYS exit 0, even when no encoder
    // features are enabled. It prints an error to stderr but
    // never panics or exits non-zero.
    let (exit_code, _, stderr) = run_binary(&[]);
    assert_eq!(exit_code, 0, "binary should never panic or exit non-zero");
    assert!(
        stderr.contains("encoded")
            || stderr.contains("error")
            || stderr.contains("not supported")
            || stderr.contains("rendered"),
        "stderr should indicate encoding result: {stderr}"
    );
}
