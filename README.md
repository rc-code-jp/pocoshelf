# minishelf

`minishelf` は、**AI エージェント（Claude Code / Codex など）の“横”に置いて、リポジトリ内のファイルを手早く確認する**ための Rust 製 TUI ツールです。  
ターミナル上で、ファイルツリーとテキストプレビューを同時に扱えます。

エージェントに修正を任せつつ、開発者が手元で「今どこを見ているか」「何が変わっているか」をサクッと確認する用途に向いています。

---

## できること

- 起動ディレクトリをルートに固定したファイルツリー表示
- UTF-8 テキストのプレビュー（サイズ上限あり）
- Git の変更状況を色でわかりやすく表示
  - `modified`
  - `added`
  - `deleted`
  - `untracked`
- 選択中パスを `@` 付きのルート相対パスでコピー
  - 例: `@docs/sample.txt`
  - 
---

## インストール

### 1) Homebrew（推奨）

```bash
brew tap rc-code-jp/minishelf https://github.com/rc-code-jp/minishelf
brew install minishelf
```

または 1 行で:

```bash
brew install rc-code-jp/minishelf/minishelf
```

### 2) リリースバイナリを直接使う

GitHub Releases からご利用環境向けアーカイブを取得し、展開して `minishelf` バイナリを PATH の通った場所へ配置してください。

現在の配布ターゲット:
- macOS Apple Silicon (`aarch64`)
- Linux x86_64

> 注: Intel macOS 向けバイナリは現在未提供です。

### 3) ソースから実行（Rust 開発者向け）

```bash
cargo run -- .
cargo run -- /path/to/project
```

---

## 使い方（クイックスタート）

```bash
minishelf .
```

- 画面上部: ファイルツリー
- 画面下部: 選択ファイルのプレビュー

まずは `j` / `k` で移動し、`Enter` で開き、`y` でパスコピー、`q` で終了という流れが基本です。

---

## キーバインド

- `j` / `k` または `Down` / `Up`
  - ツリー選択を移動（ツリーフォーカス時）
  - プレビューをスクロール（プレビューフォーカス時）
- `h` / `Left` / `Esc`
  - ツリーへフォーカスを戻す
  - ツリーフォーカス時はディレクトリを閉じる / 親へ移動
- `l` / `Right` / `Enter`
  - ファイルをプレビューとして開く（プレビューフォーカスへ）
  - ディレクトリを展開
- `Ctrl+u` / `PageUp`: プレビュー上スクロール
- `Ctrl+d` / `PageDown`: プレビュー下スクロール
- `r`: Git 状態を更新
- `y`: `@` 付きルート相対パスをクリップボードへコピー
- `q`: 終了

---

## 動作仕様（知っておくと便利）

- ツリーは**起動時のルートより上には移動しません**。
- プレビュー対象は **UTF-8 テキストのみ**です。
- バイナリ判定されたファイルはプレビューしません。
- サイズ上限を超えるファイルはプレビューしません。

---

## よくある使い方

- リポジトリの「どこが変わったか」を見たい
- ドキュメントや設定ファイルをターミナルだけで素早く読みたい
- チャットや Issue に貼るため、`@path/to/file` 形式でパスをコピーしたい
- AI エージェントの修正後に、対象ファイルを順に目視チェックしたい

---

## メンテナー向け: リリース手順

1. 必要に応じて `Cargo.toml` のバージョンを更新
2. タグを作成して push

```bash
git tag v0.1.0
git push origin v0.1.0
```

3. GitHub Actions がリリース成果物を作成
   - `minishelf-<version>-linux-x86_64.tar.gz`
   - `minishelf-<version>-macos-aarch64.tar.gz`
   - `checksums.txt`
4. 同ワークフローで Homebrew formula も更新

### Homebrew formula テンプレート

`packaging/homebrew/minishelf.rb` を利用し、以下プレースホルダーを置換してください。

- `__VERSION__`
- `__SHA256_MACOS_ARM64__`
- `__SHA256_LINUX_X86_64__`
