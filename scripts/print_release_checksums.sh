#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <release-assets-dir>" >&2
  exit 1
fi

assets_dir="$1"
cd "$assets_dir"

shasum -a 256 minishelf-*.tar.gz
