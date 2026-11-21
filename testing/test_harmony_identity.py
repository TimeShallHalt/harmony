"""Regression contracts for the fork's product identity, not runtime acceptance.

Run: python -m unittest discover -s testing -p test_harmony_identity.py -v
Native builds, smoke tests and GUI verification remain separate release gates.
"""
from pathlib import Path
import re
import struct
import tomllib
import unittest
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
APP_ID = "io.github.TimeShallHalt.Harmony"


def text(name):
    return (ROOT / name).read_text(encoding="utf-8")


class HarmonyIdentity(unittest.TestCase):
    def test_cargo_produces_the_named_executable(self):
        manifest = tomllib.loads(text("Cargo.toml"))
        self.assertEqual(manifest["package"]["name"], "yapfast")  # stable internal package
        self.assertEqual(manifest["package"]["version"], "1.3.0")
        self.assertFalse(manifest["package"]["autobins"])
        self.assertEqual(manifest["bin"], [{"name": "Harmony", "path": "src/main.rs"}])
        self.assertEqual(manifest["package"]["license"], "GPL-3.0-or-later")

    def test_original_svg_artwork_is_not_the_upstream_star(self):
        for path in ("ui/icons/logo.svg", "packaging/icon.svg"):
            source = text(path)
            root = ET.fromstring(source)
            self.assertEqual(root.attrib["viewBox"], "0 0 100 100")
            self.assertIn("Harmony", source)
            self.assertNotIn("M50,14 L58,27", source)
            self.assertNotIn("id=\"dots\"", source)
            self.assertIn("C46", source.replace(" ", ""))

    def test_icon_has_every_native_size_and_a_toast_png(self):
        data = (ROOT / "packaging/windows/harmony.ico").read_bytes()
        reserved, kind, count = struct.unpack_from("<HHH", data)
        self.assertEqual((reserved, kind), (0, 1))
        sizes = set()
        for index in range(count):
            width, height, _, _, _, _, length, offset = struct.unpack_from("<BBBBHHII", data, 6 + 16 * index)
            width, height = width or 256, height or 256
            sizes.add((width, height))
            png = data[offset:offset + length]
            self.assertEqual(len(png), length)
            self.assertTrue(png.startswith(b"\x89PNG\r\n\x1a\n"))
            self.assertEqual(struct.unpack_from(">II", png, 16), (width, height))
        self.assertEqual(sizes, {(n, n) for n in (16, 20, 24, 32, 40, 48, 64, 96, 128, 256)})
        self.assertEqual(count, len(sizes))

    def test_windows_resource_identity_matches_executable(self):
        source = text("packaging/windows/harmony.rc")
        self.assertIn('"harmony.ico"', source)
        for field in ("ProductName", "FileDescription"):
            self.assertRegex(source, rf'VALUE\s+"{field}"\s*,\s*"Harmony')
        self.assertRegex(source, r'VALUE\s+"OriginalFilename"\s*,\s*"Harmony\.exe')
        self.assertIn('arg("harmony.rc")', text("build.rs"))

    def test_linux_entry_and_runtime_share_identity(self):
        entry = text(f"packaging/linux/{APP_ID}.desktop")
        self.assertIn("Name=Harmony\n", entry)
        self.assertIn("Exec=harmony --open-link %u\n", entry)
        self.assertIn(f"Icon={APP_ID}\n", entry)
        self.assertIn("x-scheme-handler/harmony", entry)
        self.assertIn(APP_ID, text("src/icon.rs"))
        self.assertIn(f"{APP_ID}.desktop", text("src/desktop_entry.rs"))
        self.assertIn(r"\nExec=harmony --open-link %u\n", text("src/desktop_entry.rs"))

    def test_toasts_and_tray_use_harmony_art_and_name(self):
        notify = text("src/notify.rs")
        tray = text("src/tray.rs")
        self.assertIn("../packaging/windows/harmony.ico", notify)
        self.assertIn('Some("DisplayName"), "Harmony"', notify)
        self.assertIn('../packaging/icon.svg', tray)
        self.assertNotIn("</mask>", tray)
        self.assertIn('"Open Harmony"', tray)
        self.assertIn('"HarmonyTray"', tray)

    def test_autostart_uses_fork_name_without_enabling_by_default(self):
        source = text("src/autostart.rs")
        self.assertIn('const NAME: &str = "Harmony";', source)
        self.assertIn("if enabled()", source)
        self.assertNotIn('const NAME: &str = "Yapfast";', source)

    def test_upstream_updater_is_not_compiled_or_wired(self):
        self.assertNotRegex(text("src/main.rs"), r"(?m)^\s*mod update\s*;")
        self.assertNotRegex(text("src/ui.rs"), r"(?m)^\s*mod updates\s*;")
        self.assertNotIn("update::fetch", text("src/ui.rs"))
        manifest = tomllib.loads(text("Cargo.toml"))
        self.assertNotIn("dep:minisign-verify", manifest["features"]["desktop"])
        self.assertIn("publisher-signed Windows installers", text("src/build_info.rs"))

    def test_build_stamp_records_exact_upstream_baseline(self):
        for path in ("src/build_info.rs", "scripts/harmony_build_stamp.rs"):
            self.assertIn("Yapfast 0.4.1", text(path))
            self.assertNotIn("Yapfast 0.3.0", text(path))
        self.assertIn("GPL-3.0-or-later", text("src/build_info.rs"))
        self.assertIn("https://github.com/yapfast/yapfast", text("src/build_info.rs"))

    def test_saved_credentials_are_not_invalidated_by_branding(self):
        self.assertIn('const SERVICE: &str = "io.github.yapfast.Yapfast"', text("crates/yapfast-core/src/session/store.rs"))

    def test_own_scheme_does_not_register_over_stock_yapfast(self):
        source = text("src/protocol.rs")
        self.assertIn(r'Software\Classes\harmony', source)
        # The predecessor key must remain readable/retargetable for an exactly
        # owned installation, but registering our own scheme must never claim it.
        registration = source.split('fn register_harmony(', 1)[1].split('fn command_line(', 1)[0]
        self.assertNotIn(r'Software\Classes\yapfast', registration)
        self.assertNotIn('LEGACY_COMMAND', registration)
        self.assertIn(r'Software\Yapfast\DiscordProtocol', source)  # existing rollback record
        self.assertIn('x-scheme-handler/harmony', source)

    def test_merge_does_not_leave_conflict_markers_in_product_sources(self):
        files = [ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "build.rs"]
        for directory, extension in (("src", "*.rs"), ("crates", "*.rs"), ("ui", "*.slint")):
            files.extend((ROOT / directory).rglob(extension))
        conflicts = [str(path.relative_to(ROOT)) for path in files if re.search(r"(?m)^(<<<<<<< |>>>>>>> )", path.read_text(encoding="utf-8"))]
        self.assertEqual(conflicts, [])


if __name__ == "__main__":
    unittest.main()
