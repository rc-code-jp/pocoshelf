# minishelf

A Rust TUI file explorer with Git-aware coloring and text preview.

## Features

- Tree view (root-locked to startup directory)
- Text preview (UTF-8 only, with file size limit)
- Git state coloring (`modified`, `added`, `deleted`, `untracked`)
- Copy selected path as startup-root-relative path with `@` prefix
  - Example: `@docs/sample.txt`

## Usage

```bash
cargo run -- .
cargo run -- /path/to/project
```

Key bindings:

- `j`/`k` or `Down`/`Up`: move selection
- `h`/`Left`: collapse or move to parent
- `l`/`Right`/`Enter`: expand directory
- `r`: refresh git status
- `y`: copy `@`-relative path to clipboard
- `q`: quit

## User install (no Rust required)

Use GitHub Releases artifacts, or install via Homebrew tap that points to those artifacts.

## Maintainer release flow

1. Bump version in `Cargo.toml` if needed.
2. Create and push a tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

3. GitHub Actions builds binaries and creates release assets:
   - `minishelf-<version>-linux-x86_64.tar.gz`
   - `minishelf-<version>-macos-x86_64.tar.gz`
   - `minishelf-<version>-macos-aarch64.tar.gz`
   - `checksums.txt`

4. Update Homebrew formula checksums from `checksums.txt`.

## Homebrew tap formula template

Template is provided at `packaging/homebrew/minishelf.rb`.
Copy it to your tap repo (e.g. `homebrew-minishelf/Formula/minishelf.rb`) and replace:

- `__VERSION__`
- `__SHA256_MACOS_ARM64__`
- `__SHA256_MACOS_X86_64__`
- `__SHA256_LINUX_X86_64__`
