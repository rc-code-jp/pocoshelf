# Release Guide

このドキュメントは `pocoshelf` のメンテナー向けリリース手順です。

## 配布方針

- 一般ユーザー向け導線は Nix flake を優先する
- flake package は `aarch64-darwin` / `x86_64-darwin` / `aarch64-linux` / `x86_64-linux` を対象にする
- GitHub Releases には macOS Apple Silicon (`aarch64-apple-darwin`) の tar.gz と `checksums.txt` を添付する
- 最小構成では署名 / notarization なしで始められる
- 必要になったら Developer ID 署名 + notarization を workflow で有効化できる

## Prerequisites

- `main` ブランチが最新であること
- ワークツリーが clean であること
- ローカルで次の確認が通ること
  - `cargo fmt --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo test`
  - `nix flake check`
  - `nix build .#pocoshelf`

## リリース手順

### 1. リリース版を commit して push

`Cargo.toml` の `version` を次の版へ更新します。`Cargo.lock` もルート package の version 更新が入るため、一緒に commit してください。

例:

```bash
git commit -am "Release <version>"
git push origin main
```

### 2. リリースタグを作成して push

```bash
git tag v<version>
git push origin v<version>
```

`push` されたタグで `.github/workflows/release.yml` が起動し、以下を公開します。

- `pocoshelf-<version>-aarch64-apple-darwin.tar.gz`
- `checksums.txt`

### 3. 手動実行する場合

GitHub Actions の `release` workflow を手動起動する場合は、`tag` 入力に `vX.Y.Z` 形式の既存タグを指定してください。

- `tag` は必須
- `v` で始まらない値は失敗

### 4. リリース結果を確認する

GitHub Release 公開後に次を確認してください。

- Release tag と `Cargo.toml` の version が一致している
- `pocoshelf-<version>-aarch64-apple-darwin.tar.gz` が添付されている
- `checksums.txt` に release asset の sha256 が含まれている

Nix flake 経由の利用者は、各自の設定リポジトリで `pocoshelf` input を更新して新しい版へ追従します。

```bash
nix flake lock --update-input pocoshelf
```

## 推奨構成: Developer ID 署名 + notarization

最小構成のままでも配布はできますが、GitHub Releases から直接入れる macOS ユーザー向けには署名 + notarization を推奨します。

### Apple Developer 側の前提条件

- Apple Developer Program に加入していること
- `Developer ID Application` 証明書を発行済みであること
- App Store Connect API Key を発行済みであること
- Team ID を把握していること

### GitHub Secrets / Variables

`release.yml` のオプション機能を有効化するには、次を設定します。

- Repository Variable: `APPLE_CODESIGN_ENABLED=true`
- Repository Secret: `APPLE_CERTIFICATE_P12_BASE64`
- Repository Secret: `APPLE_CERTIFICATE_PASSWORD`
- Repository Secret: `APPLE_SIGN_IDENTITY`
- Repository Secret: `APPLE_TEAM_ID`
- Repository Secret: `APPLE_API_KEY_ID`
- Repository Secret: `APPLE_API_ISSUER_ID`
- Repository Secret: `APPLE_API_PRIVATE_KEY_BASE64`

### workflow 内での実行位置

`release.yml` では次の順で実行します。

1. `cargo build --release --locked --target aarch64-apple-darwin`
2. 証明書 import
3. `codesign`
4. `xcrun notarytool submit --wait`
5. `xcrun stapler staple`
6. `tar -czf` で最終成果物を作成
7. GitHub Release へ添付

## Troubleshooting

- `Invalid tag: ...`
  - `vX.Y.Z` 形式のタグか確認する
- `checksums.txt` に asset がない
  - Release asset 名と `checksums.txt` の内容が一致しているか確認する
- `codesign` / `notarytool` が失敗する
  - `APPLE_CODESIGN_ENABLED=true` のときだけ走るので、Secrets と証明書の内容を確認する
