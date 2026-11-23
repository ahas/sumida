[English](README.md) | [한국어](README.ko.md) | **日本語** | [简体中文](README.zh-CN.md)

# SUMIDA

**S**calable **U**tility **M**odules **I**ntegration for **D**ynamic **A**pplication

Axum で構築された包括的な Rust Web アプリケーションフレームワークです。堅牢な認証・認可とモダンな Web API 機能を備え、スケーラブルなマルチテナントアプリケーション向けに設計されています。

## ✨ 機能

### 🏗️ コアアーキテクチャ
- **モジュール化されたワークスペース**: 5 つのライブラリクレートを、ルートの `sumida` クレートがまとめて再エクスポートします
- **レイヤード設計**: Controller → Service → Repository パターン
- **型安全**: SeaORM とあわせて Rust の型システムを全面的に活用します
- **本番対応**: ローカル開発と AWS クラウドへのデプロイの両方に対応します

### 👥 ユーザー管理システム
- **マルチテナント組織**: 組織管理をひととおり備えています
- **関係ベースのアクセス制御 (ReBAC)**: コンパイル済みルールセット、組織階層のクロージャ、起動時のスキーマ検証を備えた Zanzibar 方式のリレーションタプル
- **ユーザーライフサイクル**: 登録、確認、招待、ブロックのワークフロー
- **アクティビティ追跡**: 詳細なユーザーログと監査証跡

### 💬 掲示板システム
- **ディスカッションフォーラム**: 掲示板と投稿の管理をひととおり備えています
- **階層構造**: Categories → Boards → Posts → Comments
- **豊富なインタラクション**: いいね/よくないね、閲覧数の追跡、ファイル添付
- **コンテンツ管理**: 下書き/公開のワークフロー、論理削除に対応

### 💳 メンバーシップと課金
- **プラン**: サブスクリプションプランとクレジットプラン (個人または組織単位)、無料トライアル、論理削除
- **クレジット台帳**: 有効期限付きの FIFO ロット、予約 (オーソリ後に売上確定)、冪等な付与/消費、監査
- **ライフサイクル**: 自動更新、督促 (dunning) と猶予期間、再有効化、日割り計算を伴う期間途中のプラン変更
- **利用権ゲート**: 期間ごとのクォータを持つサブスクリプション/クレジットのハイブリッドゲート (`entitlement_guard!` レイヤー)
- **決済**: 差し替え可能な PG 抽象化 (PortOne V2 を第一にサポート)、チェックアウト時のクーポンと税、Standard-Webhooks
- **スケジューラ**: 期限切れ、更新、スイープ、通知を扱うバックグラウンドメンテナンス
- 必要なポリシーのシード、設定、スケジューラのデプロイについては [`docs/membership.md`](docs/membership.md) を参照してください

### 🔐 認証とセキュリティ
- **複数の認証方式**: セッションベース認証と JWT トークン認証
- **API キー管理**: サービス間認証
- **安全なセッション**: Redis または AWS DynamoDB のセッションストア
- **パスワードの保護**: ソルトを適切に扱う Argon2 ハッシュ

### ☁️ クラウドとストレージ
- **柔軟なデプロイ**: ローカルの Redis とファイルストレージ、または AWS DynamoDB と S3
- **画像処理**: 組み込みの画像処理と最適化
- **メール連携**: SMTP による通知とユーザー確認
- **設定管理**: `SUMIDA_*` 環境変数で上書きできる TOML 設定、AWS SSM に対応
- **モニタリング**: 管理者専用のダッシュボード監視 API。`/dashboard/monitoring` でスナップショットと SSE ストリーミングにより
  サーバーの状態をリアルタイムに提供します

## 🚀 クイックスタート

### 前提条件

- **Rust**: 最新の stable (2024 エディション)
- **PostgreSQL**: 12 以上
- **Redis**: 6 以上 (ローカル開発用) または AWS DynamoDB (本番用)

### インストール

1. **リポジトリをクローンする**
```bash
git clone <repository-url>
cd sumida
```

2. **設定を用意する**
```bash
cp settings.example.toml settings.toml
# Edit settings.toml; SUMIDA_* environment variables override it
```

3. **データベースを準備する**
```bash
# Run migrations
cargo run --bin migration
```

4. **デプロイプロファイルを選ぶ**

#### 🏠 ローカル開発
```bash
cargo run -p basic
```

