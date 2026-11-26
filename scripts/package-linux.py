#!/usr/bin/env python3
"""Pack Harmony into an AppImage with its notices and corresponding source.

Use nix/appimage.nix's AppDir by default. For a build-stamped release,
supply the clean Cargo build, its compile stamp, and the pinned runtime."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import urllib.parse

from release_package import ROOT, dependency_notices, digest, names_home, source_snapshot

TARGET = "x86_64-unknown-linux-gnu"
FEATURES = ["--no-default-features", "--features", "desktop,discord"]
APP_ID = "io.github.TimeShallHalt.Harmony"


def nix_build(installable):
    return Path(subprocess.check_output(["nix", "build", "--no-link", "--print-out-paths", installable],
                                        cwd=ROOT, text=True, timeout=7200).strip())

def native_stamp(path, version, commit):
    stamp = json.loads(path.read_bytes())
    expected = dict(version=version, commit=commit, dirty=False, platform="linux",
                    architecture="x86_64", upstream="Yapfast 0.4.1", license="GPL-3.0-or-later")
    if (not isinstance(stamp, dict) or set(stamp) != set(expected) | {"build_id"}
            or any(stamp[key] != value for key, value in expected.items())
            or stamp["dirty"] is not False or type(stamp["build_id"]) is not int
            or not 0 < stamp["build_id"] < 2**64):
        raise ValueError("Expected a matching clean Linux x86_64 release build stamp")
    return stamp


def version_parts(version):
    return tuple(map(int, version.split(".")))


def native_appdir(appdir, source, runtime, stamp):
    policy = json.loads((ROOT / "packaging/linux/appimage-policy.json").read_bytes())
    if digest(runtime) != policy["runtime_sha256"]:
        raise ValueError("AppImage runtime differs from the pinned release")
    with source.open("rb") as stream:
        header = stream.read(64)
    if header[:6] != b"\x7fELF\x02\x01" or header[18:20] != b"\x3e\x00":
        raise ValueError("Expected an AMD64 ELF executable")
    executable = appdir / "usr/bin/harmony"
    executable.parent.mkdir(parents=True)
    shutil.copyfile(source, executable)
    executable.chmod(0o755)
    subprocess.run(["patchelf", "--set-interpreter", "/lib64/ld-linux-x86-64.so.2",
                    "--remove-rpath", executable], check=True)
    dynamic = subprocess.check_output(["readelf", "-Wd", executable], text=True)
    needed = set(re.findall(r"\(NEEDED\).*?\[([^\]]+)\]", dynamic))
    missing = needed.difference(policy["host_libraries"])
    if missing or "(RPATH)" in dynamic or "(RUNPATH)" in dynamic:
        raise ValueError(f"Unexpected dynamic dependencies or library paths: {sorted(missing)}")
    symbols = subprocess.check_output(["readelf", "-V", executable], text=True)
    existing = {}
    for prefix, label in (("GLIBC", "glibc"), ("GLIBCXX", "libstdcxx")):
        versions = re.findall(rf"Name: {prefix}_([0-9.]+)", symbols)
        highest = max(versions, key=version_parts) if versions else "0"
        if version_parts(highest) > version_parts(policy[label + "_floor"]):
            raise ValueError(f"Harmony needs {prefix}_{highest}, newer than the AppImage ABI floor")
        existing[label + "_required"] = highest
    for file, folder in (
        (ROOT / f"packaging/linux/{APP_ID}.desktop", "applications"),
        (ROOT / f"packaging/linux/{APP_ID}.metainfo.xml", "metainfo"),
        (ROOT / "packaging/icon.svg", "icons/hicolor/scalable/apps"),
    ):
        destination = appdir / "usr/share" / folder
        destination.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(file, destination / (f"{APP_ID}.svg" if file.suffix == ".svg" else file.name))
    (appdir / "AppRun").symlink_to("usr/bin/harmony")
    (appdir / f"{APP_ID}.desktop").symlink_to(f"usr/share/applications/{APP_ID}.desktop")
    (appdir / f"{APP_ID}.svg").symlink_to(f"usr/share/icons/hicolor/scalable/apps/{APP_ID}.svg")
    subprocess.run(["rsvg-convert", "-w", "256", "-h", "256", ROOT / "packaging/icon.svg",
                    "-o", appdir / ".DirIcon"], check=True)
    return {
        **stamp, **existing, "built_by": "cargo build --locked --release in the native Nix development shell",
        "glibc_floor": policy["glibc_floor"], "libstdcxx_floor": policy["libstdcxx_floor"],
        "appimage_runtime": policy["runtime_url"], "appimage_runtime_sha256": policy["runtime_sha256"],
        "update_information": "",
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "target/dist/linux")
    parser.add_argument("--exe", type=Path, help="Linux x86_64 Harmony executable from a clean release build")
    parser.add_argument("--stamp", type=Path, help="harmony-build-info.json emitted by that build")
    parser.add_argument("--runtime", type=Path, help="Pinned x86_64 AppImage runtime")
    args = parser.parse_args()
    supplied = (args.exe, args.stamp, args.runtime)
    if any(supplied) and not all(supplied):
        parser.error("--exe, --stamp and --runtime are required together")
    if args.exe and subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip():
        parser.error("Commit the exact clean candidate before packaging")
    parts = None if args.exe else nix_build(".#appdir")
    runtime = args.runtime if parts is None else parts / "runtime"
    skia_source = nix_build("path:./nix#skia-source")
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform", TARGET, *FEATURES],
        cwd=ROOT, timeout=30,
    ))
    version = next(package["version"] for package in metadata["packages"] if package["name"] == "yapfast")
    skia_url = urllib.parse.urlparse(os.environ["SKIA_BINARIES_URL"])
    if skia_url.scheme != "file":
        raise SystemExit("Packaging requires the pinned local Skia archive")
    skia_archive = Path(urllib.parse.unquote(skia_url.path))
    webrtc = Path(os.environ["LK_CUSTOM_WEBRTC"])
    output = args.output
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        raise SystemExit("Output must be empty; never overwrite a release")
    base = f"Harmony-{version}"
    image = output / f"{base}-x86_64.AppImage"
    source_path = output / f"{base}-source.zip"
    build = {}
    if parts is not None:
        build = json.loads((parts / "build.json").read_text())
        if build.get("update_information"):
            raise SystemExit("Harmony AppImages must not contain upstream update information")
    with tempfile.TemporaryDirectory(prefix="harmony-linux-package-") as temporary:
        temporary = Path(temporary)
        rtc = source_snapshot(metadata, base, temporary, source_path)
        if rtc:
            raise SystemExit("The AppImage is the Discord flavour, which has no AGPL dependencies")

        appdir = temporary / "AppDir"
        if parts is None:
            commit = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
            build = native_appdir(appdir, args.exe, runtime, native_stamp(args.stamp, version, commit))
        else:
            shutil.copytree(parts / "AppDir", appdir, symlinks=True)
        for directory in [appdir, *(path for path in appdir.rglob("*") if path.is_dir() and not path.is_symlink())]:
            directory.chmod(0o755)
        executable = appdir / "usr/bin/harmony"
        contents = executable.read_bytes()
        if names_home(contents):
            raise SystemExit("The executable contains local home paths. Use the release path-remapping flags.")
        del contents

        doc = appdir / "usr/share/doc/harmony"
        doc.mkdir(parents=True)
        shutil.copy2(ROOT / "LICENSE", doc / "LICENSE")
        (doc / "corresponding-source").mkdir()
        shutil.copy2(source_path, doc / "corresponding-source" / source_path.name)
        (doc / "SOURCE.txt").write_text(
            f"The source code of this build of Harmony {version} accompanies it:\n\n"
            f"  corresponding-source/{source_path.name}, SHA-256 in BUILD.json\n\n"
            "Based on Yapfast 0.4.1, https://github.com/yapfast/yapfast. The licenses folder\n"
            "holds every dependency's own license and notices.\n",
            encoding="utf-8",
        )
        dependencies = dependency_notices(metadata, doc, doc / "licenses", skia_archive, skia_source, webrtc)
        (doc / "THIRD-PARTY.json").write_text(json.dumps(dependencies, indent=2) + "\n", encoding="utf-8")
        lock = json.loads((ROOT / "flake.lock").read_text())["nodes"]["nixpkgs"]["locked"]
        build = {
            "version": version, "flavour": "discord", "target": TARGET,
            "cargo_features": " ".join(FEATURES),
            "built_by": "nix build .#appdir",
            "nixpkgs_revision": lock["rev"],
            "ui_framework": "Slint 1.19.0 before release, ac7d423f",
            "skia_version": "0.157.0",
            "skia_archive_prepared_sha256": digest(skia_archive),
            "cargo_lock_sha256": digest(ROOT / "Cargo.lock"),
            "executable_sha256": digest(executable),
            "source_archive_sha256": digest(source_path),
            **build,
            "application": "Harmony", "upstream": "Yapfast 0.4.1",
            "release_checks": True, "update_installation": "signed AppImage replacement in a writable user folder",
            "updater_included": True,
        }
        (doc / "BUILD.json").write_text(json.dumps(build, indent=2) + "\n", encoding="utf-8")

        # Fixed owners and times, so the same AppDir packs to the same file.
        # mksquashfs takes the times from SOURCE_DATE_EPOCH, which the
        # development shell sets, or its options, not both.
        filesystem = temporary / "AppDir.squashfs"
        environment = {name: value for name, value in os.environ.items() if name != "SOURCE_DATE_EPOCH"}
        subprocess.run(["mksquashfs", appdir, filesystem, "-noappend", "-quiet", "-no-progress", "-processors", "4",
                        "-root-owned", "-no-xattrs", "-mkfs-time", "0", "-all-time", "0",
                        "-comp", "zstd", "-Xcompression-level", "19", "-b", "1M"],
                       env=environment, check=True, timeout=1800)
        with image.open("wb") as stream:
            for part in (runtime, filesystem):
                with part.open("rb") as piece:
                    shutil.copyfileobj(piece, stream)
        image.chmod(0o755)

    offset = subprocess.check_output([image, "--appimage-offset"], text=True, timeout=30).strip()
    if int(offset) != runtime.stat().st_size:
        raise SystemExit(f"{image} finds its filesystem at {offset}, not after the runtime")
    update = subprocess.check_output([image, "--appimage-updateinformation"], text=True, timeout=30).strip()
    if update:
        raise SystemExit(f"Harmony AppImage unexpectedly contains update information {update!r}")
    hashes = output / "SHA256SUMS.txt"
    artifacts = [image, source_path]
    hashes.write_text("".join(f"{digest(path)}  {path.name}\n" for path in artifacts), encoding="utf-8")
    print(f"Packaged {image} ({image.stat().st_size / 1024 / 1024:.1f} MiB)")

    print(f"Source snapshot: {source_path}")
    print(f"Checksums: {hashes}")


if __name__ == "__main__":
    main()
