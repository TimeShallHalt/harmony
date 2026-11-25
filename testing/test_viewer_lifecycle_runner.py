"""Viewer runner regressions; native execution is explicitly opt-in.

Run Python tests: python3 -m unittest testing.test_viewer_lifecycle_runner -v
Set HARMONY_VIEWER_RUNNER_CACHE_ROOT to also compile/run the real native runner.
No compiler or test subprocess is mocked as native execution evidence.
"""
import os
from pathlib import Path
import subprocess
import sys
import unittest

from testing import run_viewer_lifecycle_tests as runner

ROOT = Path(__file__).resolve().parents[1]
CACHE_ROOT = os.environ.get("HARMONY_VIEWER_RUNNER_CACHE_ROOT")


class RunnerArgumentTests(unittest.TestCase):
    def test_lifecycle_default(self):
        args = runner.parse_args(["--cache-root", "cache"])
        self.assertFalse(args.ui_cache)
        self.assertEqual(args.cache_root, Path("cache"))
        self.assertEqual(args.filters, ["viewer_lifecycle"])

    def test_ui_cache_default(self):
        args = runner.parse_args(["--cache-root", "cache", "--ui-cache"])
        self.assertTrue(args.ui_cache)
        self.assertEqual(args.filters, ["viewer_picture"])

    def test_explicit_filters_are_preserved_in_both_modes(self):
        for mode in ([], ["--ui-cache"]):
            for filters in (["viewer_picture"], ["viewer_lifecycle", "no_match"], [""]):
                with self.subTest(mode=mode, filters=filters):
                    args = runner.parse_args(["--cache-root", "cache", *mode, *filters])
                    self.assertEqual(args.filters, filters)


class LibtestExitCodeTests(unittest.TestCase):
    """Literal libtest text is parser input, not claimed native execution."""

    def test_positive_pass_count_succeeds(self):
        for count in (1, 2):
            with self.subTest(count=count):
                output = (f"running {count} tests\r\n"
                          f"test result: ok. {count} passed; 0 failed; 0 ignored; "
                          "0 measured; 0 filtered out; finished in 0.00s\r\n")
                self.assertEqual(runner.libtest_exit_code(0, output), 0)

    def test_zero_executed_tests_fail(self):
        for ignored in (0, 2):
            with self.subTest(ignored=ignored):
                output = ("test result: ok. 0 passed; 0 failed; "
                          f"{ignored} ignored; 0 measured; 2 filtered out; finished in 0.00s\n")
                self.assertNotEqual(runner.libtest_exit_code(0, output), 0)

    def test_missing_summary_fails_even_with_a_running_tests_banner(self):
        for output in ("", "running 2 tests\n", "test result: unrecognized\n"):
            with self.subTest(output=output):
                self.assertNotEqual(runner.libtest_exit_code(0, output), 0)

    def test_reported_failure_cannot_be_success(self):
        for status in ("FAILED", "ok"):
            with self.subTest(status=status):
                output = f"test result: {status}. 1 passed; 1 failed; 0 ignored;\n"
                self.assertNotEqual(runner.libtest_exit_code(0, output), 0)

    def test_native_failure_exit_code_is_preserved(self):
        for code in (1, 101):
            for output in ("", "test result: ok. 2 passed; 0 failed; 0 ignored;\n"):
                with self.subTest(code=code, output=output):
                    self.assertEqual(runner.libtest_exit_code(code, output), code)


@unittest.skipUnless(sys.platform == "win32" and CACHE_ROOT,
                     "native tests require Windows and HARMONY_VIEWER_RUNNER_CACHE_ROOT")
class NativeViewerRunnerTests(unittest.TestCase):
    def run_native(self, *arguments):
        command = [sys.executable, str(ROOT / "testing/run_viewer_lifecycle_tests.py"),
                   "--cache-root", CACHE_ROOT, *arguments]
        result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True,
                                encoding="utf-8", errors="replace", timeout=600,
                                creationflags=subprocess.BELOW_NORMAL_PRIORITY_CLASS)
        print(result.stdout, end="", flush=True)
        print(result.stderr, end="", file=sys.stderr, flush=True)
        return result

    def test_ui_cache_default_executes_two_real_tests(self):
        result = self.run_native("--ui-cache")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("test result: ok. 2 passed; 0 failed;", result.stdout)

    def test_ui_cache_no_match_fails_even_when_libtest_succeeds(self):
        result = self.run_native("--ui-cache", "__fullres_runner_no_test_matches__")
        self.assertIn("test result: ok. 0 passed; 0 failed;", result.stdout)
        self.assertEqual(result.returncode, 1, "zero executed tests must fail closed")
        self.assertIn("refusing success", result.stderr)

    def test_lifecycle_default_executes_real_tests(self):
        result = self.run_native()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("viewer_lifecycle_", result.stdout)
        self.assertRegex(result.stdout, r"test result: ok\. [1-9][0-9]* passed; 0 failed;")

    def test_ui_cache_explicit_filter_still_executes_two_real_tests(self):
        result = self.run_native("--ui-cache", "viewer_picture")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("test result: ok. 2 passed; 0 failed;", result.stdout)

    def test_no_match_fails_after_a_successful_filter(self):
        result = self.run_native("--ui-cache", "viewer_picture", "__fullres_runner_no_test_matches__")
        self.assertIn("test result: ok. 2 passed; 0 failed;", result.stdout)
        self.assertIn("test result: ok. 0 passed; 0 failed;", result.stdout)
        self.assertEqual(result.returncode, 1)
        self.assertIn("refusing success", result.stderr)


if __name__ == "__main__":
    unittest.main()
