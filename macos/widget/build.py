#!/usr/bin/env python3
"""Build the legacy menu bar host and a sandboxed WidgetKit app extension."""

import argparse
from pathlib import Path
import platform
import plistlib
import re
import shutil
import subprocess
import tempfile


WIDGET_DIR = Path(__file__).resolve().parent
MACOS_DIR = WIDGET_DIR.parent
REPO_DIR = MACOS_DIR.parent
HOST_ID = "ai-usagebar-menubar"
EXTENSION_ID = HOST_ID + ".widget"
HOST_EXECUTABLE = "ai-usagebar-menubar"
EXTENSION_EXECUTABLE = "AIUsageBarWidget"
EXTENSION_NAME = EXTENSION_EXECUTABLE + ".appex"
APP_GROUP_INFO_KEY = "AIUsageWidgetAppGroup"
MINIMUM_MACOS = "14.0"


def app_group(team_id: str) -> str:
    if not re.fullmatch(r"[A-Z0-9]{10}", team_id):
        raise ValueError("--team-id must be the signing certificate's 10-character Team ID")
    # macOS authorizes this format from the signing team without a profile.
    return team_id + ".ai-usagebar.widget"


def validate_version(version: str) -> str:
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise ValueError("--version must contain three numeric components, such as 0.6.0")
    return version


def fork_version() -> str:
    match = re.search(r"[0-9]+\.[0-9]+\.[0-9]+", (REPO_DIR / "version.md").read_text())
    if not match:
        raise ValueError("version.md does not contain the fork version")
    return validate_version(match[0])


def write_plist(path: Path, value: dict) -> None:
    with path.open("wb") as handle:
        plistlib.dump(value, handle, sort_keys=True)


def package_bundle(host: Path, extension: Path, output: Path, version: str,
                   team_id: str, resources: Path = MACOS_DIR) -> None:
    """Package already-built binaries; callers sign the resulting nested bundles."""
    group = app_group(team_id)
    validate_version(version)
    if output.suffix != ".app":
        raise ValueError("--output must end in .app")
    if output.exists():
        raise FileExistsError(f"refusing to overwrite {output}")
    resource_names = ("AIUsage.icns", "menu-bar-icon.LICENSE", "LICENSE-APACHE-2.0")
    for source in (host, extension, *(resources / name for name in resource_names)):
        if not source.is_file():
            raise FileNotFoundError(source)

    contents = output / "Contents"
    executables = contents / "MacOS"
    assets = contents / "Resources"
    extension_contents = contents / "PlugIns" / EXTENSION_NAME / "Contents"
    extension_executables = extension_contents / "MacOS"
    for directory in (executables, assets, extension_executables):
        directory.mkdir(parents=True, exist_ok=True)
    for source, destination in (
        (host, executables / HOST_EXECUTABLE),
        (extension, extension_executables / EXTENSION_EXECUTABLE),
    ):
        shutil.copy2(source, destination)
        destination.chmod(0o755)
    for name in resource_names:
        shutil.copy2(resources / name, assets / name)

    common = {
        "CFBundleInfoDictionaryVersion": "6.0",
        "CFBundleShortVersionString": version,
        "CFBundleVersion": version,
        "LSMinimumSystemVersion": MINIMUM_MACOS,
        APP_GROUP_INFO_KEY: group,
    }
    write_plist(contents / "Info.plist", {
        **common,
        # Match the standalone executable's existing UserDefaults domain.
        "CFBundleIdentifier": HOST_ID,
        "CFBundleName": "AI Usage Bar",
        "CFBundleDisplayName": "AI Usage Bar",
        "CFBundleExecutable": HOST_EXECUTABLE,
        "CFBundlePackageType": "APPL",
        "CFBundleIconFile": "AIUsage",
        "LSUIElement": True,
        "NSHighResolutionCapable": True,
        "NSPrincipalClass": "NSApplication",
    })
    write_plist(extension_contents / "Info.plist", {
        **common,
        "CFBundleIdentifier": EXTENSION_ID,
        "CFBundleName": "AI Usage Bar Widget",
        "CFBundleDisplayName": "AI Usage Bar",
        "CFBundleExecutable": EXTENSION_EXECUTABLE,
        "CFBundlePackageType": "XPC!",
        "NSExtension": {"NSExtensionPointIdentifier": "com.apple.widgetkit-extension"},
    })


def entitlements(team_id: str, *, extension: bool) -> dict:
    result = {"com.apple.security.application-groups": [app_group(team_id)]}
    if extension:
        result["com.apple.security.app-sandbox"] = True
    # The existing host stays nonsandboxed: it invokes the configured CLI.
    # The widget reads only the group snapshot and receives no network access.
    return result