#### ☁️ AWS 本番環境
```bash
cargo run -p basic --no-default-features --features full-aws
```

## 🏗️ アーキテクチャ

### ワークスペース構成
```
sumida/
├── api/           # REST API controllers and business logic
├── entity/        # Database entities (SeaORM models)
├── infra/         # Infrastructure (DB, AWS, sessions, settings)
├── error/         # Error handling utilities
├── e2e/          # End-to-end testing framework
└── examples/     # Usage examples and demos
```

### ワークスペースの依存関係グラフ

```mermaid
graph LR
  sumida[(sumida root crate)]
  api[[api]]
  infra[[infra]]
  entity[[entity]]
  error[[error]]
  e2e[[e2e]]

  sumida --> api
  sumida --> infra
  sumida --> entity
  sumida --> error
  sumida --> e2e

  api --> infra
  api --> entity
  api --> error

  infra --> entity
  infra --> error

  entity --> error

  e2e --> api
  e2e --> infra
  e2e --> entity
  e2e --> error
```

### クレートごとのモジュールマップ

#### ルート `sumida`
```mermaid
graph TD
  sumida_root[(sumida)]
  sumida_root --> api_crate[[api]]
  sumida_root --> infra_crate[[infra]]
  sumida_root --> entity_crate[[entity]]
  sumida_root --> error_crate[[error]]
  sumida_root --> e2e_crate[[e2e]]
```

#### `api` クレートのリクエストフロー

<details>
<summary>ユーザー</summary>

```mermaid
graph TD
  u_get["GET /users<br/>(read)"] --> u_ctrl_list["UserController::list"]
  u_ctrl_list --> u_srv_list["UserService::list"]
  u_srv_list --> u_logic_list{{"SeaORM select<br/>+ pagination"}}
  u_logic_list --> u_ret_list[["List<UserListItem>"]]

  u_post["POST /users<br/>(create)"] --> u_ctrl_create["UserController::create"]
  u_ctrl_create --> u_srv_create["UserService::create"]
  u_srv_create --> u_logic_create{{"hash_password +<br/>Users::insert"}}
  u_logic_create --> u_ret_create[["UserCreated (id)"]]

  u_patch["PATCH /users/{id}<br/>(update)"] --> u_ctrl_update["UserController::update"]
  u_ctrl_update --> u_srv_update["UserService::update"]
  u_srv_update --> u_logic_update{{"txn + try_access +<br/>optional hash + update"}}
  u_logic_update --> u_ret_update[["UserUpdated"]]

  u_delete["DELETE /users/{id}<br/>(delete)"] --> u_ctrl_delete["UserController::delete"]
  u_ctrl_delete --> u_srv_delete["UserService::delete"]
  u_srv_delete --> u_logic_delete{{"txn + try_access +<br/>Users::delete_by_id"}}
  u_logic_delete --> u_ret_delete[["UserDeleted"]]
```

</details>

<details>
<summary>ロール</summary>

```mermaid
graph TD
  r_get["GET /roles<br/>(read)"] --> r_ctrl_list["RoleController::list"]
  r_ctrl_list --> r_srv_list["RoleService::list"]
  r_srv_list --> r_logic_list{{"filter + pagination"}}
  r_logic_list --> r_ret_list[["List<RoleListItem>"]]

  r_post["POST /roles<br/>(create)"] --> r_ctrl_create["RoleController::create"]
  r_ctrl_create --> r_srv_create["RoleService::create"]
  r_srv_create --> r_logic_create{{"txn + insert role +<br/>assign access policies"}}
  r_logic_create --> r_ret_create[["RoleCreated (id)"]]

  r_patch["PATCH /roles/{id}<br/>(update)"] --> r_ctrl_update["RoleController::update"]
  r_ctrl_update --> r_srv_update["RoleService::update"]
  r_srv_update --> r_logic_update{{"txn + try_access +<br/>update policies + save"}}
  r_logic_update --> r_ret_update[["RoleUpdated"]]

  r_delete["DELETE /roles/{id}<br/>(delete)"] --> r_ctrl_delete["RoleController::delete"]
  r_ctrl_delete --> r_srv_delete["RoleService::delete"]
  r_srv_delete --> r_logic_delete{{"txn + try_access +<br/>clear policies + delete"}}
  r_logic_delete --> r_ret_delete[["RoleDeleted"]]
```

