"""Run production profile-media modules/Slint with cached deps, no OS input.

No Cargo or root-target writes. The testing backend cannot take desktop focus.
"""
from pathlib import Path
import argparse
import json
import os
import re
import subprocess
import sys


def exact_library(cache, recorded, name):
    expected = next(dep[3] for dep in recorded["deps"] if dep[1] == name)
    matching = []
    for digest in (cache / ".fingerprint").glob(f"*/lib-{name}"):
        if int.from_bytes(bytes.fromhex(digest.read_text().strip()), "little") == expected:
            library = cache / "deps" / f"lib{name}-{digest.parent.name.rsplit('-', 1)[1]}.rlib"
            if library.exists():
                matching.append(library)
    if len(matching) != 1:
        raise SystemExit(f"Expected one exact cached fingerprint for {name}, got {matching}")
    return matching[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--deps-root", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--model", action="store_true")
    parser.add_argument("--core", action="store_true", help="exercise real Application dispatch with cached dependencies; no Cargo")
    parser.add_argument("--viewer-regression", action="store_true", help="run existing message ImageViewer native tests")
    parser.add_argument("--viewer-cache", action="store_true", help="run the production single-picture cache tests")
    parser.add_argument("--fidelity-regression", action="store_true", help="link pixel/URL regressions to the current production core")
    parser.add_argument("--regression-source", type=Path, help="link a generated-fixture regression to the current production core")
    args, tests = parser.parse_known_args()
    if sys.platform != "win32":
        parser.error("cached-dependency runner is Windows-only; run shared Rust tests on Linux")
    low_priority = subprocess.BELOW_NORMAL_PRIORITY_CLASS
    regression = args.fidelity_regression or args.regression_source is not None
    production_core = args.core or regression
    root = Path(__file__).resolve().parents[1]
    scratch = args.output_dir.resolve()
    scratch.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ)
    if not env.get("VCINSTALLDIR"):
        vswhere = Path(os.environ.get("ProgramFiles(x86)", "C:/Program Files (x86)")) / "Microsoft Visual Studio/Installer/vswhere.exe"
        vs = subprocess.check_output([str(vswhere), "-latest", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"], text=True).strip()
        vcvars = Path(vs) / "VC/Auxiliary/Build/vcvars64.bat"
        result = subprocess.run(f'call "{vcvars}" >nul && set', shell=True, capture_output=True, text=True, encoding="mbcs", errors="replace", check=True)
        for line in result.stdout.splitlines():
            if "=" in line and not line.startswith("="):
                key, value = line.split("=", 1)
                env[key] = value
    for key in ["TEMP", "TMP", "TMPDIR"]:
        env[key] = str(scratch)
    env["CARGO_MANIFEST_DIR"] = str(root)
    source = scratch / ("model.rs" if args.model else "native.rs")
    if args.model:
        source.write_text('#![allow(dead_code)]\npub use yapfast_core::chat;\n' + ''.join(f'#[path = "{(root / path).as_posix()}"] mod {name};\n' for name, path in [("original", "crates/yapfast-core/src/original.rs"), ("profile_media", "crates/yapfast-core/src/profile_media.rs"), ("profile_media_io", "crates/yapfast-core/src/profile_media_io.rs")]), encoding="utf-8")
        names = ["yapfast_core", "tokio", "image", "anyhow", "yapfast_discord"]
    else:
        test_file = "src/ui/viewer_picture.rs" if args.viewer_cache else "src/ui/media_viewer_tests.rs" if args.viewer_regression else "src/ui/profile_media_tests.rs"
        source.write_text(f'#[path = "{(root / test_file).as_posix()}"] mod profile_media_tests;\n', encoding="utf-8")
        names = ["slint", "i_slint_backend_testing", "i_slint_core"]
        if args.viewer_cache:
            names = ["yapfast_core", "slint"]
    deps = args.deps_root / "target/fast/deps"
    executable = source.with_suffix(".exe")
    fingerprints = sorted((args.deps_root / "target/fast/.fingerprint").glob("yapfast-core-*/lib-yapfast_core.json"), key=lambda path: path.stat().st_mtime, reverse=True)
    fingerprint = next(json.loads(path.read_text()) for path in fingerprints if '"discord"' in json.loads(path.read_text())["features"])
    ui_fingerprints = sorted((args.deps_root / "target/fast/.fingerprint").glob("yapfast-*/test-bin-yapfast.json"), key=lambda path: path.stat().st_mtime, reverse=True)
    ui_fingerprint = next(json.loads(path.read_text()) for path in ui_fingerprints if '"discord"' in json.loads(path.read_text())["features"])
    if production_core:
        source = root / "crates/yapfast-core/src/lib.rs"
        executable = scratch / "profile-media-core-tests.exe"
        env["CARGO_MANIFEST_DIR"] = str(root / "crates/yapfast-core")
        names = [dep[1] for dep in fingerprint["deps"] if dep[1] != "build_script_build"] + ["png"]
    command = [str(Path.home() / ".cargo/bin/rustc.exe"), "--edition=2024", "--test", str(source), "-L", f"dependency={deps}", "-C", "target-feature=+crt-static", "-C", "codegen-units=2", "-o", str(executable)]
    if production_core:
        command += ["--crate-name", "yapfast_core", "--cfg", 'feature="discord"', "--cfg", 'feature="test-fixtures"', "--cfg", "hwenc", "--cfg", "hevcdec", "--check-cfg", "cfg(hwenc)", "--check-cfg", "cfg(hevcdec)"]
        command += ["--check-cfg", "cfg(test)", "--check-cfg", 'cfg(feature, values("default", "discord", "matrix", "test-fixtures"))']
        output = next((args.deps_root / "target/fast/build").glob("yapfast-core-*/output"))
        for line in output.read_text().splitlines():
            if line.startswith("cargo:rustc-link-lib="):
                command += ["-l", line.split("=", 1)[1]]
    for folder in sorted({p.parent for p in (args.deps_root / "target/fast/build").rglob("*.lib")}):
        command += ["-L", f"native={folder}"]
    for folder in (Path.home() / ".cargo/registry/src").glob("*/windows_x86_64_msvc-*/lib"):
        command += ["-L", f"native={folder}"]
    for name in names:
        recorded = fingerprint if any(dep[1] == name for dep in fingerprint["deps"]) else ui_fingerprint
        if name == "png":
            image = exact_library(args.deps_root / "target/fast", fingerprint, "image")
            image_suffix = image.stem.rsplit("-", 1)[1]
            recorded = json.loads((args.deps_root / f"target/fast/.fingerprint/image-{image_suffix}/lib-image.json").read_text())
        library = exact_library(args.deps_root / "target/fast", recorded, name)
        command += ["--extern", f"{name}={library}"]
    if regression:
        # Compile this worktree's real core, not an obsolete cached core or a
        # duplicate model. Only third-party dependencies are read from target.
        local_core = scratch / "libyapfast_core_profile_media.rlib"
        library_command = list(command)
        library_command[library_command.index("--test")] = "--crate-type=rlib"
        library_command[library_command.index("-o") + 1] = str(local_core)
        subprocess.run(library_command, env=env, cwd=root, check=True, creationflags=low_priority)
        command[command.index(str(source))] = str(args.regression_source or root / "testing/fullres_pixel_regression.rs")
        command[command.index("--crate-name") + 1] = "profile_media_regression"
        command += ["--extern", f"yapfast_core={local_core}"]
    subprocess.run(command, env=env, cwd=root, check=True, creationflags=low_priority)
    if args.core and not tests:
        tests = ["profile_media"]
    result = subprocess.run([str(executable), "--test-threads=1", *tests], env=env, cwd=root, timeout=90, creationflags=low_priority, capture_output=True, text=True, encoding="utf-8", errors="replace")
    print(result.stdout, end="")
    print(result.stderr, end="", file=sys.stderr)
    counts = re.findall(r"test result: .*? (\d+) passed; (\d+) failed;", result.stdout)
    if not counts or all(int(passed) + int(failed) == 0 for passed, failed in counts):
        print("No tests executed; refusing a false-green result.", file=sys.stderr)
        return 2
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
