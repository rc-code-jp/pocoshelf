# minishelf

`minishelf` は、**AI エージェント（Claude Code / Codex など）の“横”に置いて、リポジトリ内のファイルを手早く確認する**ための Rust 製 TUI ツールです。  
ターミナル上で、ファイルツリーとテキストプレビューを同時に扱えます。

エージェントに修正を任せつつ、開発者が手元で「今どこを見ているか」「何が変わっているか」をサクッと確認する用途に向いています。

---

<img width="1512" height="879" alt="minishelf.png" src="https://github.com/user-attachments/assets/e4cf38c2-cd0d-454a-844e-3599499de2ae" />

---

## できること

- 起動ディレクトリをルートに固定したファイルツリー表示
- UTF-8 テキストのプレビュー（サイズ上限あり）
  - 行番号付き表示
- プレビューモード切り替え（`p`）
  - `raw <-> diff`（差分がある場合）
- Git の変更状況を色でわかりやすく表示
  - `modified`
  - `added`
  - `deleted`
  - `untracked`
- 選択中パスを `@` 付きのルート相対パスでコピー
  - 例: `@docs/sample.txt`
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
各リリースには `checksums.txt` と署名ファイル（`checksums.txt.sig` / `checksums.txt.pem`）を含めています。検証方法は [`docs/release.md`](docs/release.md) を参照してください。

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

まずは `j` / `k` で移動し、`Enter` で開き、`c` でパスコピー、`q` / `Esc` で終了という流れが基本です。

---

## キーバインド

- `j` / `k` または `Down` / `Up`
  - ツリー選択を移動（ツリーフォーカス時）
  - プレビューをスクロール（プレビューフォーカス時）
- `h` / `Left`
  - ツリーへフォーカスを戻す
  - ツリーフォーカス時はディレクトリを閉じる / 親へ移動
- `l` / `Right` / `Enter`
  - ファイルをプレビューとして開く（プレビューフォーカスへ）
  - ディレクトリを展開
- `Ctrl+u` / `PageUp`: プレビュー上スクロール
- `Ctrl+d` / `PageDown`: プレビュー下スクロール
- `p`: プレビューモード切り替え
- `n` / `N`: `diff` モードで次/前の変更箇所へジャンプ
- `r`: Git 状態を更新
- `c`: `@` 付きルート相対パスをクリップボードへコピー
- `v`: 選択中ファイルを `vi` で開く（ディレクトリ選択時は無視）
- `o`: 選択中の場所を Finder / ファイルマネージャで開く（ファイル選択時は親フォルダ）
- `?` / `F1`: ヘルプ（全キーマップ）を表示/非表示
- `q` / `Esc`: 終了

---

## 動作仕様（知っておくと便利）

- ツリーは**起動時のルートより上には移動しません**。
- プレビュー対象は **UTF-8 テキストのみ**です。
- バイナリ判定されたファイルはプレビューしません。
- サイズ上限を超えるファイルはプレビューしません。
- `diff` モードは、ファイル全体を表示しつつ変更行を強調表示します。
- `diff` モードは、選択中ファイルに Git 差分がある場合のみ有効です。

---

## 設定 (Configuration)

`~/.config/minishelf/config.toml` (OSの標準的な設定ディレクトリ、または環境変数 `XDG_CONFIG_HOME` に準拠) に設定ファイルを作成することで、UI の動作をカスタマイズできます。
ファイルが存在しない場合はデフォルトの比率が使用されます。

```toml
[layout]
# 通常時の上部ツリーパネルの割合 (%)
tree_ratio_normal = 50
# プレビューパネルにフォーカスが当たった時の上部ツリーパネルの割合 (%)
tree_ratio_preview_focused = 10
```

---

## よくある使い方

- リポジトリの「どこが変わったか」を見たい
- ドキュメントや設定ファイルをターミナルだけで素早く読みたい
- チャットや Issue に貼るため、`@path/to/file` 形式でパスをコピーしたい
- AI エージェントの修正後に、対象ファイルを順に目視チェックしたい

---

## メンテナー向け情報

リリース手順は [`docs/release.md`](docs/release.md) を参照してください。

### Homebrew formula テンプレート

`packaging/homebrew/minishelf.rb` を利用し、以下プレースホルダーを置換してください。

- `__VERSION__`
- `__SHA256_MACOS_ARM64__`
- `__SHA256_LINUX_X86_64__`