</details>

<details>
<summary>組織</summary>

```mermaid
graph TD
  o_get["GET /organizations<br/>(read)"] --> o_ctrl_list["OrganizationController::list"]
  o_ctrl_list --> o_srv_list["OrganizationService::list"]
  o_srv_list --> o_logic_list{{"filter + order_by + pagination"}}
  o_logic_list --> o_ret_list[["List<OrganizationListItem>"]]

  o_post["POST /organizations<br/>(create)"] --> o_ctrl_create["OrganizationController::create"]
  o_ctrl_create --> o_srv_create["OrganizationService::create"]
  o_srv_create --> o_logic_create{{"Organizations::insert"}}
  o_logic_create --> o_ret_create[["OrganizationCreated (id)"]]

  o_patch["PATCH /organizations/{id}<br/>(update)"] --> o_ctrl_update["OrganizationController::update"]
  o_ctrl_update --> o_srv_update["OrganizationService::update"]
  o_srv_update --> o_logic_update{{"txn + try_access +<br/>update model"}}
  o_logic_update --> o_ret_update[["OrganizationUpdated"]]

  o_delete["DELETE /organizations/{id}<br/>(delete)"] --> o_ctrl_delete["OrganizationController::delete"]
  o_ctrl_delete --> o_srv_delete["OrganizationService::delete"]
  o_srv_delete --> o_logic_delete{{"txn + try_access +<br/>Organizations::delete_by_id"}}
  o_logic_delete --> o_ret_delete[["OrganizationDeleted"]]
```

</details>

<details>
<summary>リレーションタプル (認可)</summary>

認可はリクエストごとにポリシー行を読むのではなく、`relation_tuples` を対象にコンパイル済みのルールセット
(`api/src/authz/`) で評価します。sumida はルールファイル `schema.sz` をひとつ持ち、利用側は `Sumida::new(RULES)`
で独自のルールを登録します。ルールセット全体は起動時に実際のデータベースと照合して検証されます。存在しない列を読む
ルールがあれば、リクエスト時に誤った答えを返すのではなく、サーバーが起動を拒否します。

```mermaid
graph TD
  rt_get["GET /relation-tuples<br/>(read)"] --> rt_ctrl_list["RelationTupleController::list"]
  rt_ctrl_list --> rt_srv_list["RelationTupleService::list"]
  rt_srv_list --> rt_logic_list{{"Authz::filter + pagination"}}
  rt_logic_list --> rt_ret_list[["List&lt;RelationTupleListItem&gt;"]]

  rt_post["POST /relation-tuples<br/>(create, upsert)"] --> rt_ctrl_create["RelationTupleController::create"]
  rt_ctrl_create --> rt_srv_create["RelationTupleService::create"]
  rt_srv_create --> rt_logic_create{{"Authz::write —<br/>requires &lt;type&gt;#grant"}}
  rt_logic_create --> rt_ret_create[["RelationTupleCreated (id)"]]

  rt_delete["DELETE /relation-tuples/{id}"] --> rt_ctrl_delete["RelationTupleController::delete"]
  rt_ctrl_delete --> rt_srv_delete["RelationTupleService::delete"]
  rt_srv_delete --> rt_logic_delete{{"Authz::revoke —<br/>requires &lt;type&gt;#grant"}}
  rt_logic_delete --> rt_ret_delete[["RelationTupleDeleted"]]

  schema["GET /authz/schema"] --> schema_ctrl["AuthzController::schema"]
  schema_ctrl --> schema_logic{{"compiled vocabulary —<br/>no grant, no table names"}}

  me["GET /me/permissions"] --> me_ctrl["MeController::permissions"]
  me_ctrl --> me_logic{{"Authz::effective —<br/>evaluated, not read raw"}}
```

`effect` フィールドはありません。拒否は行ではなくルールの構文 (`- blocked`) で表します。`PATCH` もありません。
タプルの一意制約により、`POST` がそのまま upsert になります。

タプルの書き込み自体も認可の対象です。呼び出し側は対象オブジェクトに対して `<resource_type>#grant` を持っている
必要があるため、権限を付与する資格のない者がその権限を付与することはできません。

</details>

<details>
<summary>ファイルストレージ</summary>

