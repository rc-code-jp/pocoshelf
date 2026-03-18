# GitHub App For Homebrew Tap Automation

このドキュメントは、`homebrew-sync` という GitHub App を使って、`minishelf` など複数リポジトリの release workflow から Homebrew tap を自動更新するための手順をまとめたものです。

公開 OSS でも、この用途では `private GitHub App` で十分です。目的は各アプリの GitHub Actions から `homebrew-tap` リポジトリに commit / push することだけなので、不特定ユーザーに配布する必要はありません。

## 前提

- `rc-code-jp/minishelf` と `rc-code-jp/homebrew-tap` が存在していること
- GitHub 上で `rc-code-jp` の設定を変更できること
- `minishelf` を含む 1 つ以上の release workflow から `homebrew-tap` を更新したいこと

## 参考にする GitHub 公式ページ

- [Registering a GitHub App](https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/registering-a-github-app)
- [Making a GitHub App public or private](https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/making-a-github-app-public-or-private)
- [Choosing permissions for a GitHub App](https://docs.github.com/developers/apps/building-github-apps/setting-permissions-for-github-apps)
- [Managing private keys for GitHub Apps](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/managing-private-keys-for-github-apps)
- [Installing your own GitHub App](https://docs.github.com/apps/installing-github-apps)
- [Making authenticated API requests with a GitHub App in a GitHub Actions workflow](https://docs.github.com/en/enterprise-cloud%40latest/apps/creating-github-apps/authenticating-with-a-github-app/making-authenticated-api-requests-with-a-github-app-in-a-github-actions-workflow)
- [Use GITHUB_TOKEN for authentication in workflows](https://docs.github.com/en/actions/configuring-and-managing-workflows/authenticating-with-the-github_token)

## 1. `homebrew-sync` GitHub App を作成する

GitHub の右上プロフィールから次へ進みます。

`Settings` -> `Developer settings` -> `GitHub Apps` -> `New GitHub App`

入力例:

- `GitHub App name`: `homebrew-sync`
- `Description`: `Updates Homebrew tap repositories after releases`
- `Homepage URL`: `https://github.com/rc-code-jp/minishelf`

`Webhook` はこの用途では不要なので、`Active` を無効にして構いません。

`Where can this GitHub App be installed?` は `Only on this account` を選びます。

この App は一般配布しないので、`private GitHub App` で問題ありません。

## 2. 権限を最小で設定する

この用途で最初に必要なのは、tap リポジトリへ commit / push するための権限だけです。App を複数アプリで共有しても、必要な権限は同じです。

Repository permissions:

- `Contents`: `Read and write`

通常はこれで十分です。`Formula/minishelf.rb` を更新するだけなら、`Workflows` 権限は不要です。

## 3. App を作成する

ページ下部の `Create GitHub App` を押して作成します。

作成後、App の設定画面で `App ID` を控えてください。後で GitHub Actions の secret に使います。

## 4. Private key を生成する

App 設定画面の `Private keys` セクションで `Generate a private key` を押します。

`.pem` ファイルはその場でダウンロードされます。GitHub 上からあとで同じ内容を再表示できないので、安全な場所に保管してください。

## 5. App をインストールする

App 設定画面の `Install App` から `rc-code-jp` にインストールします。

Repository access は `Only select repositories` を選び、少なくとも次の 2 つを追加します。

- `rc-code-jp/minishelf`
- `rc-code-jp/homebrew-tap`

`minishelf` 側で workflow を実行し、その token で `homebrew-tap` を更新するため、この 2 リポジトリに入れておくのが扱いやすい構成です。将来ほかのアプリでも使う場合は、その release workflow を持つ各リポジトリも同じ App のインストール対象に追加します。

## 6. 各アプリのリポジトリに Actions secrets を追加する

まず `rc-code-jp/minishelf` の次の画面を開きます。

`Settings` -> `Secrets and variables` -> `Actions`

追加するもの:

- Repository secret: `APP_ID`
- Repository secret: `APP_PRIVATE_KEY`

値:

- `APP_ID`
  - GitHub App の設定画面に表示される `App ID`
- `APP_PRIVATE_KEY`
  - ダウンロードした `.pem` ファイルの全文
  - `-----BEGIN ...-----` と `-----END ...-----` を含めてそのまま貼る

`homebrew-sync` を他のアプリにも使う場合は、その各リポジトリにも同じ `APP_ID` と `APP_PRIVATE_KEY` を設定します。

## 7. Workflow で installation token を発行する

以前は `actions/create-github-app-token` を使う構成も一般的でしたが、JavaScript アクションの Node ランタイム移行タイミングで警告や追従待ちが発生することがあります。
このリポジトリでは依存を減らすため、workflow 内で GitHub App JWT を生成し、GitHub REST API から installation token を直接発行します。

例:

```yaml
- name: Generate GitHub App installation token
  id: app-token
  shell: bash
  env:
    APP_ID: ${{ secrets.APP_ID }}
    APP_PRIVATE_KEY: ${{ secrets.APP_PRIVATE_KEY }}
    APP_OWNER: rc-code-jp
    APP_REPOSITORY: homebrew-tap
  run: |
    set -euo pipefail

    app_jwt="$(
      APP_ID="$APP_ID" APP_PRIVATE_KEY="$APP_PRIVATE_KEY" ruby <<'RUBY'
      require "base64"
      require "json"
      require "openssl"

      def base64url(data)
        Base64.urlsafe_encode64(data, padding: false)
      end

      now = Time.now.to_i
      header = { alg: "RS256", typ: "JWT" }
      payload = { iat: now - 60, exp: now + 540, iss: ENV.fetch("APP_ID") }
      private_key = OpenSSL::PKey::RSA.new(ENV.fetch("APP_PRIVATE_KEY"))
      signing_input = [header, payload].map { |part| base64url(JSON.generate(part)) }.join(".")
      signature = private_key.sign(OpenSSL::Digest::SHA256.new, signing_input)
      puts [signing_input, base64url(signature)].join(".")
      RUBY
    )"

    installation_id="$(
      curl --fail-with-body --silent --show-error \
        --request GET \
        --url "https://api.github.com/repos/${APP_OWNER}/${APP_REPOSITORY}/installation" \
        --header "Accept: application/vnd.github+json" \
        --header "Authorization: Bearer ${app_jwt}" \
        --header "X-GitHub-Api-Version: 2022-11-28" |
      ruby -rjson -e 'puts JSON.parse($stdin.read).fetch("id")'
    )"

    token="$(
      curl --fail-with-body --silent --show-error \
        --request POST \
        --url "https://api.github.com/app/installations/${installation_id}/access_tokens" \
        --header "Accept: application/vnd.github+json" \
        --header "Authorization: Bearer ${app_jwt}" \
        --header "X-GitHub-Api-Version: 2022-11-28" \
        --header "Content-Type: application/json" \
        --data '{"repositories":["homebrew-tap"]}' |
      ruby -rjson -e 'puts JSON.parse($stdin.read).fetch("token")'
    )"

    echo "::add-mask::${token}"
    echo "token=${token}" >> "$GITHUB_OUTPUT"
```

この token を使って `homebrew-tap` を checkout します。

```yaml
- name: Checkout tap repo
  uses: actions/checkout@v4
  with:
    repository: rc-code-jp/homebrew-tap
    token: ${{ steps.app-token.outputs.token }}
    path: homebrew-tap
```

その後に `Formula/minishelf.rb` の `version` と `sha256` を更新して commit / push します。

## 8. なぜ `GITHUB_TOKEN` ではなく GitHub App なのか

`GITHUB_TOKEN` は基本的に workflow が動いている同じリポジトリ向けです。今回のように `minishelf` や他のアプリから別リポジトリの `homebrew-tap` へ安全に書き込みたい場合は、GitHub 公式の案内どおり GitHub App を使うのが自然です。

fine-grained PAT でも実現はできますが、長期運用する OSS では次の点で GitHub App のほうが適しています。

- リポジトリ単位で対象を絞りやすい
- 権限を細かく制限しやすい
- 個人アカウント依存を減らせる
- 将来メンテナーが増えても管理しやすい

## 9. 導入後にやること

GitHub App の設定が終わったら、`minishelf` 側で次を実装します。

- release workflow で GitHub App token を発行する
- `rc-code-jp/homebrew-tap` を checkout する
- `Formula/minishelf.rb` の `version` と `sha256` を更新する
- `git commit` して `git push` する

このリポジトリでは、GitHub App JWT + REST API を使った tap 自動更新ジョブを [`../.github/workflows/release.yml`](../.github/workflows/release.yml) に実装しています。

ほかのアプリでも `homebrew-sync` を使う場合は、それぞれの release workflow でも同じ token 発行と tap 更新処理を流用できます。
