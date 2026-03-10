# Release Guide

このドキュメントは `minishelf` のメンテナー向けリリース手順です。

## 配布方針

- 配布対象は macOS Apple Silicon (`aarch64-apple-darwin`) のみ
- 一般ユーザー向け導線は Homebrew tap を優先
- 配布元は GitHub Releases
- `rc-code-jp/homebrew-tap` は release workflow から自動更新する
- 最初は署名 / notarization なしで始められる
- 必要になったら Developer ID 署名 + notarization を workflow に追加できる

## Prerequisites

- `main` ブランチが最新であること
- ワークツリーが clean であること
- ローカルで次の確認が通ること
  - `cargo fmt --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo test`
- `rc-code-jp/homebrew-tap` リポジトリが作成済みであること
- `minishelf` リポジトリに共用 GitHub App `homebrew-sync` 用の `APP_ID` variable と `APP_PRIVATE_KEY` secret が設定済みであること

## 最小構成でのリリース手順

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

- `minishelf-<version>-aarch64-apple-darwin.tar.gz`
- `checksums.txt`

### 3. 手動実行する場合（workflow_dispatch）

GitHub Actions の `release` workflow を手動起動する場合は、`tag` 入力に `vX.Y.Z` 形式の既存タグを指定してください。

- `tag` は必須
- `v` で始まらない値は失敗

### 4. workflow の自動更新結果を確認する

workflow は GitHub Release 公開後に `rc-code-jp/homebrew-tap` の `Formula/minishelf.rb` を自動更新します。

Actions summary で次を確認してください。

- `version`
- `url`
- `sha256`
- `rc-code-jp/homebrew-tap` の更新結果

`url` は次の形式です。

```text
https://github.com/rc-code-jp/minishelf/releases/download/v<version>/minishelf-<version>-aarch64-apple-darwin.tar.gz
```

### 5. 自動更新が失敗した場合だけ `rc-code-jp/homebrew-tap` を手動更新する

`Formula/minishelf.rb` の最低限の更新箇所は次の 2 つです。

```ruby
version "<version>"
sha256 "<checksums.txt の値>"
```

`url` は `#{version}` を参照するテンプレートにしておけば、通常は変更不要です。

更新後に commit / push します。

```bash
git add Formula/minishelf.rb
git commit -m "minishelf <version>"
git push origin main
```

これでユーザーは `brew upgrade minishelf` で更新できます。

GitHub App の設定方法は [`docs/github-app-homebrew-tap.md`](github-app-homebrew-tap.md) を参照してください。

## 推奨構成: Developer ID 署名 + notarization

最小構成のままでも配布はできますが、一般ユーザー向け体験を優先するなら署名 + notarization を推奨します。

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
- `Missing checksum for minishelf-...`
  - Release asset 名と `checksums.txt` の内容が一致しているか確認する
- `codesign` / `notarytool` が失敗する
  - `APPLE_CODESIGN_ENABLED=true` のときだけ走るので、Secrets と証明書の内容を確認する