```mermaid
graph TD
  f_get["GET /files<br/>(read)"] --> f_ctrl_list["FileController::list"]
  f_ctrl_list --> f_srv_list["FileService::list"]
  f_srv_list --> f_logic_list{{"filter + pagination + partial model"}}
  f_logic_list --> f_ret_list[["List<FileListItem>"]]

  f_post["POST /files<br/>(create/upload)"] --> f_ctrl_upload["FileController::upload"]
  f_ctrl_upload --> f_srv_upload["FileService::upload"]
  f_srv_upload --> f_logic_upload{{"insert metadata +<br/>storage.store + status update"}}
  f_logic_upload --> f_ret_upload[["FileUploaded (id)"]]

  f_download["GET /files/{id}/download"] --> f_ctrl_download["FileController::download"]
  f_ctrl_download --> f_srv_download["FileService::download"]
  f_srv_download --> f_logic_download{{"storage.download"}}
  f_logic_download --> f_ret_download[["Response<Body>"]]

  f_delete["DELETE /files/{id}<br/>(delete)"] --> f_ctrl_delete["FileController::delete"]
  f_ctrl_delete --> f_srv_delete["FileService::delete"]
  f_srv_delete --> f_logic_delete{{"txn + fetch file +<br/>optional storage.delete + delete row"}}
  f_logic_delete --> f_ret_delete[["FileDeleted"]]
```

</details>

<details>
<summary>掲示板システム</summary>

```mermaid
graph TD
  bg_get["GET /board-groups<br/>(read)"] --> bg_ctrl_list["BoardGroupController::list"]
  bg_ctrl_list --> bg_srv_list["BoardGroupService::list"]
  bg_srv_list --> bg_logic_list{{"filter + paginate<br/>BoardGroups::find"}}
  bg_logic_list --> bg_ret_list[["List<BoardGroupListItem>"]]

  bg_post["POST /board-groups<br/>(create)"] --> bg_ctrl_create["BoardGroupController::create"]
  bg_ctrl_create --> bg_srv_create["BoardGroupService::create"]
  bg_srv_create --> bg_logic_create{{"BoardGroups::insert"}}
  bg_logic_create --> bg_ret_create[["BoardGroupCreated (id)"]]

  bg_patch["PATCH /board-groups/{id}<br/>(update)"] --> bg_ctrl_update["BoardGroupController::update"]
  bg_ctrl_update --> bg_srv_update["BoardGroupService::update"]
  bg_srv_update --> bg_logic_update{{"txn + try_access +<br/>BoardGroups::update"}}
  bg_logic_update --> bg_ret_update[["BoardGroupUpdated"]]

  bg_delete["DELETE /board-groups/{id}<br/>(delete)"] --> bg_ctrl_delete["BoardGroupController::delete"]
  bg_ctrl_delete --> bg_srv_delete["BoardGroupService::delete"]
  bg_srv_delete --> bg_logic_delete{{"txn + try_access +<br/>BoardGroups::delete_by_id"}}
  bg_logic_delete --> bg_ret_delete[["BoardGroupDeleted"]]
```

```mermaid
graph TD
  b_get["GET /boards<br/>(read)"] --> b_ctrl_list["BoardController::list"]
  b_ctrl_list --> b_srv_list["BoardService::list"]
  b_srv_list --> b_logic_list{{"txn + filter +<br/>paginate Boards"}}
  b_logic_list --> b_ret_list[["List<BoardListItem>"]]

  b_post["POST /boards<br/>(create)"] --> b_ctrl_create["BoardController::create"]
  b_ctrl_create --> b_srv_create["BoardService::create"]
  b_srv_create --> b_logic_create{{"Boards::insert"}}
  b_logic_create --> b_ret_create[["BoardCreated (id)"]]

  b_patch["PATCH /boards/{id}<br/>(update)"] --> b_ctrl_update["BoardController::update"]
  b_ctrl_update --> b_srv_update["BoardService::update"]
  b_srv_update --> b_logic_update{{"txn + try_access +<br/>Boards::update"}}
  b_logic_update --> b_ret_update[["BoardUpdated"]]

  b_delete["DELETE /boards/{id}<br/>(delete)"] --> b_ctrl_delete["BoardController::delete"]
  b_ctrl_delete --> b_srv_delete["BoardService::delete"]
  b_srv_delete --> b_logic_delete{{"txn + try_access +<br/>Boards::delete_by_id"}}
  b_logic_delete --> b_ret_delete[["BoardDeleted"]]
```

