"""Hermetic bundle contracts; no compiler, signing key, or real app data is used."""

import importlib.util
from pathlib import Path
import plistlib
import tempfile
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location("widget_build", Path(__file__).with_name("build.py"))
builder = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(builder)


class WidgetBundleTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.host = self.root / "host"
        self.extension = self.root / "widget"
        self.host.write_bytes(b"synthetic host executable")
        self.extension.write_bytes(b"synthetic extension executable")
        self.resources = self.root / "resources"
        self.resources.mkdir()
        for name in ("AIUsage.icns", "menu-bar-icon.LICENSE", "LICENSE-APACHE-2.0"):
            (self.resources / name).write_text("synthetic resource")
        self.output = self.root / "AI UsageBar.app"

    def package(self):
        builder.package_bundle(self.host, self.extension, self.output, "0.6.0",
                               "TESTTEAM01", self.resources)

    def test_real_extension_layout_preserves_existing_preferences_domain(self):
        self.package()
        contents = self.output / "Contents"
        extension_contents = contents / "PlugIns/AIUsageBarWidget.appex/Contents"
        host_info = plistlib.loads((contents / "Info.plist").read_bytes())
        extension_info = plistlib.loads((extension_contents / "Info.plist").read_bytes())
        self.assertEqual(host_info["CFBundleIdentifier"], "ai-usagebar-menubar")
        self.assertEqual(host_info["CFBundleExecutable"], "ai-usagebar-menubar")
        self.assertTrue(host_info["LSUIElement"])
        self.assertEqual(extension_info["CFBundleIdentifier"], "ai-usagebar-menubar.widget")
        self.assertEqual(extension_info["NSExtension"], {
            "NSExtensionPointIdentifier": "com.apple.widgetkit-extension",
        })
        for info in (host_info, extension_info):
            self.assertEqual(info["AIUsageWidgetAppGroup"], "TESTTEAM01.ai-usagebar.widget")
            self.assertEqual(info["CFBundleVersion"], "0.6.0")
            self.assertEqual(info["LSMinimumSystemVersion"], "14.0")
        self.assertEqual((contents / "MacOS/ai-usagebar-menubar").read_bytes(), self.host.read_bytes())
        self.assertEqual((extension_contents / "MacOS/AIUsageBarWidget").read_bytes(), self.extension.read_bytes())
        self.assertEqual((contents / "MacOS/ai-usagebar-menubar").stat().st_mode & 0o777, 0o755)

    def test_existing_bundle_is_never_overwritten(self):
        self.output.mkdir()
        marker = self.output / "keep"
        marker.write_text("original app")
        with self.assertRaises(FileExistsError):
            self.package()
        self.assertEqual(marker.read_text(), "original app")

    def test_missing_binary_creates_no_partial_bundle(self):
        self.extension.unlink()
        with self.assertRaises(FileNotFoundError):
            self.package()
        self.assertFalse(self.output.exists())

    def test_entitlements_share_only_the_snapshot_group(self):
        host = builder.entitlements("TESTTEAM01", extension=False)
        widget = builder.entitlements("TESTTEAM01", extension=True)
        self.assertEqual(host, {
            "com.apple.security.application-groups": ["TESTTEAM01.ai-usagebar.widget"],
        })
        self.assertEqual(widget, {**host, "com.apple.security.app-sandbox": True})
        with self.assertRaises(ValueError):
            builder.app_group("group.example")

    def test_host_and_extension_have_separate_entry_points(self):
        host = builder.compile_command("/synthetic/sdk", "arm64", self.host, extension=False)
        widget = builder.compile_command("/synthetic/sdk", "arm64", self.extension, extension=True)
        self.assertEqual(host[:5], ["/usr/sbin/taskpolicy", "-b", "/usr/bin/nice", "-n", "19"])
        self.assertIn("WIDGET_HOST", host)
        self.assertNotIn("-application-extension", host)
        self.assertIn("WIDGET_EXTENSION", widget)
        self.assertIn("-application-extension", widget)
        self.assertIn("arm64-apple-macosx14.0", widget)
        self.assertFalse(any("Tests.swift" in argument for argument in host + widget))
        self.assertFalse(any("WidgetPublisher.swift" in argument for argument in widget))

    def test_mismatched_signing_team_is_rejected_before_release(self):
        metadata = type("Metadata", (), {"stderr": "TeamIdentifier=OTHERTEAM1\n"})()
        with patch.object(builder.subprocess, "run", return_value=metadata) as run:
            with self.assertRaisesRegex(ValueError, "TeamIdentifier"):
                builder.sign_bundle(self.output, "synthetic identity", "TESTTEAM01", self.root / "test.entitlements")
        self.assertEqual(run.call_count, 2)


if __name__ == "__main__":
    unittest.main()
