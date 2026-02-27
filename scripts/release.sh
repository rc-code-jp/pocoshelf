#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 <version>" >&2
  echo "Example: $0 0.1.15" >&2
}

if [[ $# -ne 1 ]]; then
  usage
  exit 1
fi

version="$1"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Invalid version: $version (expected X.Y.Z)" >&2
  exit 1
fi

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "This script must be run inside a git repository." >&2
  exit 1
fi

branch="$(git branch --show-current)"
if [[ "$branch" != "main" ]]; then
  echo "Current branch must be main (current: ${branch:-detached})." >&2
  exit 1
fi

if [[ -n "$(git status --porcelain)" ]]; then
  echo "Working tree must be clean before release." >&2
  exit 1
fi

if ! git remote get-url origin >/dev/null 2>&1; then
  echo "Remote 'origin' is not configured." >&2
  exit 1
fi

tag="v$version"
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null 2>&1; then
  echo "Tag already exists locally: $tag" >&2
  exit 1
fi

if git ls-remote --exit-code --tags origin "refs/tags/$tag" >/dev/null 2>&1; then
  echo "Tag already exists on origin: $tag" >&2
  exit 1
fi

echo "Running release checks..."
cargo fmt --check
cargo clippy --all-targets --all-features -D warnings
cargo test

python3 - "$version" <<'PY'
from pathlib import Path
import re
import sys

new_version = sys.argv[1]
cargo_toml = Path("Cargo.toml")
text = cargo_toml.read_text()

in_package = False
replaced = 0
old_version = None
lines = text.splitlines(keepends=True)

for i, line in enumerate(lines):
    stripped = line.strip()
    if stripped == "[package]":
        in_package = True
        continue
    if in_package and stripped.startswith("[") and stripped != "[package]":
        in_package = False
    if in_package:
        m = re.match(r'^(version = )"([0-9]+\.[0-9]+\.[0-9]+)"\s*$', line)
        if m:
            old_version = m.group(2)
            lines[i] = f'{m.group(1)}"{new_version}"\n'
            replaced += 1
            in_package = False

if replaced != 1:
    raise SystemExit("Failed to update [package] version in Cargo.toml")

if old_version == new_version:
    raise SystemExit(f"Cargo.toml is already at version {new_version}")

cargo_toml.write_text("".join(lines))
PY

git add Cargo.toml Cargo.lock
git commit -m "chore(release): $tag"
git tag "$tag"
git push origin main
git push origin "$tag"

echo "Release prepared successfully."
echo "- Version: $version"
echo "- Tag: $tag"
echo "GitHub Actions release workflow should start from the pushed tag."
