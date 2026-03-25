# minishelf

`minishelf` は、**AI エージェント（Claude Code / Codex など）の“横”に置いて、リポジトリ内のファイルを手早く確認する**ための Rust 製 TUI ツールです。  
ターミナル上で、起動ルート固定のファイルツリーを軽快に確認できます。

エージェントに修正を任せつつ、開発者が手元で「今どこを見ているか」「何が変わっているか」をサクッと確認する用途に向いています。

https://github.com/user-attachments/assets/83a1a710-89cd-4e31-8601-7c8e6f3cdce4

## できること

- 起動ディレクトリをルートに固定したファイルツリー表示
  - ファイルサイズと更新日付を一覧表示
- Git の変更状況を色でわかりやすく表示
  - `modified`
  - `added`
  - `deleted`
  - `untracked`
- 選択中パスを `@` 付きのルート相対パスでコピー
  - 例: `@docs/sample.txt`

---

## インストール

### 1) Homebrew tap（推奨）

一般ユーザー向けの主導線です。Rust / Cargo は不要です。

```bash
brew tap rc-code-jp/tap
brew install minishelf
```

アップデート:

```bash
brew upgrade minishelf
```

1 行で入れる場合:

```bash
brew install rc-code-jp/tap/minishelf
```

現在の配布ターゲット:
- macOS Apple Silicon (`aarch64-apple-darwin`) のみ

### 2) GitHub Releases から直接入れる

GitHub Releases から `minishelf-<version>-aarch64-apple-darwin.tar.gz` を取得し、展開した `minishelf` バイナリを PATH の通った場所へ配置してください。
各リリースには `checksums.txt` を添付します。

- 最小構成では署名 / notarization なしで配布できます
- 推奨構成では Developer ID 署名 + notarization 済みアーカイブにできます

### 3) ソースから実行（Rust 開発者向け）

一般ユーザー向けの配布方法ではありません。

`mise` を使う場合は、リポジトリルートで以下を実行すると `mise.toml` に従って Rust ツールチェーンが入ります。

```bash
mise install
```

```bash
cargo run -- .
cargo run -- /path/to/project
```

---

## 使い方（クイックスタート）

```bash
minishelf .
```

- 画面全体: ファイルツリー

まずは `j` / `k` で移動し、`Enter` でディレクトリを開閉し、`c` でパスコピー、`q` / `Esc` / `Ctrl+c` で終了という流れが基本です。
必要に応じて `r` で Git 状態を手動更新できます。

---

## キーバインド

- `j` / `k` または `Down` / `Up`
  - ツリー選択を移動
- `h` / `Left`
  - ディレクトリを閉じる / 親へ移動
- `l` / `Right` / `Enter`
  - ディレクトリをその場で開閉する
- ファイルをダブルクリック
  - `@` 付きルート相対パスをクリップボードへコピー
- `r`: Git 状態を更新
- `c`: `@` 付きルート相対パスをクリップボードへコピー
- `v`: 選択中ファイルを `vi` で開く（ディレクトリ選択時は無視）
- `o`: 選択中の場所を Finder / ファイルマネージャで開く（ファイル選択時は親フォルダ）
- `?` / `F1`: ヘルプ（全キーマップ）を表示/非表示
- `q` / `Esc` / `Ctrl+c`: 終了

---

## 動作仕様（知っておくと便利）

- ツリーは**起動時のルートより上には移動しません**。
- ディレクトリはインライン展開され、`▶` が閉、`▼` が開を表します。
- 左クリックではファイルを選択し、ディレクトリならその場で開閉します。
- 右クリックではファイル・ディレクトリのどちらでも `@` 付きルート相対パスをコピーできます。
- ツリーは `normal` / `changed` モードを持ち、`changed` では変更ファイルとその親ディレクトリのみを表示します。
- deleted ファイルは Git 差分由来の疑似ノードとして表示します。
- deleted ファイル選択時は `@` 付き相対パスのコピーのみ可能で、`vi` / Finder 起動は行いません。
- ツリーの追加情報は軽量なメタデータだけを使います。
  - ファイル: サイズと更新日付
  - ディレクトリ: 更新日付、サイズ欄は `-`
  - 画面幅が狭い場合は更新日付、サイズの順で省略します。

---

## 設定 (Configuration)

`~/.config/minishelf/config.toml` (OSの標準的な設定ディレクトリ、または環境変数 `XDG_CONFIG_HOME` に準拠) に設定ファイルを作成することで、ヘルプの初期表示言語を変更できます。

```toml
[help]
# ヘルプモーダルの初期表示言語: "en" または "ja"
language = "en"
```

ヘルプモーダルは起動時に `help.language` の値を使って表示されます。既定値は `en` です。
ヘルプ表示中に `t` キーで英語と日本語を切り替えできます。

---

## よくある使い方

- リポジトリの「どこが変わったか」を見たい
- チャットや Issue に貼るため、`@path/to/file` 形式でパスをコピーしたい
- AI エージェントの修正後に、変更ファイルの位置関係を素早く把握したい

---

## キー操作

- `Tab`: ツリーモード切り替え (`normal` / `changed`)

---

## 起動オプション

```bash
minishelf --tree-mode changed
minishelf --tree-mode normal ~/work/repo
```

- `--tree-mode normal`: 通常のツリー表示
- `--tree-mode changed`: Git 差分があるファイルと親ディレクトリだけを表示

---

## メンテナー向け情報

### リリース手順

1. `Cargo.toml` の `version` を次のリリース版に更新する
2. `git commit` して `main` に push する
3. リリースタグを push する

```bash
git tag v<version>
git push origin v<version>
```

4. GitHub Actions の `release` workflow 完了後、Actions summary か GitHub Release の `checksums.txt` から次を確認する
- `version`
- `url`
- `sha256`
- `rc-code-jp/homebrew-tap` の自動更新結果

5. 自動更新が失敗した場合だけ `rc-code-jp/homebrew-tap` の `Formula/minishelf.rb` を手動更新して push する

ユーザーはその後 `brew upgrade minishelf` で更新できます。

詳細は [`docs/release.md`](docs/release.md) を参照してください。
Homebrew tap 更新を GitHub App で自動化する場合は [`docs/github-app-homebrew-tap.md`](docs/github-app-homebrew-tap.md) を参照してください。
このリポジトリの release workflow では、Node ランタイム依存のある JavaScript アクションを避けるため、GitHub App JWT と REST API で installation token を発行しています。

### Homebrew formula テンプレート

`packaging/homebrew/minishelf.rb` を `rc-code-jp/homebrew-tap` の `Formula/minishelf.rb` にコピーし、以下プレースホルダーを置換してください。

- `__VERSION__`
- `__SHA256_AARCH64_APPLE_DARWIN__`
