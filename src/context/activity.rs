//! Live activity of Claude Code sessions: which ones are working, and which are
//! waiting on the user.
//!
//! Claude Code keeps one `<CLAUDE_CONFIG_DIR>/sessions/<pid>.json` per running
//! interactive session and rewrites it whenever that session's status changes:
//! `busy` while it works, `waiting` while a permission prompt or a question is
//! open, `idle` otherwise. Like the transcripts next to it this is an
//! undocumented surface, so every field is optional data: an unreadable,
//! oversized or malformed file is skipped, and an unknown status counts as
//! neither working nor waiting.
//!
//! The file outlives a crashed or killed Claude Code, so a session only counts
//! while its process is still running. On Linux that is checked against the
//! start time Claude Code records (`procStart`, field 22 of `/proc/<pid>/stat`),
//! which also rejects a pid the kernel has since handed to another process;
//! elsewhere a running process with that pid is the best available signal. A
//! file written on another operating system (`pidDomain`), as a config
//! directory shared across a dual boot or a migration carries, never counts:
//! its pid means nothing here.
//!
//! Read-only: these files belong to Claude Code, so a stale one is skipped,
//! never removed.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use serde_json::Value;

/// Directory entries visited, not sessions: Claude Code keeps a `.key` file
/// next to each session's `.json`. This bounds the walk if something else ever
/// fills the directory.
const MAX_WALK_ENTRIES: usize = 4_096;
/// Session files actually opened per scan. The Waybar tooltip scans on every
/// tick, so a directory full of crash leftovers must stay cheap: at most this
/// many reads of at most [`MAX_SESSION_BYTES`] each.
const MAX_SESSION_READS: usize = 256;
/// A session file is a few hundred bytes. Anything far larger is not one.
const MAX_SESSION_BYTES: u64 = 8 * 1024;

/// How many of an account's live interactive sessions are working, and how
/// many are waiting on the user.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionActivity {
    pub working: usize,
    pub waiting: usize,
}

impl SessionActivity {
    pub fn is_idle(&self) -> bool {
        self.working == 0 && self.waiting == 0
    }

    /// `"2 working · 1 waiting"`, naming only the counts that are non-zero;
    /// `None` when nothing is happening.
    pub fn summary(&self) -> Option<String> {
        let parts: Vec<String> = [(self.working, "working"), (self.waiting, "waiting")]
            .into_iter()
            .filter(|(count, _)| *count > 0)
            .map(|(count, state)| format!("{count} {state}"))
            .collect();
        (!parts.is_empty()).then(|| parts.join(" · "))
    }
}

/// Whether a session's process is still the one that wrote its file. The seam
/// tests replace: they must never probe the real process table.
pub trait ProcessProbe {
    /// `start_time` is the session file's `procStart`, when it carries one.
    fn is_running(&self, pid: u32, start_time: Option<&str>) -> bool;
}

/// The production probe for the platform this was built for.
pub struct SystemProbe;

impl ProcessProbe for SystemProbe {
    fn is_running(&self, pid: u32, start_time: Option<&str>) -> bool {
        process_running(pid, start_time)
    }
}

#[cfg(target_os = "linux")]
fn process_running(pid: u32, start_time: Option<&str>) -> bool {
    let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    stat_says_running(&stat, start_time)
}

/// A `/proc/<pid>/stat` line describes the session's process: not a zombie
/// (`Z`) or dead (`X`) entry awaiting reaping, and, when the session recorded
/// one, started at the same moment.
#[cfg(any(target_os = "linux", test))]
fn stat_says_running(stat: &str, start_time: Option<&str>) -> bool {
    let Some((state, started)) = stat_fields(stat) else {
        return false;
    };
    !matches!(state, "Z" | "X") && start_time.is_none_or(|expected| expected == started)
}

#[cfg(all(unix, not(target_os = "linux")))]
fn process_running(pid: u32, _start_time: Option<&str>) -> bool {
    use rustix::io::Errno;
    use rustix::process::{Pid, test_kill_process};

    let Some(pid) = i32::try_from(pid).ok().and_then(Pid::from_raw) else {
        return false;
    };
    // `kill(pid, 0)`: EPERM still means the process exists, owned by someone else.
    matches!(test_kill_process(pid), Ok(()) | Err(Errno::PERM))
}

