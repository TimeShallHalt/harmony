"""Compile real core tests using cached dependencies; never runs Cargo or the UI.

Windows-only convenience runner. Pass --cache-root to the integration checkout.
All compiler/test outputs stay in this worker's private Hermes scratch directory.
"""
import argparse
import os
from pathlib import Path
import re
import subprocess
import sys


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache-root", type=Path, required=True)
    parser.add_argument("--ui-cache", action="store_true", help="test the production viewer conversion cache only")
    parser.add_argument("filters", nargs="*")
    args = parser.parse_args(argv)
    if not args.filters:
        args.filters = ["viewer_picture" if args.ui_cache else "viewer_lifecycle"]
    return args


def libtest_exit_code(returncode, output):
    """Require a successful summary with executed tests, not just libtest exit 0."""
    if returncode:
        return returncode
    summary = re.search(r"^test result: ok\. ([0-9]+) passed; 0 failed;", output, re.MULTILINE)
    return 0 if summary and int(summary[1]) > 0 else 1


def main(argv=None):
    args = parse_args(argv)
    root = Path(__file__).resolve().parents[1]
    core = root / "crates/yapfast-core"
    cache = args.cache_root.resolve() / "target/fast"
    scratch = Path(os.environ.get("TMPDIR", Path.home() / "AppData/Local/hermes/cache/scratch")) / "harmony-fullres-lifecycle"
    scratch.mkdir(parents=True, exist_ok=True)
    vc = Path("C:/Program Files/Microsoft Visual Studio/2022/Community/VC/Auxiliary/Build/vcvars64.bat")
    env = dict(os.environ)
    result = subprocess.run('call "' + str(vc) + '" >nul && set', shell=True,
                            capture_output=True, text=True, encoding="mbcs", check=True)
    for line in result.stdout.splitlines():
        if "=" in line and not line.startswith("="):
            key, value = line.split("=", 1)
            env[key] = value
    for key in ("TEMP", "TMP", "TMPDIR"):
        env[key] = str(scratch)
    env["CARGO_MANIFEST_DIR"] = str(core)
    env["CARGO_PKG_VERSION"] = "0.1.0"
    deps = cache / "deps"
    exe = scratch / ("viewer-picture-tests.exe" if args.ui_cache else "viewer-lifecycle-tests.exe")
    source = core / "src/lib.rs"
    if args.ui_cache:
        source = scratch / "viewer-picture-harness.rs"
        source.write_text('#[path = "' + (root / "src/ui/viewer_picture.rs").as_posix()
                          + '"]\nmod viewer_picture;\n', encoding="utf-8")
    cmd = [str(Path.home() / ".cargo/bin/rustc.exe"), "--edition=2024", "--test",
           "--crate-name", "fullres_viewer_tests", str(source),
           "--cfg", 'feature="discord"', "-C", "target-feature=+crt-static",
           "-C", "debuginfo=0", "-C", "codegen-units=2", "-L", "dependency=" + str(deps), "-o", str(exe)]
    fingerprints = cache / ".fingerprint"
    core_fingerprints = sorted(fingerprints.glob("yapfast-core-*/lib-yapfast_core.json"),
                               key=lambda p: p.stat().st_mtime, reverse=True)
    import json
    metadata = json.loads(core_fingerprints[0].read_text(encoding="utf-8"))
    for _, name, _, fingerprint in metadata["deps"]:
        if name == "build_script_build":
            continue
        wanted = fingerprint.to_bytes(8, "little").hex()
        matches = [p for p in fingerprints.glob("*/lib-" + name)
                   if p.read_text(encoding="utf-8").strip() == wanted]
        if len(matches) != 1:
            raise SystemExit(f"Expected one exact cached fingerprint for {name}, got {matches}")
        suffix = matches[0].parent.name.rsplit("-", 1)[1]
        lib = deps / ("lib" + name + "-" + suffix + ".rlib")
        if not lib.exists():
            raise SystemExit(f"Missing cached dependency {lib}")
        cmd += ["--extern", name + "=" + str(lib)]
    # PNG is used only by core's fixture tests.
    png = sorted(deps.glob("libpng-*.rlib"), key=lambda p: p.stat().st_mtime, reverse=True)[0]
    cmd += ["--extern", "png=" + str(png)]
    for folder in sorted({p.parent for p in (cache / "build").rglob("*.lib")}):
        cmd += ["-L", "native=" + str(folder)]
    for folder in (Path.home() / ".cargo/registry/src").glob("*/windows_x86_64_msvc-*/lib"):
        cmd += ["-L", "native=" + str(folder)]
    # These are emitted by core/build.rs, not encoded in a dependency rlib.
    outputs = sorted((cache / "build").glob("yapfast-core-*/output"), key=lambda p: p.stat().st_mtime, reverse=True)
    if not outputs:
        raise SystemExit("No cached core native build-script output")
    for line in outputs[0].read_text(encoding="utf-8").splitlines():
        for directive, flag in (("cargo:rustc-link-lib=", "-l"), ("cargo:rustc-cfg=", "--cfg")):
            if line.startswith(directive):
                cmd += [flag, line.removeprefix(directive)]
    if args.ui_cache:
        for name in ("yapfast_core", "slint"):
            lib = sorted(deps.glob("lib" + name + "-*.rlib"), key=lambda p: p.stat().st_mtime, reverse=True)[0]
            cmd += ["--extern", name + "=" + str(lib)]
    print("Compiling production viewer tests, no Cargo/UI/network", flush=True)
    result = subprocess.run(cmd, cwd=core, env=env, creationflags=subprocess.BELOW_NORMAL_PRIORITY_CLASS)
    if result.returncode:
        sys.exit(result.returncode)
    for test_filter in args.filters:
        result = subprocess.run([str(exe), test_filter, "--test-threads=1", "--nocapture"], cwd=core, env=env,
                                capture_output=True, text=True, encoding="utf-8", errors="replace",
                                creationflags=subprocess.BELOW_NORMAL_PRIORITY_CLASS)
        print(result.stdout, end="", flush=True)
        print(result.stderr, end="", file=sys.stderr, flush=True)
        exit_code = libtest_exit_code(result.returncode, result.stdout)
        if exit_code:
            if not result.returncode:
                print(f"No successful nonempty libtest summary for filter {test_filter!r}; refusing success.",
                      file=sys.stderr)
            sys.exit(exit_code)


if __name__ == "__main__":
    main()
