"""A failed Cargo suite must stop validation, even if its output looks successful."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


@unittest.skipUnless(os.name == 'posix' and shutil.which('sh'), 'Requires the Linux shell controller')
class RustValidationControllerTests(unittest.TestCase):
    def test_cargo_failure_cannot_be_hidden_by_output_capture(self):
        root = Path(__file__).resolve().parents[1]
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            cargo = directory/'cargo'
            cargo.write_text('#!/bin/sh\ncase "$1" in\n'
                'fmt|clippy) exit 0;;\n'
                'test) printf "test result: ok. 999 passed\\n"; exit 7;;\n'
                '*) touch "$CARGO_TARGET_DIR/unexpected-build"; exit 0;;\nesac\n')
            cargo.chmod(0o700)
            result = subprocess.run(['sh', str(root/'scripts/test_rust_wsl.sh')],
                env={**os.environ, 'PATH':str(directory)+os.pathsep+os.environ['PATH'],
                     'CARGO_TARGET_DIR':str(directory)}, capture_output=True, text=True, timeout=15)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse((directory/'unexpected-build').exists())
            self.assertNotIn('Linux executable:', result.stdout)