def compile_command(sdk: str, arch: str, output: Path, *, extension: bool) -> list[str]:
    sources = [WIDGET_DIR / "WidgetSnapshot.swift"]
    if extension:
        sources += [WIDGET_DIR / "UsageWidgetView.swift", WIDGET_DIR / "UsageWidget.swift"]
    else:
        sources += [WIDGET_DIR / "WidgetPublisher.swift", MACOS_DIR / "ai-usagebar-menubar.swift"]
    command = [
        "/usr/sbin/taskpolicy", "-b", "/usr/bin/nice", "-n", "19",
        "/usr/bin/xcrun", "swiftc", "-O", "-parse-as-library",
        "-sdk", sdk, "-target", f"{arch}-apple-macosx{MINIMUM_MACOS}",
        "-module-name", "AIUsageBarWidget" if extension else "AIUsageBarHost",
        "-D", "WIDGET_EXTENSION" if extension else "WIDGET_HOST",
    ]
    if extension:
        command.append("-application-extension")
    command += [str(source) for source in sources]
    command += ["-o", str(output)]
    return command


def sign_bundle(bundle: Path, identity: str, team_id: str, entitlement_path: Path) -> None:
    subprocess.run([
        "/usr/bin/codesign", "--force", "--sign", identity, "--options", "runtime",
        "--timestamp=none", "--entitlements", str(entitlement_path), str(bundle),
    ], check=True)
    metadata = subprocess.run([
        "/usr/bin/codesign", "--display", "--verbose=4", str(bundle),
    ], check=True, capture_output=True, text=True)
    if f"TeamIdentifier={team_id}" not in metadata.stderr.splitlines():
        raise ValueError("the signing identity's TeamIdentifier does not match --team-id")
    subprocess.run([
        "/usr/bin/codesign", "--verify", "--strict", str(bundle),
    ], check=True)


def build(args: argparse.Namespace) -> Path:
    if platform.system() != "Darwin":
        raise RuntimeError("building a WidgetKit extension requires macOS and Xcode")
    if args.sign_identity.strip() in ("", "-"):
        raise ValueError("use a Developer ID or Apple Development identity, not ad-hoc signing")
    app_group(args.team_id)
    version = validate_version(args.version or fork_version())
    output = args.output.expanduser().absolute()
    if output.suffix != ".app":
        raise ValueError("--output must end in .app")
    if output.exists():
        raise FileExistsError(f"refusing to overwrite {output}; choose a fresh staging path")
    if args.host_binary is not None and not args.host_binary.is_file():
        raise FileNotFoundError(args.host_binary)
    sdk = subprocess.check_output([
        "/usr/bin/xcrun", "--sdk", "macosx", "--show-sdk-path",
    ], text=True).strip()
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".widget-build-", dir=output.parent) as temporary:
        temporary = Path(temporary)
        host = args.host_binary or temporary / HOST_EXECUTABLE
        if args.host_binary is None:
            print("Building the menu bar host at background priority...", flush=True)
            subprocess.run(compile_command(sdk, args.arch, host, extension=False), check=True)
        extension = temporary / EXTENSION_EXECUTABLE
        print("Building the WidgetKit extension at background priority...", flush=True)
        subprocess.run(compile_command(sdk, args.arch, extension, extension=True), check=True)
        staged = temporary / output.name
        package_bundle(host, extension, staged, version, args.team_id)
        for is_extension in (True, False):
            target = staged / "Contents" / "PlugIns" / EXTENSION_NAME if is_extension else staged
            entitlement_path = temporary / ("extension.entitlements" if is_extension else "host.entitlements")
            write_plist(entitlement_path, entitlements(args.team_id, extension=is_extension))
            sign_bundle(target, args.sign_identity, args.team_id, entitlement_path)
        subprocess.run([
            "/usr/bin/codesign", "--verify", "--deep", "--strict", str(staged),
        ], check=True)
        if output.exists():
            raise FileExistsError(f"refusing to overwrite {output}")
        staged.rename(output)
    return output


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path,
                        default=REPO_DIR / "target/macos-widget/AI UsageBar.app")
    parser.add_argument("--sign-identity", required=True,
                        help="existing Developer ID Application or Apple Development identity")
    parser.add_argument("--team-id", required=True, help="the identity's Team ID")
    parser.add_argument("--version", help="bundle version; defaults to the fork's version.md")
    parser.add_argument("--arch", choices=("arm64", "x86_64"), default=platform.machine())
    parser.add_argument("--host-binary", type=Path,
                        help="reuse a host already built with WIDGET_HOST and WidgetPublisher")
    args = parser.parse_args()
    try:
        output = build(args)
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Widget build failed: {error}\n")
    print(f"Built and signature-verified: {output}")
    print("Not installed, launched, registered, or notarized.")


if __name__ == "__main__":
    main()
