"""Run focused window tests without locking Cargo or rebuilding native media.

Requires the matching desktop dependencies already built with --profile fast.
Each test process owns its windows; this runner never finds or closes an app.
Use --native for the actual Win32 message-boundary test. It injects no
physical input but manipulates OS mouse capture, so run only on an idle desktop.
"""
from pathlib import Path
import argparse
import json
import os
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--native", action="store_true")
    parser.add_argument("--build-probe", action="store_true", help="compile the isolated real-input probe; do not launch")
    parser.add_argument("--output-dir", type=Path)
    args, test_args = parser.parse_known_args()
    if sys.platform != "win32":
        parser.error("this cached-dependency runner is Windows-only; Cargo runs the shared tests on Linux")
    root = Path(__file__).resolve().parents[1]
    scratch = args.output_dir or Path(os.environ["LOCALAPPDATA"]) / "hermes/cache/scratch/harmony-chrome-tests"
    scratch.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ)
    # Prefer an existing developer shell. Otherwise discover the installed VS.
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
    source = scratch / "harness.rs"
    test_source = root / "src/ui" / ("title_bar_native_tests.rs" if args.native else "title_bar_tests.rs")
    source.write_text((f'#[path = "{(root / "src/ui/title_bar/windows.rs").as_posix()}"]\nmod windows;\n' if args.native else '') + f'#[path = "{test_source.as_posix()}"]\nmod chrome_tests;\n', encoding="utf-8")
    deps = root / "target/fast/deps"
    executable = scratch / ("window-chrome-probe.exe" if args.build_probe else "native-tests.exe" if args.native else "hit-region-tests.exe")
    if args.build_probe:
        source = root / "testing/window_chrome_native_probe.rs"
    command = ["rustc", "--edition=2024", *([] if args.build_probe else ["--test"]), str(source), "-L", f"dependency={deps}", "-C", "target-feature=+crt-static", "-o", str(executable)]
    for folder in sorted({p.parent for p in (root / "target/fast/build").rglob("*.lib")}):
        command += ["-L", f"native={folder}"]
    for folder in (Path.home() / ".cargo/registry/src").glob("*/windows_x86_64_msvc-*/lib"):
        command += ["-L", f"native={folder}"]
    names = ["slint", "windows_sys", "tracing"] if args.native or args.build_probe else ["slint", "i_slint_backend_testing", "i_slint_core"]
    for name in names:
        libraries = sorted(deps.glob(f"lib{name}-*.rlib"), key=lambda p: p.stat().st_mtime, reverse=True)
        if name == "windows_sys":
            libraries = [p for p in libraries if "Win32_UI_WindowsAndMessaging" in json.loads((root / "target/fast/.fingerprint" / p.stem.removeprefix("lib").replace("windows_sys-", "windows-sys-") / "lib-windows_sys.json").read_text())["features"] and "windows-sys-0.61." in (deps / (p.stem.removeprefix("lib") + ".d")).read_text()]
        if not libraries:
            raise SystemExit(f"Missing cached {name}; build desktop tests with --profile fast first")
        command += ["--extern", f"{name}={libraries[0]}"]
    subprocess.run(command, env=env, cwd=root, check=True)
    if args.build_probe:
        print(executable)
        return 0
    native_opt_in = ["--include-ignored"] if args.native else []
    return subprocess.run([str(executable), "--test-threads=1", *native_opt_in, *test_args], env=env, cwd=root, timeout=60).returncode


if __name__ == "__main__":
    raise SystemExit(main())
