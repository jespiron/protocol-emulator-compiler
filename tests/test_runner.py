"""Runner failures must never masquerade as a successful simulation."""

import importlib.util
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

spec = importlib.util.spec_from_file_location(
    "runner", Path(__file__).resolve().parents[1] / "assets/simulator/run.py"
)
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class RunnerTests(unittest.TestCase):
    def test_results_require_fresh_pass(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "results.xml"
            for text in (
                "bad XML",
                "<testsuite/>",
                "<testsuite><testcase><skipped/></testcase></testsuite>",
                "<testsuite><testcase><failure/></testcase></testsuite>",
                "<testsuite><testcase><error/></testcase></testsuite>",
            ):
                path.write_text(text)
                with self.assertRaises((RuntimeError, runner.ET.ParseError)):
                    runner.verify_results(path)
            path.write_text("<testsuite><testcase name='ok'/></testsuite>")
            runner.verify_results(path)
            path.unlink()
            with self.assertRaises(FileNotFoundError):
                runner.verify_results(path)

    @unittest.skipUnless(os.name == "posix", "runner supports macOS/Linux")
    def test_timeout_kills_process_group(self):
        with self.assertRaises(subprocess.TimeoutExpired):
            runner.command([sys.executable, "-c", "import time; time.sleep(20)"], timeout=0.05)

    def test_missing_tools_are_actionable(self):
        with patch.dict(os.environ, {"PATH": ""}):
            with self.assertRaisesRegex(RuntimeError, "Missing git"):
                runner.prerequisites()

    def test_nonzero_is_error(self):
        with self.assertRaises(RuntimeError):
            runner.command([sys.executable, "-c", "raise SystemExit(2)"])


if __name__ == "__main__":
    unittest.main()