```mermaid
graph TD
  bt_get["GET /board-topics<br/>(read)"] --> bt_ctrl_list["BoardTopicController::list"]
  bt_ctrl_list --> bt_srv_list["BoardTopicService::list"]
  bt_srv_list --> bt_logic_list{{"filter + paginate<br/>BoardTopics::find"}}
  bt_logic_list --> bt_ret_list[["List<BoardTopicListItem>"]]

  bt_post["POST /board-topics<br/>(create)"] --> bt_ctrl_create["BoardTopicController::create"]
  bt_ctrl_create --> bt_srv_create["BoardTopicService::create"]
  bt_srv_create --> bt_logic_create{{"BoardTopics::insert"}}
  bt_logic_create --> bt_ret_create[["BoardTopicCreated (id)"]]

  bt_patch["PATCH /board-topics/{id}<br/>(update)"] --> bt_ctrl_update["BoardTopicController::update"]
  bt_ctrl_update --> bt_srv_update["BoardTopicService::update"]
  bt_srv_update --> bt_logic_update{{"txn + try_access +<br/>BoardTopics::update"}}
  bt_logic_update --> bt_ret_update[["BoardTopicUpdated"]]

  bt_delete["DELETE /board-topics/{id}<br/>(delete)"] --> bt_ctrl_delete["BoardTopicController::delete"]
  bt_ctrl_delete --> bt_srv_delete["BoardTopicService::delete"]
  bt_srv_delete --> bt_logic_delete{{"txn + try_access +<br/>BoardTopics::delete_by_id"}}
  bt_logic_delete --> bt_ret_delete[["BoardTopicDeleted"]]
```

```mermaid
graph TD
  bp_get["GET /board-posts<br/>(read)"] --> bp_ctrl_list["BoardPostController::list"]
  bp_ctrl_list --> bp_srv_list["BoardPostService::list"]
  bp_srv_list --> bp_logic_list{{"conditional columns +<br/>FTS/snippet + pagination"}}
  bp_logic_list --> bp_ret_list[["List<BoardPostListItem>"]]

  bp_get_one["GET /board-posts/{id}"] --> bp_ctrl_get["BoardPostController::get"]
  bp_ctrl_get --> bp_srv_get["BoardPostService::get"]
  bp_srv_get --> bp_logic_get{{"txn + load content +<br/>attached files"}}
  bp_logic_get --> bp_ret_get[["GetBoardPost"]]

  bp_post["POST /board-posts<br/>(create)"] --> bp_ctrl_create["BoardPostController::create"]
  bp_ctrl_create --> bp_srv_create["BoardPostService::create"]
  bp_srv_create --> bp_logic_create{{"ap::chain checks +<br/>hash password + link files"}}
  bp_logic_create --> bp_ret_create[["BoardPostCreated (id)"]]

  bp_draft["POST /board-posts/draft"] --> bp_ctrl_draft["BoardPostController::create_draft"]
  bp_ctrl_draft --> bp_srv_draft["BoardPostService::create_draft"]
  bp_srv_draft --> bp_logic_draft{{"create draft record +<br/>optional password hash"}}
  bp_logic_draft --> bp_ret_draft[["BoardPostCreated (draft id)"]]

  bp_patch["PATCH /board-posts/{id}<br/>(update)"] --> bp_ctrl_update["BoardPostController::update"]
  bp_ctrl_update --> bp_srv_update["BoardPostService::update"]
  bp_srv_update --> bp_logic_update{{"try_access +<br/>BoardPosts::update + link files"}}
  bp_logic_update --> bp_ret_update[["BoardPostUpdated"]]

  bp_delete["DELETE /board-posts/{id}<br/>(delete)"] --> bp_ctrl_delete["BoardPostController::delete"]
  bp_ctrl_delete --> bp_srv_delete["BoardPostService::delete"]
  bp_srv_delete --> bp_logic_delete{{"try_access + cascade cleanup"}}
  bp_logic_delete --> bp_ret_delete[["BoardPostDeleted"]]
```

