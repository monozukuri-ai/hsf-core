"""Prepare an hsf-core release: version, lock file, install snippets and changelog.

Run from the repository root with Python 3.11 or later:

    python3 scripts/bump_version.py 0.1.0-alpha.8 --check
    python3 scripts/bump_version.py 0.1.0-alpha.8 --contract 39

The script sets the package version in Cargo.toml and Cargo.lock, updates the
dependency snippets in README.md and docs/getting-started.md, and turns the
changelog heading "## Unreleased" into "## <version> (contract <N>)". --contract
also sets CONTRACT_VERSION in src/lib.rs; otherwise the current contract is kept.
Running it again changes nothing. --check then runs the CI checks and
`cargo publish --dry-run`. The script never commits or publishes.
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import tomllib
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
INDEX = "https://index.crates.io/hs/f-/hsf-core"
USER_AGENT = "hsf-core-release-script (github.com/monozukuri-ai/hsf-core)"
SNIPPETS = ("README.md", "docs/getting-started.md")
SEMVER = re.compile(
    r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
)
CONTRACT = re.compile(r"pub const CONTRACT_VERSION: u32 = (\d+);")


def semver_key(version: str) -> tuple:
    """Sort key with SemVer precedence: 0.1.0-alpha.9 < 0.1.0-alpha.10 < 0.1.0."""
    match = SEMVER.fullmatch(version)
    if not match:
        raise SystemExit(f"Not a semantic version: {version}")
    core = tuple(int(part) for part in match.groups()[:3])
    if match[4] is None:
        return (*core, 1, ())
    identifiers = tuple(
        (0, int(part), "") if part.isdigit() else (1, 0, part)
        for part in match[4].split(".")
    )
    return (*core, 0, identifiers)


def published_versions() -> set[str]:
    request = urllib.request.Request(INDEX, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=30) as response:
        lines = response.read().decode().splitlines()
    return {json.loads(line)["vers"] for line in lines if line}


def set_package_version(text: str, version: str) -> str:
    """Replace ``version`` in the ``[package]`` table only."""
    lines = text.splitlines(keepends=True)
    section = None
    for index, line in enumerate(lines):
        header = re.fullmatch(r"\[([^\]]+)\]\s*", line)
        if header:
            section = header[1]
        elif section == "package" and re.fullmatch(r'version = "[^"]*"\s*', line):
            lines[index] = f'version = "{version}"\n'
            edited = "".join(lines)
            if tomllib.loads(edited)["package"]["version"] != version:
                raise SystemExit("Cargo.toml edit did not produce the version")
            return edited
    raise SystemExit("Cargo.toml has no [package] version line")


def set_changelog_heading(text: str, version: str, contract: int) -> str:
    heading = f"## {version} (contract {contract})"
    released = re.compile(rf"^## {re.escape(version)}(?: \(contract \d+\))?$", re.M)
    if released.search(text):
        return released.sub(heading, text, count=1)
    unreleased = re.compile(r"^## Unreleased.*$", re.M)
    if not unreleased.search(text):
        raise SystemExit(
            'CHANGELOG.md needs a "## Unreleased" section describing the release'
        )
    return unreleased.sub(heading, text, count=1)


def cargo_command() -> list[str]:
    """Cargo from the toolchain the CI workflow pins, when rustup has it."""
    workflow = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
    match = re.search(r"dtolnay/rust-toolchain@([0-9][0-9.]*)", workflow)
    if match and shutil.which("rustup"):
        toolchains = subprocess.run(
            ["rustup", "toolchain", "list"], capture_output=True, text=True
        ).stdout
        if any(line.startswith(match[1] + "-") for line in toolchains.splitlines()):
            return ["cargo", "+" + match[1]]
        print(f"note: Rust {match[1]} (used by CI) is not installed; using cargo")
    return ["cargo"]


def run(command: list[str]) -> None:
    print("$", " ".join(command), flush=True)
    subprocess.run(command, cwd=ROOT, check=True)


def write_if_changed(path: Path, text: str) -> None:
    if path.read_text(encoding="utf-8") != text:
        path.write_text(text, encoding="utf-8")
        print(f"updated {path.relative_to(ROOT).as_posix()}")


def main() -> None:
    parser = argparse.ArgumentParser(
        description=(__doc__ or "").split("\n", 1)[0],
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=(__doc__ or "").split("\n", 2)[-1],
    )
    parser.add_argument("version", help="new version, such as 0.1.0-alpha.8")
    parser.add_argument("--contract", type=int, help="new CONTRACT_VERSION")
    parser.add_argument(
        "--check", action="store_true", help="run the CI checks and a publish dry run"
    )
    args = parser.parse_args()
    version = args.version

    manifest = ROOT / "Cargo.toml"
    current = tomllib.loads(manifest.read_text(encoding="utf-8"))["package"]["version"]
    if semver_key(version) < semver_key(current):
        raise SystemExit(f"{version} is older than the current version {current}")
    try:
        if version in published_versions():
            raise SystemExit(f"hsf-core {version} is already on crates.io")
    except OSError as error:
        print(f"warning: crates.io index unavailable ({error}); not checked")

    library = ROOT / "src/lib.rs"
    source = library.read_text(encoding="utf-8")
    found = CONTRACT.search(source)
    if not found:
        raise SystemExit("src/lib.rs has no CONTRACT_VERSION")
    contract = args.contract if args.contract is not None else int(found[1])
    changelog = ROOT / "CHANGELOG.md"
    # Check every edit before writing any file.
    log = set_changelog_heading(
        changelog.read_text(encoding="utf-8"), version, contract
    )
    edited = set_package_version(manifest.read_text(encoding="utf-8"), version)

    write_if_changed(manifest, edited)
    write_if_changed(
        library, CONTRACT.sub(f"pub const CONTRACT_VERSION: u32 = {contract};", source)
    )
    write_if_changed(changelog, log)
    for name in SNIPPETS:
        path = ROOT / name
        text, count = re.subn(
            r'hsf-core = "[^"]+"',
            f'hsf-core = "{version}"',
            path.read_text(encoding="utf-8"),
        )
        if count == 0:
            print(f"warning: no hsf-core dependency snippet in {name}")
        write_if_changed(path, text)
    cargo = cargo_command()
    run([*cargo, "update", "--workspace"])

    print(f"\nhsf-core {current} -> {version} (contract {found[1]} -> {contract})")
    if args.check:
        try:
            run([*cargo, "fmt", "--check"])
            run([*cargo, "clippy", "--all-targets", "--locked", "--", "-D", "warnings"])
            run([*cargo, "test", "--locked"])
            run([*cargo, "publish", "--dry-run", "--locked", "--allow-dirty"])
        except subprocess.CalledProcessError as error:
            raise SystemExit(f"Check failed: {' '.join(error.cmd)}") from None
        print("\nAll checks passed.")
    else:
        print(f"\nRun the checks: python3 scripts/bump_version.py {version} --check")
    print(
        "\nThen commit and publish:\n"
        "  git add -A\n"
        f'  git commit -m "{version}"\n'
        "  git push origin HEAD\n"
        "  cargo publish --locked\n"
        "Afterwards, edrawings-kit follows it with its own scripts/bump_version.py."
    )


if __name__ == "__main__":
    main()