#[cfg(windows)]
fn process_running(pid: u32, _start_time: Option<&str>) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY: plain Win32 calls on a handle this function opens and closes
    // itself; `code` outlives the call that writes it.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code = 0u32;
        let queried = GetExitCodeProcess(handle, &mut code) != 0;
        CloseHandle(handle);
        queried && code == STILL_ACTIVE as u32
    }
}

#[cfg(not(any(unix, windows)))]
fn process_running(_pid: u32, _start_time: Option<&str>) -> bool {
    false
}

/// Fields 3 (`state`) and 22 (`starttime`) of `/proc/<pid>/stat`. The command
/// name in field 2 is parenthesised and may itself contain spaces or `)`, so
/// fields are counted from the last `)`, where field 3 starts.
#[cfg(any(target_os = "linux", test))]
fn stat_fields(stat: &str) -> Option<(&str, &str)> {
    let (_, rest) = stat.rsplit_once(')')?;
    let mut fields = rest.split_whitespace();
    let state = fields.next()?;
    Some((state, fields.nth(18)?))
}

/// Count the live interactive sessions under `config_dir/sessions` by status.
/// A missing or unreadable directory is simply no activity.
pub fn scan_dir(config_dir: &Path, probe: &impl ProcessProbe) -> SessionActivity {
    let mut activity = SessionActivity::default();
    let Ok(entries) = fs::read_dir(config_dir.join("sessions")) else {
        return activity;
    };
    let mut reads = 0;
    for entry in entries.take(MAX_WALK_ENTRIES).flatten() {
        let Some(pid) = file_pid(&entry.file_name().to_string_lossy()) else {
            continue;
        };
        // `DirEntry::file_type` does not follow symlinks: a link is not a file here.
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        if reads == MAX_SESSION_READS {
            break;
        }
        reads += 1;
        let Some(session) = read_session(&entry.path()) else {
            continue;
        };
        // A file renamed or copied under another pid's name is not that
        // session, and a pid from another operating system is not a process here.
        if session.pid != pid || !session.interactive || !session.local {
            continue;
        }
        let counter = match session.status.as_deref() {
            Some("busy") => &mut activity.working,
            Some("waiting") => &mut activity.waiting,
            _ => continue,
        };
        if probe.is_running(pid, session.start_time.as_deref()) {
            *counter += 1;
        }
    }
    activity
}

struct SessionFile {
    pid: u32,
    interactive: bool,
    local: bool,
    status: Option<String>,
    start_time: Option<String>,
}

/// The platform prefix Claude Code writes at the start of `pidDomain`
/// (Node's `process.platform`): `linux:<machine-id>:pid:[<ns>]`, `win32:<user>`.
fn current_platform() -> &'static str {
    match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        other => other,
    }
}

/// `<pid>.json`, with nothing but digits before the extension.
fn file_pid(name: &str) -> Option<u32> {
    let stem = name.strip_suffix(".json")?;
    if stem.is_empty() || !stem.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    stem.parse().ok()
}

