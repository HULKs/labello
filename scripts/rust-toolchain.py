#!/usr/bin/env python3
"""Install, check, and audit the compiler policy without resolving dependencies."""

import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read_toml(path):
    return tomllib.loads((ROOT / path).read_text())


def policy():
    pin = read_toml("rust-toolchain.toml")["toolchain"]
    require(re.fullmatch(r"\d+\.\d+\.0", pin["channel"]), "pin an exact x.y.0 compiler")
    require(pin["profile"] == "minimal", "toolchain profile must be minimal")
    require(set(pin["components"]) == {"rustfmt", "clippy"}, "pin rustfmt and Clippy")
    require(pin["targets"] == ["wasm32-unknown-unknown"], "pin the WASM target")
    return pin


def audit():
    pin = policy()
    msrv = pin["channel"].removesuffix(".0")
    manifests = {"Cargo.toml", "apps/egui-mcp-inspector/Cargo.toml"}
    for workspace_path in sorted(manifests):
        workspace = read_toml(workspace_path)
        require(workspace["workspace"]["package"]["rust-version"] == msrv,
                f"{workspace_path}: workspace MSRV must match the toolchain")
        directory = Path(workspace_path).parent
        members = workspace["workspace"].get("members", [])
        packages = [directory / member / "Cargo.toml" for member in members]
        if "package" in workspace:
            packages.append(Path(workspace_path))
        for package_path in packages:
            manifests.add(str(package_path))
            package = read_toml(package_path)["package"]
            require(package.get("rust-version") in (msrv, {"workspace": True}),
                    f"{package_path}: package must inherit MSRV or declare {msrv}")

    tracked = subprocess.check_output(["git", "ls-files", "--cached", "--others",
                                       "--exclude-standard"], cwd=ROOT, text=True).splitlines()
    actual = {path for path in tracked if Path(path).name == "Cargo.toml"}
    require(actual == manifests, "every maintained Cargo manifest must belong to an audited workspace")
    require({path for path in tracked if Path(path).name in {"rust-toolchain", "rust-toolchain.toml"}}
            == {"rust-toolchain.toml"}, "nested toolchain overrides are forbidden")

    ci = (ROOT / ".github/workflows/ci.yml").read_text()
    jobs = re.split(r"^  [a-z_]+:\n", ci, flags=re.MULTILINE)[1:]
    for job in jobs:
        if re.search(r'./scripts/verify.sh ci "\$CI_BASE_SHA" (format|clippy|workspace-tests|ui-tests|doctests|inspector|wasm|browser)\b', job):
            require(job.count("python3 scripts/rust-toolchain.py install") == 1,
                    "each compiling CI job must install the shared toolchain policy")
    release = (ROOT / ".github/workflows/release.yml").read_text()
    require("python3 scripts/rust-toolchain.py check" in release,
            "release must check its preinstalled toolchain")
    require("image: ${{ vars.LABELLO_RELEASE_BUILD_IMAGE }}" in release
            and '"$BUILD_IMAGE" =~ ^[^[:space:]]+@sha256:[0-9a-f]{64}$' in release,
            "release must require the configured immutable build image")
    for path, text in [("CI", ci), ("release", release)]:
        require(not re.search(r"rustup (?:default|override|toolchain|target|component)|RUSTUP_TOOLCHAIN|cargo \+|rustc \+", text),
                f"{path}: use the shared policy instead of a separate compiler selection")
    image = (ROOT / "deployment/release/Containerfile").read_text()
    require(re.search(r"^FROM docker.io/library/rust:" + re.escape(pin["channel"])
                      + r"-bookworm@sha256:[0-9a-f]{64}$", image, re.MULTILINE),
            "release image must pin the matching Rust Bookworm image digest")
    require("python3" in image and "rustup component add clippy rustfmt" in image
            and "rustup target add wasm32-unknown-unknown" in image,
            "release image must include audit and compiler prerequisites")
    verify = (ROOT / "scripts/verify.sh").read_text()
    for function in ("format_check", "clippy_check", "workspace_tests", "ui_tests",
                     "doctests", "inspector_check", "wasm_check", "browser_build"):
        require(f"{function}() {{\n    toolchain_check\n" in verify,
                f"{function}: canonical compiler checks must reject toolchain overrides")
    require('run python3 scripts/rust-toolchain.py check' in verify,
            "canonical verification must check the actual compiler")
    require('run python3 scripts/rust-toolchain.py audit' in verify,
            "canonical audit must check toolchain drift")
    print(f"Toolchain policy audit passed: Rust {pin['channel']}, {len(manifests)} manifests")


def check():
    pin = policy()
    for directory in (ROOT, ROOT / "apps/egui-mcp-inspector"):
        for tool in ("rustc", "cargo"):
            version = subprocess.check_output([tool, "--version"], cwd=directory, text=True).split()
            require(version[1] == pin["channel"], f"{tool}: expected {pin['channel']} in {directory.relative_to(ROOT)}")
        components = subprocess.check_output(["rustup", "component", "list", "--installed"], cwd=directory, text=True)
        for component in pin["components"]:
            require(any(line.startswith(component + "-") for line in components.splitlines()),
                    f"missing toolchain component: {component}")
        targets = subprocess.check_output(["rustup", "target", "list", "--installed"], cwd=directory, text=True).splitlines()
        require(all(target in targets for target in pin["targets"]), "missing WASM target")
    print(f"Both workspaces use Rust {pin['channel']} with the required components and WASM target")


def install():
    pin = policy()
    command = ["rustup", "toolchain", "install", pin["channel"], "--profile", pin["profile"], "--no-self-update"]
    for component in pin["components"]:
        command.extend(["--component", component])
    for target in pin["targets"]:
        command.extend(["--target", target])
    subprocess.run(command, cwd=ROOT, check=True)
    check()


if __name__ == "__main__":
    try:
        require(len(sys.argv) == 2 and sys.argv[1] in {"audit", "check", "install"},
                "usage: python3 scripts/rust-toolchain.py <audit|check|install>")
        {"audit": audit, "check": check, "install": install}[sys.argv[1]]()
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
        print(f"toolchain policy: {error}", file=sys.stderr)
        sys.exit(1)
