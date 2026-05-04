# pocoshelf

`pocoshelf` は、**AI エージェント（Claude Code / Codex など）の“横”に置いて、リポジトリ内のファイルを手早く確認する**ための Rust 製 TUI ツールです。  
ターミナル上で、起動ルート固定のファイルツリーを軽快に確認できます。

エージェントに修正を任せつつ、開発者が手元で「今どこを見ているか」「何が変わっているか」をサクッと確認する用途に向いています。

https://github.com/user-attachments/assets/83a1a710-89cd-4e31-8601-7c8e6f3cdce4

## できること

- 起動ディレクトリをルートに固定したファイルツリー表示
  - 更新日付を一覧表示
- Git の変更状況を色でわかりやすく表示
  - `modified`
  - `added`
  - `deleted`
  - `untracked`
- 選択中パスを `@` 付きのルート相対パスでコピー
  - 例: `@docs/sample.txt`

---

## インストール

### 1) Nix flake（推奨）

一時的に実行する場合:

```bash
nix run github:rc-code-jp/pocoshelf -- .
```

ユーザープロファイルにインストールする場合:

```bash
nix profile install github:rc-code-jp/pocoshelf
```

`nix-darwin` / `home-manager` の flake input として管理する場合は、設定リポジトリ側の `flake.nix` に input を追加します。

```nix
inputs = {
  nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  pocoshelf = {
    url = "github:rc-code-jp/pocoshelf";
    inputs.nixpkgs.follows = "nixpkgs";
  };
};
```

`nix-darwin` の `environment.systemPackages` へ追加する例:

```nix
{ pkgs, inputs, ... }:

{
  environment.systemPackages = [
    inputs.pocoshelf.packages.${pkgs.system}.default
  ];
}
```

flake は `aarch64-darwin` / `x86_64-darwin` / `aarch64-linux` / `x86_64-linux` を対象にしています。

### 2) GitHub Releases から直接入れる

GitHub Releases から `pocoshelf-<version>-aarch64-apple-darwin.tar.gz` を取得し、展開した `pocoshelf` バイナリを PATH の通った場所へ配置してください。
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
pocoshelf .
```

- 画面全体: ファイルツリー

まずは `j` / `k` で移動し、`Enter` でファイルならパスコピー、ディレクトリなら開閉し、`q` / `Esc` / `Ctrl+c` で終了という流れが基本です。
必要に応じて `r` で Git 状態を手動更新できます。

---

## キーバインド

- `j` / `k` または `Down` / `Up`
  - ツリー選択を移動
- `h` / `Left`
  - ディレクトリを閉じる / 親へ移動
- `l` / `Right`
  - ディレクトリをその場で開閉する
- ファイルをダブルクリック
  - `@` 付きルート相対パスをクリップボードへコピー
- `Enter`
  - ファイル選択時は `@` 付きルート相対パスをクリップボードへコピー
  - ディレクトリ選択時はその場で開閉する
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
  - ファイル / ディレクトリ: 更新日付
  - 画面幅が狭い場合は更新日付を省略します。

---

## 設定 (Configuration)

`~/.config/pocoshelf/config.toml` (OSの標準的な設定ディレクトリ、または環境変数 `XDG_CONFIG_HOME` に準拠) に設定ファイルを作成することで、ヘルプの初期表示言語やコピー後フックを変更できます。

```toml
[help]
# ヘルプモーダルの初期表示言語: "en" または "ja"
language = "en"

[copy]
# コピー成功後に起動する実行ファイル
after_copy_hook = "/Users/you/bin/pocoshelf-after-copy"
```

ヘルプモーダルは起動時に `help.language` の値を使って表示されます。既定値は `en` です。
ヘルプ表示中に `t` キーで英語と日本語を切り替えできます。

`copy.after_copy_hook` を指定すると、`@` 付き相対パス、通常相対パス、`cat` / `vi` コマンド文字列のコピー成功後に、そのファイルを引数なしで起動します。TUI操作を止めないため、フックはバックグラウンドで終了待ちし、前回のフックがまだ実行中の間は追加で起動しません。
`#!/usr/bin/osascript` で始まるファイルも指定できます。直接実行するため、事前に実行権限を付けてください。

```bash
chmod +x /Users/you/bin/pocoshelf-after-copy
```

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
pocoshelf --tree-mode changed
pocoshelf --tree-mode normal ~/work/repo
```

- `--tree-mode normal`: 通常のツリー表示
- `--tree-mode changed`: Git 差分があるファイルと親ディレクトリだけを表示

---

## メンテナー向け情報

### リリース手順

1. `Cargo.toml` の `version` を次のリリース版に更新する
2. `Cargo.lock` にルート package の version 更新を反映する
3. `git commit` して `main` に push する
4. リリースタグを push する

```bash
git tag v<version>
git push origin v<version>
```

5. GitHub Actions の `release` workflow 完了後、GitHub Release の asset と `checksums.txt` を確認する
- `version`
- `pocoshelf-<version>-aarch64-apple-darwin.tar.gz`
- `checksums.txt`

Nix flake 経由の利用者は、各自の設定リポジトリで `pocoshelf` input を更新すると新しい版へ追従できます。

詳細は [`docs/release.md`](docs/release.md) を参照してください。
