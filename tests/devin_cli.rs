use std::process::Command;

use tempfile::TempDir;

const CANARY: &str = "DevinSecretCanary-593827";

/// Runs the real binary, the only test in the repo that does. It stays
/// hermetic because `--config` and `--cache-dir` are injected and the run
/// fails at credential parsing, before `theme_from_cli` would read the user's
/// real Omarchy theme. Do not extend it to a rendered (non-`⚠`) output
/// without also injecting the theme.
#[test]
fn cli_json_output_is_one_valid_json_line_and_never_exposes_the_cli_key() {
    let dir = TempDir::new().unwrap();
    let credentials_path = dir.path().join("devin-credentials.toml");
    std::fs::write(
        &credentials_path,
        format!("windsurf_api_key = \"{CANARY}\"\napi_server_url = \"https://canary.invalid\"\n"),
    )
    .unwrap();
    let config_path = dir.path().join("config.toml");
    let credential_path = serde_json::to_string(&credentials_path.to_string_lossy()).unwrap();
    std::fs::write(
        &config_path,
        format!("[devin]\nenabled = true\ncredentials_path = {credential_path}\n"),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_ai-usagebar"))
        .args(["--vendor", "devin", "--json", "--config"])
        .arg(&config_path)
        .args(["--cache-dir"])
        .arg(dir.path().join("cache"))
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with('\n'));
    assert_eq!(stdout.bytes().filter(|byte| *byte == b'\n').count(), 1);
    let json: serde_json::Value = serde_json::from_str(stdout.trim_end()).unwrap();
    assert_eq!(json["text"], "⚠");
    assert!(
        json["tooltip"]
            .as_str()
            .unwrap()
            .contains("unsupported API destination")
    );

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stdout.contains(CANARY));
    assert!(!stderr.contains(CANARY));
}