```mermaid
graph TD
  bf_get["GET /board-files<br/>(read)"] --> bf_ctrl_list["BoardFileController::list"]
  bf_ctrl_list --> bf_srv_list["BoardFileService::list"]
  bf_srv_list --> bf_logic_list{{"join Files + filter + paginate"}}
  bf_logic_list --> bf_ret_list[["List<BoardFileListItem>"]]

  bf_get_one["GET /board-files/{post_id}/{file_id}"] --> bf_ctrl_get["BoardFileController::get"]
  bf_ctrl_get --> bf_srv_get["BoardFileService::get"]
  bf_srv_get --> bf_logic_get{{"join Files + select metadata"}}
  bf_logic_get --> bf_ret_get[["GetBoardFile"]]

  bf_post["POST /board-files<br/>(upload)"] --> bf_ctrl_upload["BoardFileController::upload"]
  bf_ctrl_upload --> bf_srv_upload["BoardFileService::upload"]
  bf_srv_upload --> bf_logic_upload{{"board guard + size checks +<br/>delegate to FileService::upload"}}
  bf_logic_upload --> bf_ret_upload[["FileUploaded (id)"]]

  bf_delete["DELETE /board-files<br/>(delete)"] --> bf_ctrl_delete["BoardFileController::delete"]
  bf_ctrl_delete --> bf_srv_delete["BoardFileService::delete"]
  bf_srv_delete --> bf_logic_delete{{"BoardFiles::delete_many +<br/>optional FileService::delete"}}
  bf_logic_delete --> bf_ret_delete[["BoardFileDeleted"]]
```

</details>

### フィーチャーフラグ

| フィーチャー | 説明 |
|---------|-------------|
| `full-local` | ローカル開発用のフルスタック (Redis + ローカルストレージ) |
| `full-aws` | 本番用の AWS スタック (DynamoDB + S3 + SSM) |
| `test-suite` | テスト用の構成 |

## 📖 使い方

### 基本的な組み込み

`examples/basic` が完全なサーバーです。その中核は次のとおりです。

```rust
use std::net::SocketAddr;

use axum::{Extension, Router, routing};
use sea_orm::DatabaseConnection;
use sumida::api::{Sumida, SumidaState};

#[derive(Clone)]
pub struct AppState {
    db: DatabaseConnection,
}

impl SumidaState for AppState {
    fn db(&self) -> &DatabaseConnection {
        &self.db
    }
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Initialize infrastructure
    let settings = sumida::infra::settings::get();
    sumida::infra::tracing::subscribe();
    let state = AppState { db: sumida::infra::db::connect().await };

    let (messenger_layer, messenger) = sumida::infra::messenger::new_layer();
    let session_store = sumida::infra::session::new_store(&settings.session).await?;

    // Build application; `&[]` declares no rule files beyond sumida's own
    let app = Router::new()
        .merge(Sumida::new(&[]).set_state(&state).router())
        .layer(messenger_layer)
        .layer(sumida::api::layer::session::new(session_store).await?)
        .layer(sumida::api::layer::cors::new())
        .layer(Extension(messenger))
        .layer(Extension(settings))
        .with_state(state)
        .route("/", routing::get(health_check))
        .layer(settings.session.ip_source.clone().into_extension())
        .into_make_service_with_connect_info::<SocketAddr>();

    // Start server
    let listener = tokio::net::TcpListener::bind(&settings.server.host).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_check() -> axum::http::StatusCode {
    axum::http::StatusCode::OK
}
```

### API エンドポイント

このフレームワークは幅広い REST API を提供します。

- **認証**: `/auth/*` - ログイン、ログアウト、登録
- **ユーザー**: `/users/*` - ユーザー管理とプロフィール
- **組織**: `/organizations/*` - マルチテナントの組織管理
- **ロール**: `/roles/*` - ロールと権限の管理
- **掲示板**: `/boards/*` - ディスカッションフォーラムの管理
- **投稿**: `/board-posts/*` - コンテンツの作成と管理
- **API キー**: `/api-keys/*` - サービス認証

## 🧪 テスト

### テストの実行
```bash
# Integration tests, one test binary at a time (each one resets the test database)
bash scripts/test/suite.sh

# Unit tests
cargo test

# End-to-end tests
cargo test --package e2e

# With specific features
cargo test --features test-suite
```

### テストカバレッジ
```bash
# Install cargo-tarpaulin for coverage
cargo install cargo-tarpaulin

# Generate coverage report
cargo tarpaulin --out Html
```

## 🔧 設定

### 設定ファイル

