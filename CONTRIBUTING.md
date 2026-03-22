# Contributing to minishelf

まず、コントリビューションへの興味をありがとうございます。

minishelf は小さく・安全に・使いやすくあり続けることを目指しています。
変更を送る前に、このドキュメントをひと通り読んでください。

---

## 前提

- Rust ツールチェーン（`cargo`, `rustfmt`, `clippy` を含む）
  - バージョンは `mise.toml` または `rust-toolchain.toml` を参照
  - `mise install` で揃えることができます
- macOS または Linux の動作環境

---

## 開発の流れ

### 1. フォークして clone する

```bash
git clone https://github.com/<your-name>/minishelf.git
cd minishelf
```

### 2. ブランチを切る

```bash
git checkout -b feat/your-feature-name
# または
git checkout -b fix/issue-description
```

### 3. 変更を加える

- 小さく・目的が明確な単位でコミットしてください。
- モジュールの責務境界を守ってください（UI / state / filesystem+git は分離）。
- 重い依存クレートの追加は極力避け、追加する場合は PR で理由を説明してください。
- ユーザーに見えるエラーメッセージは具体的にしてください。

### 4. テストを確認する

```bash
cargo test
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

すべてグリーンであることを確認してから PR を出してください。

### 5. PR を作成する

PR の説明には以下を含めてください：

- **ユーザーに見える変化**（何が変わるか）
- **変更したファイルと理由**
- **リスク・トレードオフ・残課題**（あれば）

---

## スコープと非目標

このプロジェクトは intentionally small です。
以下は現時点での **非目標** です。受け入れられない可能性が高いので、まず Issue で相談してください。

- Windows サポート
- 設定ファイルによるキーリマップ
- ファイルツリー・Git 可視化と無関係な機能拡張

---

## Issue / バグ報告

- バグを報告する場合は、OS、ターミナルエミュレータ、再現手順を書いてください。
- 新機能の提案は、まず Issue でユースケースを共有してください。

---

## ライセンス

コントリビューションは [MIT License](./LICENSE) のもとで提供されたものとして扱われます。
