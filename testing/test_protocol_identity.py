"""Account-free registration regressions; never calls the real OS registry.

Run: python3 -m unittest discover -s testing -p test_protocol_identity.py -v
Compiles the actual Windows modules against an in-memory Win32 registry fixture,
plus the production pure Linux ownership helpers. No Cargo/target writes, desktop
input, live registry/shortcut writes, or changes to the user's integration files.
This is focused source/native-module coverage, not a full Linux desktop build.
"""
from pathlib import Path
import os
import re
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "testing/fixtures/protocol_identity.rs"


def source(name):
    return (ROOT / name).read_text(encoding="utf-8")


def between(text, start, end):
    return text.split(start, 1)[1].split(end, 1)[0]


class ProtocolIdentity(unittest.TestCase):
    def test_normal_refresh_has_no_registration_or_backup_creation_path(self):
        windows = source("src/protocol.rs").split("mod windows {", 1)[1]
        refresh = between(windows, "pub fn refresh(root:", "/// Registers Harmony")
        self.assertNotIn("register_harmony", refresh)
        self.assertNotIn("BACKUP", refresh)
        self.assertIn("[COMMAND, HARMONY_COMMAND, LEGACY_COMMAND]", refresh)
        self.assertIn("replace_existing(keys[index], previous, &updated)", refresh)
        replacement = between(windows, "fn replace_existing(", "impl Value")
        self.assertIn("read_open_value(handle.0, std::ptr::null())?.as_ref() == Some(previous)", replacement)
        self.assertIn("RegOpenKeyExW", replacement)
        self.assertIn("RegSetValueExW", replacement)
        self.assertNotIn("RegSetKeyValueW", replacement)
        self.assertIn("previous.kind", replacement)

    def test_raw_previous_handler_storage_is_not_string_reencoding(self):
        text = source("src/protocol.rs")
        self.assertIn('const BACKUP: &str = r"Software\\Yapfast\\DiscordProtocol";', text)
        self.assertIn("RegQueryValueExW", text)
        self.assertIn("set_raw_value(BACKUP, Some(PREVIOUS_COMMAND), previous)", text)
        self.assertIn("set_raw_value(COMMAND, None, &previous)", text)
        self.assertNotIn("kind.max(REG_SZ)", text)
        self.assertNotIn("fn is_ours(", text)

    def test_actual_new_desktop_template_mime_and_runtime_assertion(self):
        desktop = source("packaging/linux/io.github.TimeShallHalt.Harmony.desktop")
        self.assertIn("\nName=Harmony\n", desktop)
        self.assertIn("\nMimeType=x-scheme-handler/harmony;\n", desktop)
        self.assertNotIn("x-scheme-handler/yapfast", desktop)
        runtime = source("src/desktop_entry.rs")
        self.assertIn('include_str!("../packaging/linux/io.github.TimeShallHalt.Harmony.desktop")', runtime)
        self.assertIn(r'assert!(text.contains("\nMimeType=x-scheme-handler/harmony;\n"))', runtime)

    def test_linux_refresh_retains_only_proven_owned_legacy_entries(self):
        autostart = source("src/autostart.rs").split("mod linux {", 1)[1].split("#[cfg(windows)]", 1)[0]
        self.assertIn("desktop_entry::identity::owned", autostart)
        self.assertIn("desktop_entry::identity::active", autostart)
        self.assertIn("desktop_entry::identity::refreshed", autostart)
        protocol = source("src/protocol.rs").split("mod linux {", 1)[1]
        refresh = between(protocol, "pub fn refresh(root:", "fn launch(")
        self.assertNotIn("apply", refresh)
        self.assertNotIn("install", refresh)
        self.assertIn("legacy_desktop(root)", refresh)
        self.assertIn("legacy_owned && names_legacy(line)", protocol)
        self.assertIn("desktop_entry::identity::owned(&text, &command)", protocol)

    def test_native_production_modules_and_pure_linux_contracts(self):
        rustc = shutil.which("rustc")
        if rustc is None:
            self.fail("rustc is required for the executable regression fixture")
        if os.name != "nt":
            self.skipTest("full in-memory Win32 fixture currently targets Windows; pure helpers remain portable")
        scratch = Path(os.environ.get("TMPDIR", tempfile.gettempdir()))
        scratch.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix="protocol-identity-", dir=scratch) as folder:
            folder = Path(folder)
            desktop = source("src/desktop_entry.rs")
            quoted = between(desktop, "pub(crate) fn quoted(", "// BEGIN pure Linux desktop ownership")
            quoted = "pub(crate) fn quoted(" + quoted
            identity = between(desktop, "// BEGIN pure Linux desktop ownership (also exercised on non-Linux hosts).", "// END pure Linux desktop ownership.")
            fixture = FIXTURE.read_text(encoding="utf-8")
            fixture = fixture.replace("// @AUTOSTART@", f'#[path = {str(ROOT / "src/autostart.rs").replace(chr(92), "/")!r}] mod autostart;'.replace("'", '"'))
            fixture = fixture.replace("// @PROTOCOL@", f'#[path = {str(ROOT / "src/protocol.rs").replace(chr(92), "/")!r}] mod protocol;'.replace("'", '"'))
            fixture = fixture.replace("// @LINUX_DESKTOP@", quoted + identity)
            entry = "fn entry(" + between(desktop, "fn entry(", "/// Whether an AppImage manager")
            template = (ROOT / "packaging/linux/io.github.TimeShallHalt.Harmony.desktop").as_posix()
            fixture = fixture.replace("// @ACTUAL_DESKTOP_TEMPLATE@", f'const ENTRY: &str = include_str!("{template}");\n' + entry)
            # Include actual MIME selection/editing helpers, not a Python model.
            linux = source("src/protocol.rs").split("mod linux {", 1)[1]
            mime = between(linux, "    /// Whether a default line names Harmony first.", "    /// Writes the list where it is:")
            mime += "fn enabled_with_legacy(" + between(linux, "    fn enabled_with_legacy(", "    /// Claims the schemes")
            mime += "fn apply_with_legacy(" + between(linux, "    fn apply_with_legacy(", "    /// Whether a default line names Harmony first.")
            fixture = fixture.replace("// @LINUX_MIME@", mime)
            linux_autostart = between(source("src/autostart.rs"), '#[cfg(target_os = "linux")]\nmod linux {', '#[cfg(windows)]\nmod windows {')
            # Keep the entire real Linux autostart module, including its tests;
            # only its XDG/write adapters are fixture-bound sandbox operations.
            fixture = fixture.replace("// @LINUX_AUTOSTART@", "mod linux {" + linux_autostart.rsplit('}', 1)[0] + "}")
            generated = folder / "identity_fixture.rs"
            generated.write_text(fixture, encoding="utf-8")
            binary = folder / "Harmony.exe"
            env = os.environ.copy()
            env.update({name: str(folder) for name in ("TMPDIR", "TMP", "TEMP")})
            compile_run = subprocess.run([rustc, "--edition=2024", "--test", "--crate-name", "protocol_identity_fixture", str(generated), "-o", str(binary)], capture_output=True, text=True, env=env, timeout=120)
            self.assertEqual(compile_run.returncode, 0, compile_run.stdout + compile_run.stderr)
            tested = subprocess.run([str(binary), "--test-threads=1", "--nocapture"], capture_output=True, text=True, env=env, timeout=120)
            print("\n" + tested.stdout, end="")
            self.assertEqual(tested.returncode, 0, tested.stdout + tested.stderr)
            self.assertRegex(tested.stdout, r"test result: ok\. [1-9][0-9]* passed; 0 failed;")


    def test_real_win32_bindings_compile_without_linking_or_live_calls(self):
        if os.name != "nt":
            self.skipTest("Windows native bindings require a Windows host")
        rustc = shutil.which("rustc")
        if rustc is None:
            self.fail("rustc is required")
        deps = ROOT / "target/fast/deps"
        registries = []
        import json
        for fingerprint in (ROOT / "target/fast/.fingerprint").glob("windows-sys-*/lib-windows_sys.json"):
            if "Win32_System_Registry" in json.loads(fingerprint.read_text())["features"]:
                library = deps / ("lib" + fingerprint.parent.name.replace("windows-sys", "windows_sys") + ".rlib")
                if library.exists():
                    registries.append(library)
        traces = list(deps.glob("libtracing-*.rlib"))
        if not registries or not traces:
            self.skipTest("no cached native bindings; parent must run the integrated native build")
        scratch = Path(os.environ.get("TMPDIR", tempfile.gettempdir()))
        with tempfile.TemporaryDirectory(prefix="protocol-real-bindings-", dir=scratch) as folder:
            folder = Path(folder)
            harness = folder / "bindings.rs"
            modules = "\n".join(f'#[path = "{(ROOT / name).as_posix()}"] mod {Path(name).stem};' for name in ("src/autostart.rs", "src/protocol.rs"))
            harness.write_text('extern crate self as yapfast_core;\npub const OPEN_LINK: &str = "--open-link";\npub fn default_directory() -> std::io::Result<std::path::PathBuf> { Ok(std::path::PathBuf::from(r"C:\\fixture\\data")) }\n' + modules, encoding="utf-8")
            env = os.environ.copy()
            env.update({name: str(folder) for name in ("TMPDIR", "TMP", "TEMP")})
            result = subprocess.run([rustc, "--edition=2024", "--crate-type=lib", "--emit=metadata", str(harness), "-o", str(folder / "bindings.rmeta"), "-L", "dependency=" + str(deps), "--extern", "windows_sys=" + str(max(registries, key=lambda p: p.stat().st_size)), "--extern", "tracing=" + str(traces[0])], capture_output=True, text=True, env=env, timeout=120)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
