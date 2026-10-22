#!/usr/bin/env python3
"""Host-neutral regression for APE binutils dispatch (no cosmo toolchain needed)."""
import importlib.machinery
import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

DRIVER = Path(__file__).resolve().parents[1] / "tools" / "cargo-cosmo"
loader = importlib.machinery.SourceFileLoader("cargo_cosmo_driver", str(DRIVER))
spec = importlib.util.spec_from_loader(loader.name, loader)
driver = importlib.util.module_from_spec(spec)
loader.exec_module(driver)


class InjectDispatch(unittest.TestCase):
    @unittest.skipUnless(os.name == "posix", "shell dispatch is a Unix path")
    def test_non_elf_binutils_use_shell(self):
        with tempfile.TemporaryDirectory(prefix="cosmo-inject-dispatch-") as tmp:
            old_bin = driver.BIN
            driver.BIN = tmp
            try:
                for name in ("nm", "ld", "objcopy"):
                    path = Path(tmp) / f"x86_64-linux-cosmo-{name}"
                    # No shebang: direct subprocess exec must not be used.
                    path.write_text("printf 'DISPATCH_OK:%s' \"$1\"\n")
                    path.chmod(0o755)
                    result = subprocess.run(
                        driver.tool_command("x86_64", name, "argument with spaces"),
                        capture_output=True, check=True,
                    )
                    self.assertEqual(result.stdout, b"DISPATCH_OK:argument with spaces")
            finally:
                driver.BIN = old_bin


if __name__ == "__main__":
    unittest.main()
