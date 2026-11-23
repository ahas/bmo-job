[English](README.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | **简体中文**

# bmo-job

一个以中间件形式运行的待办系统，就像 Redis 是中间件一样：条目及其备注、报告和工作单，条目排队等待的队列，条目上的
租约，问题及其回复，以及订阅，全部通过一个 API 存储和提供。该服务本身从不执行任何东西，也不调用任何其他单元。工作
通过两条并行的途径到达执行者：一是推送，执行者注册的订阅事件通过 webhook 投递，或作为流读取并转换为 MCP 通知；二是
拉取，通过租约获取。编码 harness 以 `bmo-job` 插件（MCP 服务器 `bmo-job mcp` 及其技能）作为首个入口来接收工作。

## 目录结构

| 文件夹 | 内容 |
|---|---|
| `src/` | 服务自身的包：存储、HTTP 服务器及其事件流、webhook 投递、设置，以及 `src/bin/` 下的二进制 `bmo-job` |
| `crates/` | `client`（crate `bmo-job-client`）：用 Rust 表达的契约，每个请求体都是一个类型，每个路由都是一个调用，并包含事件流、webhook 签名以及由这些类型生成的 OpenAPI 文档；`plugin`（crate `bmo-job-plugin`）：harness 插件、基于客户端的 MCP 服务器及其工具、包含工具的完整 OpenAPI 文档，以及 `skills/` 下的 `bmo-job` 技能；`tool-call`：工具宿主的调用协议，以及各工具共用的服务访问 |
| `package/` | 为下游检出打包的 TypeScript 包 `bmo-job-client`：由 `docs/openapi.json` 生成的类型及基于它们的客户端 |
| `deploy/` | 服务的用户单元，从部署树运行 `bmo-job serve`（docs/deploy.md） |
| `tools/` | 服务提供给工具宿主的工具 `todo`、`state` 和 `launch`，每个都由清单、可执行文件和模式组成（docs/tools.md） |
| `tests/` | 根包的测试：仓库检查、契约检查、通过真实服务器的每个路由，以及按单元和 harness 的运行方式运行的二进制 |
| `docs/` | 服务行为的规范说明，以及由代码生成的契约 `openapi.json` |

根目录下有 Cargo 工作区清单、版本文件、`testbed.toml`（bmo-testbed 关于本仓库如何测试、检查、构建和部署的声明，
docs/deploy.md）、`install.toml`（在新机器上安装时所需的内容，由 `bmo install` 读取）、`rustfmt.toml`、`CLAUDE.md`、部署时提供的设置文件示例 `settings.example.toml`，以及 `LICENSE`。

## 文档

| 文档 | 内容 |
|---|---|
| `docs/api.md` | API：所有资源、其状态与转换、路由、事件、订阅与 webhook 投递、客户端和 harness 插件 |
| `docs/openapi.json` | 由代码生成的契约，工具位于 `x-tools` 下，harness 入口位于 `x-harness` 下 |
| `docs/tools.md` | 服务放置的工具：队列的入口、聊天代理的状态读取及其启动入口 |
| `docs/cli.md` | 命令行：`nouns`、`serve`、`mcp`、`job close`、`job ask`、`todo` 和 `openapi` |
| `docs/settings.md` | 设置文件：服务监听的地址、令牌及其对应的调用方、条目会自动打开的标签，以及数据库 |
| `docs/schema.md` | 模式：各张表及其遵守的规则 |
| `docs/deploy.md` | 部署服务：一个发布版本包含什么、服务放在哪里，以及部署的准入条件 |
| `docs/base-libraries.md` | 当任务导入的基础库缺少某个部分时的处理流程：拉取请求、需遵守的规则、评审和版本固定 |
| `docs/checks.md` | 仓库检查 |
| `docs/PERSONAL-DATA.md` | 仓库不得包含的个人数据、这类值应放在何处，以及允许的值 |

## 版本

- **整个仓库只有一个版本，** 即工作区的版本。`VERSION` 保存它，工作区版本、OpenAPI 文档和 TypeScript 包的版本都
  与之相同。
- **版本只在所有者宣布的发布时变动。** 如 bmo-chat 的 `docs/conventions.md`「Versions」所述，提交、部署、修复或
  契约变更都不会改变版本，部署之间以提交来区分。
- **在首次发布之前，版本为带修订后缀的 `0.1.0`，** 即 `0.1.0-rev.N`：`0.1.0` 是基准，所有者每宣布一次发布，修订号
  加一。首次发布之前，主版本号、次版本号和补丁号都不变。

## 命令

```sh
cargo check --workspace --all-targets
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd package && bun install --frozen-lockfile && bun run check)
```

服务器读取 `BMO_JOB_SETTINGS` 指定的设置文件，未指定时读取 `settings.toml`（docs/settings.md），然后开始提供服务；
MCP 服务器通过地址连接到正在运行的服务：

```sh
cargo run --bin bmo-job -- serve
BMO_JOB_TOKEN=... cargo run --bin bmo-job -- mcp --service http://127.0.0.1:7841
```

修改契约后，重新生成文档和 TypeScript 类型：

```sh
cargo run --bin bmo-job -- openapi > docs/openapi.json
(cd package && bun run generate)
```

在 `package/` 中运行 `bun pm pack` 会生成下游检出所安装的 tarball（docs/api.md，"The client"）。

部署后，服务以用户单元 `deploy/bmo-job.service` 运行，即从部署树运行 `bmo-job serve`；bmo-testbed 依据
`testbed.toml` 将提交上线（docs/deploy.md）。在 BMO 机器上，服务由智能体按照索引的 INSTALL.md
（`bmo install`，https://bmo.ahas.io/install.md）安装。

## 许可证

MIT。全文见 `LICENSE`。
