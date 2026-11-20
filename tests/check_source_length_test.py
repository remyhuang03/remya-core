import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


CHECKER = Path(__file__).resolve().parents[1] / "tool" / "check_source_length.py"


class CheckSourceLengthTest(unittest.TestCase):
    def test_threshold_and_content_based_exception(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "-q", root], check=True)
            tool = root / "tool"
            tool.mkdir()
            script = shutil.copy(CHECKER, tool)
            exceptions = tool / "source_length_exceptions.json"
            exceptions.write_text("{}\n")
            (root / ".gitignore").write_text("ignored.rs\n")
            (root / "ignored.rs").write_bytes(b"\n" * 801)
            (root / "at limit.dart").write_bytes(b"\r\n" * 800)
            dart = root / "over limit.dart"
            dart.write_bytes(b"\n" * 801)
            rust = root / "over limit.rs"
            rust.write_bytes(b"\n" * 800 + b"// No final newline")
            subprocess.run(["git", "-C", root, "add", rust.name], check=True)

            def run(*args, github_actions="false", check=True):
                return subprocess.run(
                    [sys.executable, script, *args],
                    cwd=tool,
                    env={**os.environ, "GITHUB_ACTIONS": github_actions},
                    text=True,
                    capture_output=True,
                    check=check,
                )

            warnings = run(github_actions="true").stdout.splitlines()
            self.assertEqual(len(warnings), 2)
            for path, warning in zip([dart, rust], warnings, strict=True):
                self.assertTrue(warning.startswith("::warning "))
                self.assertIn(f"{path.name} has 801 lines", warning)

            reason = "Generated bindings"
            run("add", rust.name, "--reason", reason)
            self.assertEqual(json.loads(exceptions.read_text()), {
                rust.name: {"sha256": hashlib.sha256(rust.read_bytes()).hexdigest(),
                            "reason": reason},
            })
            self.assertNotIn(rust.name, run().stdout)
            self.assertIn(dart.name, run().stdout)

            invalid = run("add", "at limit.dart", "--reason", reason, check=False)
            self.assertEqual(invalid.returncode, 2)
            rust.write_bytes(b"\n" * 800 + b"// Changed, still 801 lines")
            self.assertIn(rust.name, run().stdout)
            run("add", rust.name, "--reason", reason)
            self.assertNotIn(rust.name, run().stdout)
            rust.unlink()
            self.assertNotIn(rust.name, run().stdout)
