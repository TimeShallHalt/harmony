"""Exercise the real version tool against isolated copies, never the checkout."""
from pathlib import Path
from datetime import date
import os
import runpy
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]
TOOL = runpy.run_path(str(ROOT / "scripts/set-version"))
FILES = ("Cargo.toml", "Cargo.lock", "packaging/windows/harmony.rc", "packaging/arch/PKGBUILD", "packaging/linux/io.github.TimeShallHalt.Harmony.metainfo.xml")


class SetVersionTest(unittest.TestCase):
    def setUp(self):
        scratch = Path(os.environ.get("HARMONY_TEST_SCRATCH", ROOT / ".local/test-scratch"))
        scratch.mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(prefix="harmony-version-", dir=scratch)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for relative in (*FILES, "scripts/set-version"):
            dest = self.root / relative
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, dest)

    def test_stable_updates_every_native_version_and_adds_real_release_container(self):
        changes = TOOL["plan"](self.root, "1.2.3", "2026-11-16")
        self.assertEqual(tomllib.loads(changes["Cargo.toml"])["package"]["version"], "1.2.3")
        lock = tomllib.loads(changes["Cargo.lock"])
        self.assertEqual(next(p["version"] for p in lock["package"] if p["name"] == "yapfast"), "1.2.3")
        rc = changes["packaging/windows/harmony.rc"]
        self.assertIn("FILEVERSION 1,2,3,0", rc)
        self.assertIn("PRODUCTVERSION 1,2,3,0", rc)
        self.assertIn('VALUE "FileVersion", "1.2.3', rc)
        self.assertIn('VALUE "ProductVersion", "1.2.3', rc)
        self.assertIn('"OriginalFilename", "Harmony.exe', rc)
        self.assertIn('<release version="1.2.3" date="2026-11-16"/>', changes[FILES[-1]])

    def test_prerelease_updates_binary_but_does_not_publish_stable_metadata(self):
        changes = TOOL["plan"](self.root, "1.2.3-rc.1", "2026-11-16")
        self.assertNotIn(FILES[-1], changes)
        self.assertNotIn("packaging/arch/PKGBUILD", changes)
        self.assertIn("FILEVERSION 1,2,3,0", changes["packaging/windows/harmony.rc"])
        self.assertIn('"1.2.3-rc.1', changes["packaging/windows/harmony.rc"])

    def test_invalid_tokens_never_normalize_or_mutate(self):
        before = {p: (self.root / p).read_bytes() for p in FILES}
        for value in ("01.2.3", "1.2.3-01", "+1.2.3", " 1.2.3", "1.2.3 ", "1.2", "1.2.3\n", "65536.0.0", '1.2.3"'):
            with self.subTest(value=value), self.assertRaises(ValueError):
                TOOL["plan"](self.root, value, "2026-11-16")
        self.assertEqual(before, {p: (self.root / p).read_bytes() for p in FILES})

    def test_actual_cli_and_idempotent_release_entry(self):
        result = subprocess.run([sys.executable, str(self.root / "scripts/set-version"), "1.2.3"], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        xml = (self.root / FILES[-1]).read_text()
        self.assertIn(f'date="{date.today().isoformat()}"', xml)
        result2 = subprocess.run([sys.executable, str(self.root / "scripts/set-version"), "1.2.3"], capture_output=True, text=True)
        self.assertEqual(result2.returncode, 0, result2.stderr)
        self.assertEqual((self.root / FILES[-1]).read_text().count('<release version="1.2.3"'), 1)

    def test_missing_resource_field_fails_before_writes(self):
        path = self.root / "packaging/windows/harmony.rc"
        path.write_text(path.read_text().replace("FILEVERSION", "REMOVED_VERSION"))
        before = {p: (self.root / p).read_bytes() for p in FILES}
        with self.assertRaises(ValueError):
            TOOL["plan"](self.root, "1.2.3", "2026-11-16")
        self.assertEqual(before, {p: (self.root / p).read_bytes() for p in FILES})


if __name__ == "__main__":
    unittest.main()
