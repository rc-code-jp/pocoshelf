# AGENTS.md

## Purpose
This document defines how AI coding agents should work in this repository.
Goal: keep changes safe, reviewable, and aligned with the current product direction.

## Project Summary
- Name: `minishelf`
- Type: Rust TUI application
- Core features:
  - File tree view (startup-root locked)
  - Text preview (UTF-8 only, max size limit)
  - Git-aware coloring (`modified`, `added`, `deleted`, `untracked`)
  - Copy selected path as startup-root-relative path with `@` prefix (example: `@docs/sample.txt`)
- Runtime targets: macOS and Linux
- Distribution: GitHub Releases artifacts + Homebrew tap formula

## Source of Truth
When instructions conflict, follow this order:
1. User request in current chat
2. This `AGENTS.md`
3. `README.md`
4. Existing code behavior in `src/`

## Repository Map
- `src/main.rs`: app entrypoint, terminal lifecycle, event loop
- `src/app.rs`: app state, command handling, copy path logic
- `src/ui.rs`: layout/rendering (vertical split: tree top 20%, preview bottom 80%)
- `src/tree.rs`: tree model, navigation, root-boundary behavior
- `src/preview.rs`: preview loading and guards (size, UTF-8, binary detection)
- `src/git_status.rs`: git status collection and folder state aggregation
- `src/input.rs`: key bindings
- `.github/workflows/release.yml`: release automation
- `packaging/homebrew/minishelf.rb`: Homebrew formula template

## Agent Working Rules

### 1) Scope control
- Implement only what is requested.
- Do not introduce unrelated refactors.
- Preserve current UX and key bindings unless explicitly asked.

### 2) Safety constraints
- Never allow navigation above startup root.
- Keep `@`-relative path format stable.
- Keep preview rules stable unless requirement changes:
  - UTF-8 text only
  - reject binary content
  - enforce max preview size

### 3) Coding standards
- Prefer small, composable functions.
- Keep module boundaries clear (UI vs state vs filesystem/git).
- Avoid adding heavy dependencies unless justified.
- Use explicit error messages for user-facing failures.

### 4) Performance expectations
- Avoid expensive full-tree recomputation on every key press.
- Keep git refresh bounded (manual + periodic strategy).
- Keep rendering simple and deterministic.

### 5) Testing policy
For behavior changes, add or update tests near touched modules.
Minimum checks before merge:
- `cargo test`
- `cargo fmt --check` (if formatting changes were introduced)
- `cargo clippy --all-targets --all-features -D warnings` (when feasible)

## Change Checklist (for agents)
Before finalizing a change, verify:
1. Requirement is fully implemented.
2. No behavior regressions in tree navigation and preview.
3. `@`-relative copy still works from startup root.
4. Git coloring still covers file and directory aggregation.
5. Tests pass locally or explain why they could not run.
6. Docs updated if behavior or operations changed.

## Release Notes for Agents
If a change affects packaging/distribution, update both:
- `README.md` (maintainer/user flow)
- `packaging/homebrew/minishelf.rb` template placeholders or instructions

If release process changes, also update:
- `.github/workflows/release.yml`

## Non-goals (unless explicitly requested)
- Windows support
- Non-UTF-8 preview decoding
- Config system and runtime key remapping
- Feature expansion unrelated to file tree/preview/git visibility

## Communication Style for Agent PRs
- Describe user-visible changes first.
- Then list technical changes by file.
- Call out risks, tradeoffs, and follow-up items explicitly.
