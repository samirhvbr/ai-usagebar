"""Exercise release checks in temporary repositories, without user Git config."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


CHECKER = Path(__file__).with_name("check-changelog-immutable.sh")
UNSET = object()
CHANGELOG = """# Changelog

## [Unreleased]

## [1.0.0]

### Fixed
- Keep this published entry.

## [0.9.0]

- An older release.
"""
VERSION_FILES = {
    "Cargo.toml": 'version = "{}"\n',
    "manifest.json": '{{"version": "{}"}}\n',
    "packaging/aur/PKGBUILD": "pkgver={}\n",
    "packaging/aur/PKGBUILD-bin": "pkgver={}\n",
}


class ChangelogCheckTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        self.repo = root / "repository"
        self.repo.mkdir()
        home = root / "home"
        home.mkdir()
        self.env = {
            "PATH": os.environ["PATH"],
            "HOME": str(home),
            "XDG_CONFIG_HOME": str(home / ".config"),
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CONFIG_GLOBAL": os.devnull,
            "GIT_TERMINAL_PROMPT": "0",
            "LC_ALL": "C",
        }
        self.git("init", "--quiet", "--initial-branch=main")
        self.git("config", "user.name", "Changelog Test")
        self.git("config", "user.email", "changelog-test@example.invalid")
        self.git("config", "commit.gpgsign", "false")
        self.git("config", "tag.gpgsign", "false")
        (self.repo / "scripts").mkdir()
        shutil.copyfile(CHECKER, self.repo / "scripts" / CHECKER.name)
        (self.repo / "CHANGELOG.md").write_text(CHANGELOG)
        self.write_versions("1.0.0")
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", "Prepare release")
        self.before_release = self.git("rev-parse", "HEAD").stdout.strip()
        self.git("commit", "--quiet", "--allow-empty", "-m", "Release 1.0.0")
        self.git("tag", "-a", "v1.0.0", "-m", "Release 1.0.0")

    def git(self, *args):
        return subprocess.run(
            ["git", *args], cwd=self.repo, env=self.env,
            text=True, capture_output=True, check=True,
        )

    def write_versions(self, version):
        for name, template in VERSION_FILES.items():
            path = self.repo / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(template.format(version))

    def add_unmerged_release(self):
        self.git("checkout", "--quiet", "-b", "upstream")
        changelog = CHANGELOG.replace(
            "## [Unreleased]\n",
            "## [Unreleased]\n\n## [2.0.0]\n\n- A future upstream release.\n",
        )
        (self.repo / "CHANGELOG.md").write_text(changelog)
        self.write_versions("2.0.0")
        self.git("add", ".")
        self.git("commit", "--quiet", "-m", "Release 2.0.0")
        self.git("tag", "v2.0.0")
        self.git("checkout", "--quiet", "main")

    def check(self, ref=UNSET):
        env = self.env.copy()
        if ref is not UNSET:
            env["CHANGELOG_TAG_REF"] = ref
        return subprocess.run(
            ["bash", "scripts/check-changelog-immutable.sh"],
            cwd=self.repo, env=env, text=True, capture_output=True,
        )

    def test_scoped_versions_ignore_an_unmerged_newer_release(self):
        self.add_unmerged_release()
        result = self.check("HEAD")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn("v2.0.0", result.stdout)

    def test_unset_ref_still_compares_all_release_tags(self):
        self.add_unmerged_release()
        result = self.check()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Cargo.toml says 1.0.0, older than the released 2.0.0", result.stdout)

    def test_scoped_sections_ignore_an_unmerged_newer_release(self):
        self.add_unmerged_release()
        self.write_versions("2.0.0")
        (self.repo / "CHANGELOG.md").write_text(CHANGELOG.replace(
            "## [Unreleased]\n",
            "## [Unreleased]\n\n## [2.0.0]\n\n- A different local draft.\n",
        ))
        result = self.check("HEAD")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        unscoped = self.check()
        self.assertNotEqual(unscoped.returncode, 0)
        self.assertIn("the published [2.0.0] section no longer matches", unscoped.stdout)

    def test_scoped_check_rejects_a_tampered_released_section(self):
        (self.repo / "CHANGELOG.md").write_text(CHANGELOG.replace(
            "Keep this published entry.", "Rewrite this published entry.",
        ))
        result = self.check("HEAD")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("the published [1.0.0] section no longer matches", result.stdout)

    def test_scoped_check_rejects_each_version_rollback(self):
        for name, template in VERSION_FILES.items():
            with self.subTest(path=name):
                self.write_versions("1.0.0")
                (self.repo / name).write_text(template.format("0.9.0"))
                result = self.check("HEAD")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(f"{name} says 0.9.0, older than the released 1.0.0", result.stdout)

    def test_explicit_ref_requires_a_nonempty_commit_reference(self):
        for ref in ("", "missing-ref", "--all", "HEAD:CHANGELOG.md"):
            with self.subTest(ref=ref):
                result = self.check(ref)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("CHANGELOG_TAG_REF", result.stdout + result.stderr)

    def test_explicit_ref_requires_at_least_one_reachable_release_tag(self):
        result = self.check(self.before_release)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("no v* tags are reachable", result.stdout + result.stderr)

    def test_unset_ref_preserves_the_no_tags_behavior(self):
        self.git("tag", "-d", "v1.0.0")
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("no v* tags found", result.stdout)

    def test_scoped_check_allows_versions_above_the_latest_release(self):
        self.write_versions("1.1.0")
        result = self.check("v1.0.0")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
