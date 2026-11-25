"""Do not accidentally exercise the stale pre-rebrand executable in native tooling."""
from pathlib import Path
import os
import tempfile
import unittest
from unittest import mock
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
# The CLI has an intentionally top-level parser. Load its unchanged definitions
# without parsing unittest's arguments or starting a desktop session.
SESSION: dict[str, Any] = {"__file__": str(ROOT / "scripts/ui-session"), "__name__": "ui_session_fixture"}
DEFINITIONS = (ROOT / "scripts/ui-session").read_text().split("\nparser = argparse.ArgumentParser", 1)[0]
exec(compile(DEFINITIONS, str(ROOT / "scripts/ui-session"), "exec"), SESSION)


class DevelopmentBinaryPaths(unittest.TestCase):
    def test_ui_session_builds_and_copies_the_explicit_harmony_target(self):
        scratch = Path(os.environ.get("HARMONY_TEST_SCRATCH", ROOT / ".local/test-scratch"))
        scratch.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=scratch) as temp:
            root = Path(temp)
            state = root / ".local/ui-session"
            state.mkdir(parents=True)
            binary = root / "target/fast/Harmony"
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"new harmony build fixture")
            (binary.parent / "yapfast").write_bytes(b"old stale build fixture")
            function = SESSION["build"]
            with mock.patch.dict(function.__globals__, ROOT=root, STATE=state), mock.patch("subprocess.run", return_value=mock.Mock(returncode=0)) as run:
                function(True, "fast")
            command = run.call_args.args[0]
            self.assertEqual(command[command.index("--bin") + 1], "Harmony")
            # Private driver state filenames remain stable for test automation.
            self.assertEqual((state / "yapfast").read_bytes(), b"new harmony build fixture")

    def test_ui_session_never_falls_back_to_a_stale_yapfast_build(self):
        scratch = Path(os.environ.get("HARMONY_TEST_SCRATCH", ROOT / ".local/test-scratch"))
        scratch.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=scratch) as temp:
            root = Path(temp)
            state = root / ".local/ui-session"
            state.mkdir(parents=True)
            old = root / "target/fast/yapfast"
            old.parent.mkdir(parents=True)
            old.write_bytes(b"old stale fixture")
            function = SESSION["build"]
            with mock.patch.dict(function.__globals__, ROOT=root, STATE=state), mock.patch("subprocess.run", return_value=mock.Mock(returncode=0)):
                with self.assertRaises(FileNotFoundError):
                    function(False, "fast")

    def test_smoke_gate_targets_new_binary_and_handles_windows_suffix(self):
        source = (ROOT / "scripts/check").read_text()
        self.assertIn('program="target/$profile_dir/Harmony"', source)
        self.assertIn('program+=".exe"', source)
        self.assertIn('run smoke "$program" --smoke-test', source)
        self.assertNotIn('"target/$profile_dir/yapfast"', source)


if __name__ == "__main__":
    unittest.main()
