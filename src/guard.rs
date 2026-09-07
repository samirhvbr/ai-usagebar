//! Source-scanning helpers for structural guard tests.
//!
//! A guard that enumerates what to check fails open on everything added after
//! it. A guard that *walks* fails closed: a new file is scanned by default and
//! an exemption has to be written down. `claude_desktop`'s note-sanitization
//! guard is built on this.
//!
//! Written by Augusto Claro for the encrypted-sync bundle format (#123) and
//! kept when that module was reverted, because the walking-guard idea outlived
//! the feature it was written for. The `production_code` comment below records
//! a real defect it was hardened against; the file names in it refer to that
//! now-removed module and are left as the history of the fix.

use std::path::{Path, PathBuf};

/// Every `.rs` file under `dir`, recursively.
pub(crate) fn rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(dir, &mut out);
    out
}

/// Every `.rs` file under `CARGO_MANIFEST_DIR`-relative `rel`.
///
/// Resolved from the manifest directory rather than from a relative path, so
/// a guard is independent of the working directory and survives the AUR
/// `srcdir` layout.
pub(crate) fn rs_files_in(rel: &str) -> Vec<PathBuf> {
    rs_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join(rel))
}

/// A file's production code: every line that is neither a comment nor part
/// of the file's own `#[cfg(test)]` module. A test that names a needle is a
/// test, not a violation, and prose that discusses one is neither.
///
/// **Comments are removed before the marker is looked for, and that is the
/// fix.** The previous shape split the raw source on the first *textual*
/// `#[cfg(test)]`. In `github/pairing.rs` the first occurrence is inside a
/// doc comment at line 76, so the scanned region ended at line 75 and the
/// 397 lines below it — five production functions — were invisible to every
/// guard built on this helper. Phase 5's audit put
/// `std::env::var("SYNC_PASSWORD")` in that region and watched the T-5-66
/// guard pass.
///
/// `github/mod.rs`'s own guard recorded this exact defect and worked around
/// it for itself; the lesson reached one call site and not the shared helper
/// every other guard depends on. A *smarter* marker search — line-anchored,
/// or `\n#[cfg(test)]\nmod tests` — keeps the same shape: a guard that stops
/// looking where it happens to find a string. Dropping comments first makes
/// the marker unambiguous by construction, because prose is no longer part
/// of the text being searched.
///
/// Returns an owned `String` rather than a borrowed slice, since the result
/// is no longer a contiguous piece of the input.
pub(crate) fn production_code(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .take_while(|line| !line.trim_start().starts_with("#[cfg(test)]"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("a readable source directory") {
        let path = entry.expect("a readable directory entry").path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}
