#!/usr/bin/env python3
"""Prove that independent policy drift fails without invoking Cargo or rustup."""

import contextlib
import importlib.util
import io
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("toolchain", Path(__file__).with_name("rust-toolchain.py"))
toolchain = importlib.util.module_from_spec(spec)
spec.loader.exec_module(toolchain)
SOURCE = toolchain.ROOT


class DriftTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        paths = subprocess.check_output(["git", "ls-files", "--cached", "--others", "--exclude-standard"],
                                        cwd=SOURCE, text=True).splitlines()
        for path in paths:
            if Path(path).name == "Cargo.toml" or path in {
                "rust-toolchain.toml", ".github/workflows/ci.yml", ".github/workflows/release.yml",
                "deployment/release/Containerfile", "scripts/verify.sh",
            }:
                destination = self.root / path
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(SOURCE / path, destination)
        subprocess.run(["git", "init", "--quiet", self.root], check=True)
        toolchain.ROOT = self.root
        self.addCleanup(setattr, toolchain, "ROOT", SOURCE)

    def test_current_contract(self):
        with contextlib.redirect_stdout(io.StringIO()):
            toolchain.audit()

    def test_drift_is_rejected(self):
        version = toolchain.policy()["channel"]
        msrv = version.removesuffix(".0")
        changes = [
            ("Cargo.toml", f'rust-version = "{msrv}"', 'rust-version = "1.97"'),
            ("apps/egui-mcp-inspector/Cargo.toml", f'rust-version = "{msrv}"', 'rust-version = "1.97"'),
            ("crates/labello-api/Cargo.toml", 'rust-version.workspace = true', ''),
            ("apps/labello-wasm/Cargo.toml", 'rust-version.workspace = true', 'rust-version = "1.99"'),
            ("rust-toolchain.toml", f'channel = "{version}"', 'channel = "stable"'),
            ("rust-toolchain.toml", '"clippy"', '"rust-docs"'),
            ("rust-toolchain.toml", '"wasm32-unknown-unknown"', '"x86_64-unknown-linux-gnu"'),
            (".github/workflows/ci.yml", 'python3 scripts/rust-toolchain.py install', 'rustup default stable'),
            (".github/workflows/release.yml", 'python3 scripts/rust-toolchain.py check', 'rustc --version'),
            (".github/workflows/release.yml", '@sha256:[0-9a-f]{64}$', '@sha256:.*$'),
            ("deployment/release/Containerfile", f'rust:{version}-bookworm', 'rust:1.97.0-bookworm'),
            ("deployment/release/Containerfile", 'RUN apt-get', 'ENV RUSTUP_TOOLCHAIN=stable\nRUN apt-get'),
            ("scripts/verify.sh", 'wasm_check() {\n    toolchain_check', 'wasm_check() {'),
        ]
        for name, old, new in changes:
            with self.subTest(path=name, replacement=new):
                path = self.root / name
                original = path.read_text()
                self.assertIn(old, original)
                path.write_text(original.replace(old, new, 1))
                try:
                    with self.assertRaises((ValueError, KeyError)):
                        toolchain.audit()
                finally:
                    path.write_text(original)

    def test_nested_pin_is_rejected(self):
        (self.root / "apps/egui-mcp-inspector/rust-toolchain").write_text("stable\n")
        with self.assertRaisesRegex(ValueError, "nested toolchain"):
            toolchain.audit()

    def test_unaudited_manifest_is_rejected(self):
        path = self.root / "tools/unregistered/Cargo.toml"
        path.parent.mkdir(parents=True)
        path.write_text('[package]\nname = "unregistered"\nversion = "0.1.0"\n')
        with self.assertRaisesRegex(ValueError, "every maintained Cargo manifest"):
            toolchain.audit()

    def test_missing_compiler_fails_before_a_proxy_can_install_it(self):
        with patch.object(toolchain.subprocess, "check_output", return_value="stable-x86_64-unknown-linux-gnu\n") as command:
            with self.assertRaisesRegex(ValueError, "not preinstalled"):
                toolchain.check()
            self.assertEqual(command.call_count, 1)
            self.assertEqual(command.call_args.args[0], ["rustup", "toolchain", "list"])

    def test_missing_components_and_target_fail_before_compiler_invocation(self):
        version = toolchain.policy()["channel"]
        installed = f"{version}-x86_64-unknown-linux-gnu\n"
        for responses, message in [
            ([installed, "rustfmt-x86_64-unknown-linux-gnu\n"], "missing toolchain component"),
            ([installed, "rustfmt-x86_64-unknown-linux-gnu\nclippy-x86_64-unknown-linux-gnu\n", ""], "missing WASM target"),
        ]:
            with self.subTest(message=message):
                with patch.object(toolchain.subprocess, "check_output", side_effect=responses) as command:
                    with self.assertRaisesRegex(ValueError, message):
                        toolchain.check()
                    self.assertTrue(all(call.args[0][0] == "rustup" for call in command.call_args_list))


if __name__ == "__main__":
    unittest.main()