設定は Figment で読み込みます。`settings.toml` (パスは `SUMIDA_SETTINGS`、既定値は `./settings.toml`) をベースに、
`SUMIDA_*` 環境変数がそれを上書きします。変数名の `__` はネストを、単独の `_` は `-` を表すため、
`SUMIDA_DATABASE__URL` は `database.url` を設定します。すべてのセクションは `settings.example.toml` に載っています。

#### ローカル開発
```toml
[server]
host = "0.0.0.0:7997"
stage = "dev"

[database]
url = "postgres://user:password@localhost:5432/sumida"

[session.redis]
url = "redis://localhost:6379/0"

[jwt]
secret = "REPLACE_WITH_JWT_SECRET"  # at least 32 bytes

[smtp]
server = "smtp.example.com"
```

#### AWS 本番環境
```toml
[database]
url = "postgres://user:password@rds-endpoint:5432/sumida"

[session.dynamodb]
table = "sumida-sessions"

[storage.s3]
bucket = "sumida-storage"

[jwt]
ssm_path = "/sumida/prod/jwt-secret"  # default: /sumida/{stage}/jwt-secret
```

### フィーチャーの構成

Cargo のフィーチャーでビルドをカスタマイズします。

```toml
[dependencies]
sumida = { path = ".", features = ["full-aws", "image"] }
```

## 📊 パフォーマンスとモニタリング

### 組み込みのオブザーバビリティ
- **構造化ログ**: レベルを設定できる Tracing 連携
- **ヘルスチェック**: ロードバランサー向けの基本的なヘルスエンドポイント
- **エラー追跡**: コンテキストを伴う包括的なエラー処理

### 推奨モニタリングスタック
- **メトリクス**: Prometheus + Grafana
- **ログ**: ELK Stack または AWS CloudWatch
- **トレーシング**: Jaeger または AWS X-Ray
- **稼働監視**: 独自のヘルスチェック監視

## 🚀 デプロイ

### ローカル開発
```bash
# Start PostgreSQL and Redis first, then apply the schema
cargo run --bin migration

# Run application
cargo run -p basic
```

### Docker でのデプロイ
```dockerfile
FROM rust:1-bookworm AS builder
# .cargo/config.toml links with mold.
RUN apt-get update && apt-get install -y mold
WORKDIR /app
COPY . .
RUN cargo build --release -p basic --no-default-features --features full-aws

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates
COPY --from=builder /app/target/release/app /usr/local/bin/app
CMD ["app"]
```

### AWS へのデプロイ
推奨する AWS サービスは次のとおりです。
- **コンピューティング**: ECS Fargate または EKS
- **データベース**: RDS PostgreSQL
- **セッション**: DynamoDB
- **ストレージ**: S3
- **シークレット**: AWS Systems Manager Parameter Store

## 🤝 コントリビュート

### 開発環境のセットアップ
1. Rust ツールチェーンと必要な依存関係をインストールします
2. ローカルの PostgreSQL と Redis インスタンスを用意します
3. `settings.example.toml` を `settings.toml` にコピーして設定します
4. データベースのマイグレーションを実行します: `cargo run --bin migration`
5. 開発サーバーを起動します: `cargo run -p basic`

### コードスタイル
- Rust 2024 エディションの慣習に従います
- `cargo +nightly fmt` でフォーマットします。`rustfmt.toml` は nightly 専用のオプションを使っています
- リントには `cargo clippy` を実行します
- テストが通ることを確認します: `cargo test`

### プルリクエストのガイドライン
- 内容がわかるコミットメッセージを書きます
- 新機能にはテストを含めます
- API を変更したらドキュメントも更新します
- CI/CD パイプラインが通ることを確認します

## 📝 ライセンス

このプロジェクトは MIT ライセンスのもとで公開されています。詳しくは [LICENSE](LICENSE) ファイルを参照してください。

## 🔗 関連プロジェクト

- [SeaORM](https://github.com/SeaQL/sea-orm) - データベース ORM
- [Axum](https://github.com/tokio-rs/axum) - Web フレームワーク
- [Tower](https://github.com/tower-rs/tower) - ミドルウェアのエコシステム

---

**技術指標**: PostgreSQL + SeaORM • Axum 0.8.4 • 本番対応のアーキテクチャ

ポリシー、設定、ルール言語の詳細は、[`docs/`](docs/) 以下のドキュメントと [CLAUDE.md](CLAUDE.md) を参照してください。
