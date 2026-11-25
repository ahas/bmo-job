[English](README.md) | [한국어](README.ko.md) | **日本語** | [简体中文](README.zh-CN.md)

# bmo-job

Redis がミドルウェアであるのと同じ意味で、ミドルウェアとして動く ToDo です。アイテムとそのノート、レポート、作業
指示書、アイテムが待つキュー、アイテムのリース、質問とその返答、サブスクリプションを、一つの API で保存・提供します。
このサービス自体は何も実行せず、他のユニットを呼び出すこともありません。作業は二つの経路で並行して担当者に届きます。
一つはプッシュで、担当者が登録したサブスクリプションのイベントが Webhook で配信されるか、ストリームとして読まれて
MCP 通知に変換されます。もう一つはプルで、リースによって取得します。コーディングハーネスは `bmo-job` プラグイン
（MCP サーバー `bmo-job mcp` とそのスキル）を最初の入口として作業を受け取ります。

## 構成

| フォルダ | 内容 |
|---|---|
| `src/` | サービス自身のパッケージ：ストア、HTTP サーバーとイベントストリーム、Webhook 配信、設定、そして `src/bin/` 配下のバイナリ `bmo-job` |
| `crates/` | `client`（クレート `bmo-job-client`）：Rust で表した契約。すべてのボディを型として、すべてのルートを呼び出しとして持ち、イベントストリーム、Webhook の署名、それらの型から生成される OpenAPI ドキュメントを含む。`plugin`（クレート `bmo-job-plugin`）：ハーネスプラグイン、クライアント上の MCP サーバーとそのツール、ツールを含む OpenAPI ドキュメント全体、`skills/` 配下の `bmo-job` スキル。`tool-call`：ツールホストの呼び出しプロトコルと、ツールが共有するサービスへのアクセス |
| `package/` | 下流のチェックアウト向けにパックされる TypeScript パッケージ `bmo-job-client`：`docs/openapi.json` から生成した型とその上のクライアント |
| `deploy/` | デプロイツリーから `bmo-job serve` を実行するサービスのユーザーユニット（docs/deploy.md） |
| `tools/` | ツールホスト向けのサービスのツール `todo`、`state`、`launch`。それぞれマニフェスト、実行ファイル、スキーマからなる（docs/tools.md） |
| `tests/` | ルートパッケージのテスト：リポジトリのチェック、契約のチェック、実サーバーを通したすべてのルート、そしてユニットやハーネスが実行するとおりのバイナリ |
| `docs/` | サービスが何をするかの正規の記述と、コードが生成する契約 `openapi.json` |

ルートには Cargo ワークスペースのマニフェスト、`testbed.toml`（このリポジトリをどうテスト・チェック・
ビルド・デプロイするかを示す bmo-testbed の宣言、docs/deploy.md）、`install.toml`（新しいマシンへのインストールに
必要なもので、`bmo install` が読みます）、`rustfmt.toml`、`CLAUDE.md`、デプロイが用意する
設定ファイルの例 `settings.example.toml`、そして `LICENSE` があります。

## ドキュメント

| ドキュメント | 内容 |
|---|---|
| `docs/api.md` | API：すべてのリソース、その状態と遷移、ルート、イベント、サブスクリプションと Webhook 配信、クライアントとハーネスプラグイン |
| `docs/openapi.json` | コードが生成する契約。ツールは `x-tools`、ハーネスの入口は `x-harness` の下にある |
| `docs/tools.md` | サービスが配置するツール：キューの入口、チャットエージェントの状態の読み取りと起動の入口 |
| `docs/cli.md` | コマンドライン：`nouns`、`serve`、`mcp`、`job close`、`job ask`、`todo`、`openapi` |
| `docs/settings.md` | 設定ファイル：サービスの待ち受け先、トークンとそれが指す呼び出し元、アイテムが自動で開くラベル、データベース |
| `docs/schema.md` | スキーマ：テーブルとそれが守るルール |
| `docs/deploy.md` | サービスのデプロイ：リリースに含まれるもの、サービスの配置場所、デプロイを通すための条件 |
| `docs/base-libraries.md` | ジョブが import するベースライブラリに必要な部分がないときの道筋：プルリクエスト、守るべきルール、レビュー、バージョンの固定 |
| `docs/checks.md` | リポジトリのチェック |
| `docs/PERSONAL-DATA.md` | リポジトリに置いてはならない個人データ、その値を代わりに置く場所、許される値 |

## バージョン

- **バージョンはセット全体のものです。** ビルド時に git タグから読み取って `BMO_VERSION` として渡され、ヘルス、
  MCP サーバー、`--version` がそれを答えます。ワークスペース、OpenAPI ドキュメント、TypeScript パッケージの
  バージョンは動かないプレースホルダーです。
- **バージョンはオーナーが宣言したリリースでのみ上がります。** bmo-chat の `docs/conventions.md` の「Versions」に
  あるとおり、コミット、デプロイ、修正、契約の変更ではバージョンは動かず、デプロイはコミットで区別します。

## コマンド

```sh
cargo check --workspace --all-targets
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd package && bun install --frozen-lockfile && bun run check)
```

サーバーは `BMO_JOB_SETTINGS` が指す設定ファイル、なければ `settings.toml` を読み（docs/settings.md）、サービスを
開始します。MCP サーバーはアドレスを指定して稼働中のサービスに接続します。

```sh
cargo run --bin bmo-job -- serve
BMO_JOB_TOKEN=... cargo run --bin bmo-job -- mcp --service http://127.0.0.1:7841
```

契約を変更したら、ドキュメントと TypeScript の型を生成し直します。

```sh
cargo run --bin bmo-job -- openapi > docs/openapi.json
(cd package && bun run generate)
```

`package/` で `bun pm pack` を実行すると、下流のチェックアウトがインストールする tarball が作られます（docs/api.md、
"The client"）。

デプロイされたサービスはユーザーユニット `deploy/bmo-job.service` として、デプロイツリーから `bmo-job serve` で
動きます。bmo-testbed が `testbed.toml` に従ってコミットを本番に反映します（docs/deploy.md）。BMO のマシンでは、
エージェントがインデックスの INSTALL.md（`bmo install`、https://bmo.ahas.io/install.md）でサービスをインストールします。

## ライセンス

MIT。全文は `LICENSE` にあります。
