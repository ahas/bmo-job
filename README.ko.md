[English](README.md) | **한국어** | [日本語](README.ja.md) | [简体中文](README.zh-CN.md)

# bmo-job

Redis가 미들웨어인 것과 같은 방식으로, 미들웨어로 운영되는 할 일 목록입니다. 항목과 그 노트, 보고서, 작업 지시서,
항목이 기다리는 큐, 항목에 대한 리스, 질문과 그 답변, 구독을 하나의 API로 저장하고 제공합니다. 이 서비스는 아무것도
실행하지 않으며 다른 어떤 유닛도 호출하지 않습니다. 작업은 두 가지 경로로 나란히 수행자에게 전달됩니다. 하나는
푸시로, 수행자가 등록한 구독의 이벤트가 웹훅으로 전달되거나 스트림으로 읽혀 MCP 알림으로 바뀝니다. 다른 하나는 풀로,
리스를 통해 가져갑니다. 코딩 하니스는 `bmo-job` 플러그인(MCP 서버 `bmo-job mcp`와 그 스킬)을 첫 진입점으로 삼아 작업을
받습니다.

## 구성

| 폴더 | 내용 |
|---|---|
| `src/` | 서비스 자체 패키지: 저장소, HTTP 서버와 이벤트 스트림, 웹훅 전달, 설정, 그리고 `src/bin/` 아래의 바이너리 `bmo-job` |
| `crates/` | `client`(크레이트 `bmo-job-client`): Rust로 표현한 계약으로, 모든 본문을 타입으로, 모든 라우트를 호출로 담고, 이벤트 스트림, 웹훅 서명, 그 타입들에서 생성되는 OpenAPI 문서를 포함. `plugin`(크레이트 `bmo-job-plugin`): 하니스 플러그인, 클라이언트 위의 MCP 서버와 그 도구, 도구를 포함한 전체 OpenAPI 문서, `skills/` 아래의 `bmo-job` 스킬. `tool-call`: 도구 호스트의 호출 프로토콜과 도구들이 함께 쓰는 서비스 접근 |
| `package/` | 다운스트림 체크아웃용으로 패킹되는 TypeScript 패키지 `bmo-job-client`: `docs/openapi.json`에서 생성한 타입과 그 위의 클라이언트 |
| `deploy/` | 배포 트리에서 `bmo-job serve`를 실행하는 서비스의 사용자 유닛 (docs/deploy.md) |
| `tools/` | 도구 호스트용 서비스 도구 `todo`, `state`, `launch`. 각각 매니페스트, 실행 파일, 스키마로 구성 (docs/tools.md) |
| `tests/` | 루트 패키지의 테스트: 저장소 검사, 계약 검사, 실제 서버를 통한 모든 라우트, 그리고 유닛과 하니스가 실행하는 방식의 바이너리 |
| `docs/` | 서비스가 하는 일에 대한 정식 기술과, 코드가 생성하는 계약인 `openapi.json` |

루트에는 Cargo 워크스페이스 매니페스트, 버전, `testbed.toml`(이 저장소를 어떻게 테스트, 검사, 빌드, 배포하는지에
대한 bmo-testbed의 선언, docs/deploy.md), `install.toml`(새 머신에 설치할 때 필요한 것으로, `bmo install`이
읽습니다), `rustfmt.toml`, `CLAUDE.md`, 배포가 제공하는 설정 파일의 예시인
`settings.example.toml`, 그리고 `LICENSE`가 있습니다.

## 문서

| 문서 | 내용 |
|---|---|
| `docs/api.md` | API: 모든 리소스, 그 상태와 전이, 라우트, 이벤트, 구독과 웹훅 전달, 클라이언트와 하니스 플러그인 |
| `docs/openapi.json` | 코드가 생성하는 계약. 도구는 `x-tools`, 하니스 진입점은 `x-harness` 아래에 있음 |
| `docs/tools.md` | 서비스가 배치하는 도구: 큐의 진입점, 채팅 에이전트의 상태 조회와 실행 진입점 |
| `docs/cli.md` | 명령줄: `nouns`, `serve`, `mcp`, `job close`, `job ask`, `todo`, `openapi` |
| `docs/settings.md` | 설정 파일: 서비스가 수신하는 주소, 토큰과 그 토큰이 가리키는 호출자, 항목이 저절로 열리는 레이블, 데이터베이스 |
| `docs/schema.md` | 스키마: 테이블과 테이블이 지키는 규칙 |
| `docs/deploy.md` | 서비스 배포: 릴리스에 담기는 것, 서비스가 놓이는 위치, 배포를 통제하는 조건 |
| `docs/base-libraries.md` | 작업이 import하는 기반 라이브러리에 필요한 부분이 없을 때 거치는 경로: 풀 리퀘스트, 지켜야 할 규칙, 리뷰, 버전 고정 |
| `docs/checks.md` | 저장소 검사 |
| `docs/PERSONAL-DATA.md` | 저장소에 둘 수 없는 개인정보, 그 값을 대신 두는 곳, 허용되는 값 |

## 버전

- **저장소 전체에 하나의 버전,** 즉 워크스페이스의 버전을 씁니다. `VERSION`이 이를 담고, 워크스페이스의 버전,
  OpenAPI 문서, TypeScript 패키지의 버전이 모두 이와 같습니다.
- **버전은 소유자가 선언한 릴리스에서만 바뀝니다.** bmo-chat의 `docs/conventions.md` 「Versions」에 적힌 대로,
  커밋, 배포, 수정, 계약 변경은 버전을 바꾸지 않으며, 배포는 커밋으로 구별합니다.
- **첫 릴리스 전까지 버전은 리비전 접미사가 붙은 `0.1.0`,** 즉 `0.1.0-rev.N`입니다. `0.1.0`이 기준이고, 소유자가
  릴리스를 선언할 때마다 리비전이 하나씩 올라갑니다. 첫 릴리스 전에는 메이저, 마이너, 패치 번호가 바뀌지 않습니다.

## 명령

```sh
cargo check --workspace --all-targets
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
(cd package && bun install --frozen-lockfile && bun run check)
```

서버는 `BMO_JOB_SETTINGS`가 가리키는 설정 파일을, 없으면 `settings.toml`을 읽고(docs/settings.md) 서비스를
시작합니다. MCP 서버는 주소로 실행 중인 서비스에 접속합니다.

```sh
cargo run --bin bmo-job -- serve
BMO_JOB_TOKEN=... cargo run --bin bmo-job -- mcp --service http://127.0.0.1:7841
```

계약을 바꾸면 문서와 TypeScript 타입을 다시 생성합니다.

```sh
cargo run --bin bmo-job -- openapi > docs/openapi.json
(cd package && bun run generate)
```

`package/`에서 `bun pm pack`을 실행하면 다운스트림 체크아웃이 설치하는 tarball이 만들어집니다(docs/api.md,
"The client").

배포된 서비스는 사용자 유닛 `deploy/bmo-job.service`로, 배포 트리에서 `bmo-job serve`로 실행됩니다. bmo-testbed가
`testbed.toml`에 따라 커밋을 운영에 반영합니다(docs/deploy.md). BMO 머신에서는 에이전트가 인덱스의
INSTALL.md(`bmo install`, https://bmo.ahas.io/install.md)로 서비스를 설치합니다.

## 라이선스

MIT. 전문은 `LICENSE`에 있습니다.
