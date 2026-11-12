//! Open `ai-usagebar-tui`, or a provider's login command, in a real console.
//! Both occupy a terminal; spawning them without one just flashes nothing.

use std::process::Command;

pub fn open() {
    spawn_console(&[&sibling("ai-usagebar-tui")]);
}

/// Run one command line in a new console that stays open afterwards. The line
/// goes through `raw_arg`: Rust's argv escaping turns its quotes into `\"`,
/// which `cmd /k` does not understand.
pub fn run_in_terminal(line: &str) {
    use std::os::windows::process::CommandExt;
    if Command::new("wt.exe")
        .args(["new-tab", "--", "cmd.exe", "/k"])
        .raw_arg(line)
        .spawn()
        .is_ok()
    {
        return;
    }
    let _ = Command::new("conhost.exe")
        .args(["cmd.exe", "/k"])
        .raw_arg(line)
        .spawn();
}

fn spawn_console(args: &[&str]) {
    // `wt -e` is not a real Windows Terminal flag (it belongs to wezterm);
    // wt rejects it, prints its Usage page, and exits — the TUI never starts.
    // The correct syntax is `wt new-tab -- <command>`.
    if Command::new("wt.exe")
        .args(["new-tab", "--"])
        .args(args)
        .spawn()
        .is_ok()
    {
        return;
    }
    let _ = Command::new("conhost.exe").args(args).spawn();
}

/// `path` as one `cmd` argument. Windows paths cannot contain `"`.
pub fn quote(path: &str) -> String {
    format!("\"{path}\"")
}

/// This project's binary `name`: beside the tray, then `~/.cargo/bin`, then `PATH`.
pub fn sibling(name: &str) -> String {
    let exe = format!("{name}.exe");
    if let Ok(current) = std::env::current_exe()
        && let Some(dir) = current.parent()
    {
        let sibling = dir.join(&exe);
        if sibling.is_file() {
            return sibling.to_string_lossy().into_owned();
        }
    }
    if let Ok(home) = crate::cache::home_dir() {
        let cargo = home.join(".cargo").join("bin").join(&exe);
        if cargo.is_file() {
            return cargo.to_string_lossy().into_owned();
        }
    }
    exe
}