fn read_session(path: &Path) -> Option<SessionFile> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(MAX_SESSION_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_SESSION_BYTES {
        return None;
    }
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let text = |key: &str| value.get(key).and_then(Value::as_str);
    Some(SessionFile {
        pid: u32::try_from(value.get("pid")?.as_u64()?).ok()?,
        // Agent SDK runs (`entrypoint: "sdk-cli"`, which `claude -p` and SDK
        // hosts such as background agents use) are not someone's session at a
        // terminal or in an editor, even though they also write
        // `kind: "interactive"`. A file without either field predates them.
        interactive: text("kind").is_none_or(|kind| kind == "interactive")
            && text("entrypoint").is_none_or(|entrypoint| !entrypoint.starts_with("sdk")),
        local: text("pidDomain")
            .is_none_or(|domain| domain.split(':').next() == Some(current_platform())),
        status: text("status").map(str::to_owned),
        start_time: text("procStart").map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;
    use tempfile::TempDir;

    use super::*;

    /// Running pids and the start time each one was started at.
    #[derive(Default)]
    struct FakeProbe {
        running: HashMap<u32, Option<String>>,
    }

    impl FakeProbe {
        fn with(mut self, pid: u32, start_time: Option<&str>) -> Self {
            self.running.insert(pid, start_time.map(str::to_owned));
            self
        }
    }

    impl ProcessProbe for FakeProbe {
        fn is_running(&self, pid: u32, start_time: Option<&str>) -> bool {
            match (self.running.get(&pid), start_time) {
                (None, _) => false,
                (Some(Some(actual)), Some(expected)) => actual == expected,
                (Some(_), _) => true,
            }
        }
    }

    fn config_dir() -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("sessions")).unwrap();
        dir
    }

    fn write(dir: &TempDir, name: &str, body: &str) {
        fs::write(dir.path().join("sessions").join(name), body).unwrap();
    }

    /// The shape Claude Code 2.1 writes for a terminal session on this
    /// platform, trimmed to the fields that matter here.
    fn session_body(pid: u32, status: &str) -> Value {
        json!({
            "pid": pid,
            "sessionId": format!("session-{pid}"),
            "cwd": "/work/project",
            "kind": "interactive",
            "entrypoint": "cli",
            "pidDomain": format!("{}:machine:pid:[4026531836]", current_platform()),
            "name": "project",
            "procStart": format!("{pid}00"),
            "status": status,
            "statusUpdatedAt": 1_791_169_032_529_u64,
        })
    }

    fn session(dir: &TempDir, pid: u32, status: &str) {
        write(
            dir,
            &format!("{pid}.json"),
            &session_body(pid, status).to_string(),
        );
    }

    fn session_with(dir: &TempDir, pid: u32, status: &str, key: &str, value: Value) {
        let mut body = session_body(pid, status);
        body[key] = value;
        write(dir, &format!("{pid}.json"), &body.to_string());
    }

    /// Each pid running since the start time `session` records for it.
    fn running(pids: &[u32]) -> FakeProbe {
        pids.iter().fold(FakeProbe::default(), |probe, &pid| {
            probe.with(pid, Some(&format!("{pid}00")))
        })
    }

    #[test]
    fn live_sessions_are_counted_by_status() {
        let dir = config_dir();
        session(&dir, 101, "busy");
        session(&dir, 102, "busy");
        session(&dir, 103, "waiting");
        session(&dir, 104, "idle");

        let activity = scan_dir(dir.path(), &running(&[101, 102, 103, 104]));

        assert_eq!(
            activity,
            SessionActivity {
                working: 2,
                waiting: 1
            }
        );
    }

    #[test]
    fn idle_shell_and_unknown_statuses_count_as_nothing() {
        let dir = config_dir();
        session(&dir, 101, "idle");
        session(&dir, 102, "shell");
        session(&dir, 103, "something-new");
        write(
            &dir,
            "104.json",
            &json!({"pid": 104, "kind": "interactive"}).to_string(),
        );

        let activity = scan_dir(dir.path(), &running(&[101, 102, 103, 104]));

        assert!(activity.is_idle());
    }

    #[test]
    fn a_session_whose_process_is_gone_does_not_count() {
        let dir = config_dir();
        session(&dir, 101, "busy");
        session(&dir, 102, "waiting");

        let activity = scan_dir(dir.path(), &running(&[101]));

        assert_eq!(
            activity,
            SessionActivity {
                working: 1,
                waiting: 0
            }
        );
    }

    #[test]
    fn a_recycled_pid_with_another_start_time_does_not_count() {
        let dir = config_dir();
        session(&dir, 101, "busy");

        let probe = FakeProbe::default().with(101, Some("999999"));

        assert!(scan_dir(dir.path(), &probe).is_idle());
    }

    #[test]
    fn agent_sdk_runs_do_not_count_but_editor_sessions_do() {
        let dir = config_dir();
        // Agent SDK hosts write `kind: "interactive"` too; only the entrypoint
        // tells them apart from a person's session.
        session_with(&dir, 101, "busy", "entrypoint", json!("sdk-cli"));
        session_with(&dir, 102, "busy", "kind", json!("print"));
        session_with(&dir, 103, "waiting", "entrypoint", json!("claude-vscode"));
        // A file from before `kind` and `entrypoint` existed was a session.
        let legacy = json!({"pid": 104, "status": "busy"});
        write(&dir, "104.json", &legacy.to_string());

        let probe = FakeProbe::default()
            .with(101, None)
            .with(102, None)
            .with(103, None)
            .with(104, None);

        assert_eq!(
            scan_dir(dir.path(), &probe),
            SessionActivity {
                working: 1,
                waiting: 1
            }
        );
    }

    #[test]
    fn a_session_written_on_another_operating_system_does_not_count() {
        let dir = config_dir();
        // A config directory shared across a dual boot or carried over by a
        // migration: that pid belongs to a process table that is not this one.
        session_with(&dir, 101, "busy", "pidDomain", json!("plan9:someone"));

        assert!(scan_dir(dir.path(), &running(&[101])).is_idle());
    }

    #[test]
    fn a_file_named_for_another_pid_is_ignored() {
        let dir = config_dir();
        let body = json!({"pid": 101, "kind": "interactive", "status": "busy"});
        write(&dir, "202.json", &body.to_string());

        let probe = FakeProbe::default().with(101, None).with(202, None);

        assert!(scan_dir(dir.path(), &probe).is_idle());
    }

    #[test]
    fn malformed_oversized_and_foreign_files_are_skipped_without_hiding_others() {
        let dir = config_dir();
        session(&dir, 101, "busy");
        write(&dir, "102.json", "{ not json");
        write(
            &dir,
            "103.json",
            &" ".repeat(MAX_SESSION_BYTES as usize + 1),
        );
        write(&dir, "104.json", r#"{"pid": "104", "status": "busy"}"#);
        write(
            &dir,
            "notes.json",
            &json!({"pid": 105, "status": "busy"}).to_string(),
        );
        write(
            &dir,
            "106.json.tmp",
            &json!({"pid": 106, "status": "busy"}).to_string(),
        );
        fs::create_dir(dir.path().join("sessions").join("107.json")).unwrap();

        let probe = running(&[101, 102, 103, 104, 105, 106, 107]);

        assert_eq!(
            scan_dir(dir.path(), &probe),
            SessionActivity {
                working: 1,
                waiting: 0
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_session_file_is_not_followed() {
        let dir = config_dir();
        let outside = dir.path().join("elsewhere.json");
        let body = json!({"pid": 101, "kind": "interactive", "status": "busy"});
        fs::write(&outside, body.to_string()).unwrap();
        std::os::unix::fs::symlink(&outside, dir.path().join("sessions").join("101.json")).unwrap();

        assert!(scan_dir(dir.path(), &FakeProbe::default().with(101, None)).is_idle());
    }

    /// The tooltip scans on every tick: a directory holding far more session
    /// files than any machine runs costs a bounded number of reads.
    #[test]
    fn a_crowded_directory_is_read_only_up_to_the_cap() {
        let dir = config_dir();
        let pids: Vec<u32> = (1000..1000 + MAX_SESSION_READS as u32 + 20).collect();
        for &pid in &pids {
            session(&dir, pid, "busy");
        }

        let activity = scan_dir(dir.path(), &running(&pids));

        assert_eq!(activity.working, MAX_SESSION_READS);
    }

    #[test]
    fn a_missing_sessions_directory_is_no_activity() {
        let dir = TempDir::new().unwrap();

        assert!(scan_dir(dir.path(), &FakeProbe::default()).is_idle());
    }

    #[test]
    fn summary_names_only_the_counts_that_are_not_zero() {
        let summary = |working, waiting| SessionActivity { working, waiting }.summary();

        assert_eq!(summary(2, 1).as_deref(), Some("2 working · 1 waiting"));
        assert_eq!(summary(1, 0).as_deref(), Some("1 working"));
        assert_eq!(summary(0, 3).as_deref(), Some("3 waiting"));
        assert_eq!(summary(0, 0), None);
    }

    #[test]
    fn stat_fields_are_counted_from_the_last_parenthesis() {
        // A command name may contain spaces and `)`; fields 3 and 22 are still found.
        let stat = "4242 (my (odd) cmd) S 1 4242 4242 0 -1 4194560 100 0 0 0 \
                    1 2 0 0 20 0 1 0 7322237 1000000 200";

        assert_eq!(stat_fields(stat), Some(("S", "7322237")));
        assert_eq!(stat_fields("4242 (truncated"), None);
        assert_eq!(stat_fields("4242 (short) S 1"), None);
    }

    #[test]
    fn a_zombie_or_a_different_start_time_is_not_the_session() {
        let stat = |state: &str| {
            format!(
                "4242 (claude) {state} 1 4242 4242 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 7322237 1000000 200"
            )
        };

        assert!(stat_says_running(&stat("S"), Some("7322237")));
        assert!(stat_says_running(&stat("R"), None));
        assert!(!stat_says_running(&stat("S"), Some("7322238")));
        assert!(!stat_says_running(&stat("Z"), Some("7322237")));
        assert!(!stat_says_running(&stat("X"), None));
        assert!(!stat_says_running("garbage", None));
    }
}
