//! Open `ai-usagebar-tui`, or a provider's login command, in Terminal.app.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

pub fn open() {
    run_in_terminal(&quote(&sibling("ai-usagebar-tui")));
}

/// Run one shell line in a new Terminal window, which stays open afterwards.
pub fn run_in_terminal(line: &str) {
    let Ok(tmp) = write_terminal_script(line) else {
        return;
    };
    let command = format!("/bin/bash {}; true", quote(&tmp.to_string_lossy()));
    let script = format!(
        "tell application \"Terminal\" to do script \"{}\"\ntell application \"Terminal\" to activate",
        apple_script_string(&command)
    );
    if Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .spawn()
        .is_err()
        && let Some(dir) = tmp.parent()
    {
        let _ = fs::remove_dir_all(dir);
    }
}

/// Create the launcher below an atomically-created mode-0700 directory. A
/// predictable file directly in `/tmp` can be pre-planted as a symlink.
fn write_terminal_script(line: &str) -> std::io::Result<PathBuf> {
    let dir = tempfile::Builder::new().prefix("ai-usagebar-").tempdir()?;
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700))?;
    let path = dir.path().join("launch.sh");
    let cleanup = quote(&dir.path().to_string_lossy());
    fs::write(
        &path,
        format!("#!/bin/bash\n{line}\necho\nread -p 'Enter to close...'\nrm -rf -- {cleanup}\n"),
    )?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    let _ = dir.keep();
    Ok(path)
}

fn apple_script_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// `path` as one single-quoted bash word, whatever characters it holds.
pub fn quote(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\\''"))
}

/// This project's binary `name`: beside the tray, then `~/.cargo/bin`, then `PATH`.
pub fn sibling(name: &str) -> String {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let sibling = dir.join(name);
        if sibling.is_file() {
            return sibling.to_string_lossy().into_owned();
        }
    }
    if let Ok(home) = crate::cache::home_dir() {
        let cargo = home.join(".cargo").join("bin").join(name);
        if cargo.is_file() {
            return cargo.to_string_lossy().into_owned();
        }
    }
    name.into()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn quote_keeps_shell_metacharacters_literal() {
        assert_eq!(super::quote("/A b/x"), "'/A b/x'");
        assert_eq!(super::quote("/it's $(x)"), "'/it'\\''s $(x)'");
    }

    #[test]
    fn terminal_scripts_live_in_distinct_private_directories() {
        let first = super::write_terminal_script("echo first").unwrap();
        let second = super::write_terminal_script("echo second").unwrap();
        assert_ne!(first.parent(), second.parent());
        assert_eq!(
            std::fs::metadata(first.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&first).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let body = std::fs::read_to_string(&first).unwrap();
        assert!(body.contains("echo first"));
        assert!(body.contains("rm -rf -- '"));
        std::fs::remove_dir_all(first.parent().unwrap()).unwrap();
        std::fs::remove_dir_all(second.parent().unwrap()).unwrap();
    }

    #[test]
    fn apple_script_strings_escape_code_delimiters() {
        assert_eq!(
            super::apple_script_string(r#"bash '/tmp/a'; echo "x\\y""#),
            r#"bash '/tmp/a'; echo \"x\\\\y\""#
        );
    }
}
