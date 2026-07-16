# codex-qianzong

[English](#english) | [中文](#中文)

![codex-qianzong dashboard](docs/assets/dashboard-overview.png)

<!-- release-verification:start -->

## 最新发布校验 / Latest Release Verification

当前版本 / Current version: `v1.5.3`

| 文件 / File                        | SHA-256                                                            |
| ---------------------------------- | ------------------------------------------------------------------ |
| `codex-qianzong_1.5.3_x64.msi`     | `e737716cfdd74335e20598ff69a8b5d9d3a6e9935cad51b1ad4e01dbed3f7778` |
| `codex-qianzong_1.5.3_aarch64.dmg` | `2a340c98a2c6bde8426d45d7ad3115f297c3ba72c9a422b20e2591136517bb32` |
| `codex-qianzong_1.5.3_x86_64.dmg`  | `dc99a5a62a9b66bb80c80ae308655d5c4b6899a02246fc5aa302bf3bf9d2f051` |

附注：

- Apple Silicon 请下载 `codex-qianzong_1.5.3_aarch64.dmg`
- Mac Intel 请下载 `codex-qianzong_1.5.3_x86_64.dmg`

Notes:

- Apple Silicon: download `codex-qianzong_1.5.3_aarch64.dmg`
- Mac Intel: download `codex-qianzong_1.5.3_x86_64.dmg`
<!-- release-verification:end -->

## 中文

`codex-qianzong` 是一个开源的跨平台 Codex 桌面仪表盘，用于管理本机 Codex / ChatGPT Codex 的官方登录与 API 中转切换，并查看额度窗口、令牌用量、价值估算、会话状态、Skills 技能和运行环境诊断。

它同时支持官方原生登录和第三方 API / API 中转模式。设置页可以安全保存两种模式各自的认证状态、获取中转模型列表、统一同一台电脑上的 Codex 会话历史，并通过内置备份恢复配置和历史迁移。

> 本项目是社区工具，非 OpenAI 官方产品。

### 核心功能

- **额度窗口监控**：展示 5 小时 / 7 天滚动额度窗口和重置时间。
- **令牌价值估算**：按官方 API 价格估算今日、近 7 天、累计和当前会员周期价值。
- **官方原生 / API 中转一键切换**：自动同步 Codex `config.toml` 与 `auth.json`，保留 MCP、项目路径和 trust records 等无关配置。
- **加密认证保险箱**：完整保存官方 `auth.json` 登录信息和中转 API Key；密文使用 ChaCha20-Poly1305。Windows 主密钥由系统凭据库管理，macOS 使用应用数据目录中的 `0600` 本地密钥文件，不申请 Keychain 权限。
- **API Key 复用**：中转 Key 与 API 地址绑定，同一地址再次切换时无需重复输入；地址变化时要求提供新的 Key。
- **模型自动获取**：填写 API 地址和 Key 后，可调用 `GET /v1/models` 获取 OpenAI 文本 / 推理 / Codex 模型选项；同时保留不受限制的手动填写。
- **统一本机会话历史**：可选地让官方和中转模式共用本机 Codex 会话；支持迁移现有 JSONL 与 `state_5.sqlite`，并可从备份恢复。
- **配置备份与恢复**：首次启动自动保存默认配置；可手动保存、恢复、删除非默认备份。
- **Skills 技能看板**：浏览本机 Codex Skills，支持搜索、翻译描述、启用 / 禁用 / 归档用户技能。
- **环境诊断**：检查 Codex CLI、数据目录、SQLite state、app-server、会话日志解析状态。
- **本地优先**：SQLite、JSONL、配置同步等敏感操作都在本机 Rust 侧完成。
- **跨平台桌面**：Tauri 2 + React + Rust，面向 Windows 和 macOS。

### v1.5.1 更新

- **认证保险箱**：完整保存官方 Codex `auth.json` 登录快照，并将中转 API Key 与对应端点绑定后加密保存；主密钥由系统 keyring 管理。
- **可靠切换登录状态**：切到 API 中转前保留官方 access / refresh / ID token；切回官方模式时恢复原有登录状态。即使没有旧快照，也可以正常进入新的官方登录流程。
- **统一会话历史**：可选让官方模式与 API 中转模式在同一台电脑上共用 Codex 会话历史；既有 JSONL 与 SQLite 记录仅在用户明确启用迁移后处理，并提供备份与恢复。
- **跨平台安装包**：Release 提供 Windows x64 MSI，以及分别适用于 Apple Silicon 和 Intel Mac 的 DMG；DMG 内包含拖放到 `Applications` 的快捷入口。

### 第三方 API / API 中转

设置页可以把 Codex 从官方原生模式切换到 API 中转模式：

![settings api switch](docs/assets/settings-api-switch.png)

保存中转配置后会同步：

- `model_provider = "qianzong_relay"`
- `[model_providers.qianzong_relay]`
- `base_url`
- `wire_api = "responses"`
- `preferred_auth_method = "apikey"`
- 模型、推理强度、速度策略
- `auth.json` 中的 `auth_mode = "apikey"` 与 `OPENAI_API_KEY`

切换到中转前，应用会把完整的官方 `auth.json` 保存到加密认证保险箱。切回官方原生模式时，会移除千总中转 provider 并恢复此前的完整官方登录状态，包括 access token、refresh token、ID token 和应用尚不认识的字段。

如果设备上从未登录过官方账号，切回官方模式也不会被中转认证卡住：应用会把 `model_provider` 恢复为 `openai`，并把 `auth.json` 写为 `{}`，此时可直接通过 Codex 完成新的官方登录。中转地址、模型、推理强度和速度偏好会保留，方便下次切换。

### 模型列表

在 API 中转模式下，填写 API 地址和 API Key 后点击“获取模型”，应用会请求该服务的 `GET /v1/models` 接口。自动列表只保留可用于 Codex 的 OpenAI GPT、ChatGPT、o 系列和 Codex 模型，并排除审核、安全、图片、嵌入、音频和实时模型。

自动过滤只作用于接口返回的选项。你始终可以选择“手动填写”，输入和修改任意服务实际支持的模型名。

### 统一会话历史

“统一 Codex 会话历史”用于统一**同一台电脑上**官方模式和 API 中转模式的本地会话，它不是跨设备云同步。启用后，两种模式使用同一个受管理的 provider ID，因此可以继续打开另一种模式创建的会话。

如需迁移已有会话，可额外启用迁移选项。应用会改写已知 provider 的会话 JSONL 元数据和 `state_5.sqlite` thread 记录，并在修改前创建逐文件、数据库级备份和来源记录。为避免数据库并发写入，迁移及恢复前必须完全退出 Codex。关闭统一模式后，可使用设置页的恢复功能还原仍由统一 provider 管理的记录；未知 provider 不会被改动。

### 配置备份

为了防止修改 Codex 配置时误伤其他参数，应用会在首次启动时自动创建 `default-initial` 默认备份。你也可以在设置页：

- 保存当前配置备份
- 从下拉框选择并恢复备份
- 删除非默认备份

恢复备份前会额外保存当前配置，因此误恢复后仍有回退路径。

### Skills 技能看板

![skills board](docs/assets/skills-board.png)

技能看板只读取 bounded metadata，不把完整 `SKILL.md` 内容发送到前端。系统技能、插件缓存技能、受保护技能默认为只读；用户技能可以启用、禁用或归档。

### 数据来源

- `codex app-server` JSON-RPC：
  - `account/read`
  - `account/rateLimits/read`
  - `account/usage/read`
- 本机 Codex SQLite：
  - Windows: `%USERPROFILE%\.codex\state_5.sqlite`
  - macOS: `~/.codex/state_5.sqlite`
  - fallback: `.codex/sqlite/state_5.sqlite`
- 会话日志：
  - `.codex/sessions/**/rollout-*.jsonl`
- 自动化配置：
  - `.codex/automations/**/automation.toml`

### 安装与使用

从 [Releases](https://github.com/Riieco/codex-qianzong/releases) 下载 Windows 安装包或 macOS DMG，也可以从源码运行。macOS 构建同时提供 Apple Silicon (`aarch64`) 和 Intel (`x86_64`) 版本。

macOS 安装包使用 ad-hoc 签名，尚未经过 Apple 公证，首次打开时可能出现系统安全提示。当前版本的准确文件名和 SHA-256 见 README 顶部的发布校验区块。

桌面快捷键：

- Windows: `Ctrl+Alt+U`
- macOS: `Command+U`

托盘菜单：

- 显示 / 隐藏
- 切换窗口置顶
- 退出

### 从源码开发

环境要求：

- Node.js 24+
- Rust 1.92+
- Microsoft WebView2 Runtime
- Windows 或 macOS

```powershell
npm install
npm run dev
```

前端预览：

```powershell
npm run dev:frontend
```

验证：

```powershell
npm run lint
npm run test -- --run
npm run build
npm run rust:check
npm run rust:test
```

打包：

```powershell
npm run tauri build
```

Windows 产物位于：

```text
src-tauri/target/release/bundle/
```

仓库的 GitHub Actions 安装包工作流会构建 Windows x64 MSI，并分别构建 Apple Silicon 和 Intel DMG。三个安装包均作为工作流 artifact 上传。

### 手动发布 Release

维护者可在 GitHub `Actions` 页面选择 `Publish release`，点击 `Run workflow`，从 `main` 分支运行并填写尚未使用的版本号，例如 `1.5.3`。工作流会自动：

- 更新 npm、Cargo 和 Tauri 的项目版本并提交到 `main`
- 运行前端测试、ESLint 和 Rust 测试
- 构建 Windows x64 MSI、Apple Silicon DMG 和 Intel DMG
- 计算三个安装包的 SHA-256，并更新 README 顶部的发布校验区块
- 创建 `v版本号` tag 和 GitHub Release，上传安装包及 `SHA256SUMS.txt`

不要提前创建 tag；已有 tag 或 Release 的版本号会被拒绝。仓库必须允许 GitHub Actions 使用 `contents: write` 向 `main` 推送版本与 README 提交。

### 技术栈

- Tauri 2
- React 19
- TypeScript
- Vite
- Rust
- SQLite / `rusqlite`
- `toml_edit` / `serde_json`
- Tauri tray and global shortcut

### 安全边界

- 前端不直接读取任意本地文件。
- Codex 配置、认证文件、SQLite、JSONL 读取都在 Rust 命令中处理。
- API Key 不写入应用自己的 `settings.json`，也不会返回给前端。
- 模型获取不跟随重定向，并限制响应大小；Key 只作为 Bearer Header 发送到设置的目标地址。
- 认证保险箱的中转 Key 与 API 地址绑定，官方认证和中转认证使用独立存储槽位。
- README 和 UI 不展示原始密钥或完整配置内容。
- 配置恢复只使用应用管理的备份目录，并校验备份 ID。

### 文档

- [Architecture](docs/architecture.md)
- [Data contract](docs/data-contract.md)
- [UI design](docs/ui-design.md)
- [Windows packaging](docs/windows-packaging.md)
- [macOS packaging](docs/macos-packaging.md)

### 许可证

MIT License. See [LICENSE](LICENSE).

---

## English

`codex-qianzong` is an open-source cross-platform Codex desktop dashboard. It manages official-login and API-relay switching for local Codex / ChatGPT Codex while showing quota windows, token usage, estimated API value, session state, local Skills metadata, and runtime diagnostics.

It supports both official native login and third-party API relay mode. Settings can preserve each mode's credentials, discover relay models, unify local session history on one computer, and restore managed configuration or history backups.

> This is a community tool and is not an official OpenAI product.

### Highlights

- **Quota windows**: Monitor 5-hour and 7-day rolling quota windows.
- **Token value estimate**: Estimate today, 7-day, lifetime, and membership-cycle value using official API pricing assumptions.
- **Official / API relay one-click switch**: Synchronize Codex `config.toml` and `auth.json` while preserving unrelated MCP, project-path, and trust settings.
- **Encrypted credential vault**: Preserve the complete official `auth.json` login and the relay API Key. Windows keeps the master key in the system credential store; macOS uses a local `0600` key file in the app data directory and does not request Keychain access.
- **Endpoint-bound Key reuse**: Reuse the stored relay Key for the same endpoint without entering it again; changing the endpoint requires a new Key.
- **Model discovery**: Call `GET /v1/models` after entering an endpoint and Key, choose from eligible OpenAI text/reasoning/Codex models, or enter any model manually.
- **Unified local session history**: Optionally share local Codex sessions between official and relay modes, migrate existing JSONL and `state_5.sqlite` records, and restore them from managed backups.
- **Managed config backups**: Automatically save the first-run default config; manually save, restore, and delete non-default backups.
- **Skills board**: Browse local Codex Skills, search, translate descriptions, enable / disable / archive user skills.
- **Environment diagnostics**: Check Codex CLI, data directory, SQLite state, app-server, and session log parsing.
- **Local-first boundary**: SQLite, JSONL, and Codex config operations stay in the Rust desktop layer.
- **Cross-platform desktop**: Tauri 2 + React + Rust for Windows and macOS.

### v1.5.1 changes

- **Credential vault**: Preserve the complete official Codex `auth.json` login snapshot and encrypt relay API keys bound to their endpoints, with the master key stored in the operating-system keyring.
- **Reliable auth switching**: Keep official access, refresh, and ID tokens before entering relay mode, then restore the previous login state when returning to official mode. A fresh official login remains available when no snapshot exists.
- **Unified session history**: Optionally share Codex history between official and relay modes on the same machine. Existing JSONL and SQLite records are migrated only after explicit opt-in, with backups and restore support.
- **Cross-platform installers**: Releases provide a Windows x64 MSI and separate DMGs for Apple Silicon and Intel Macs. Each DMG includes an `Applications` shortcut for drag-to-install.

### Third-party API / API Relay

The settings drawer can switch Codex from official native mode to API relay mode:

![settings api switch](docs/assets/settings-api-switch.png)

When saved, the app synchronizes:

- `model_provider = "qianzong_relay"`
- `[model_providers.qianzong_relay]`
- `base_url`
- `wire_api = "responses"`
- `preferred_auth_method = "apikey"`
- model, reasoning effort, and speed strategy
- `auth.json` fields including `auth_mode` and `OPENAI_API_KEY`

Before entering relay mode, the app saves the complete official `auth.json` in its encrypted credential vault. Switching back removes the managed relay provider and restores the complete previous official login, including access, refresh, and ID tokens plus fields the app does not yet recognize.

If the device has never had an official login, switching back still succeeds: `model_provider` is restored to `openai` and `auth.json` becomes `{}`, allowing Codex to start a normal official login. Relay endpoint, model, reasoning, and speed preferences remain available for the next switch.

### Model discovery

In API relay mode, enter the API endpoint and Key, then click the model-fetch button to call that service's `GET /v1/models` endpoint. The automatic list keeps OpenAI GPT, ChatGPT, o-series, and Codex candidates suitable for Codex, while excluding moderation, safety, image, embedding, audio, and realtime families.

Filtering applies only to fetched options. The manual option remains unrestricted and editable for any model name supported by the configured service.

### Unified session history

“Unified Codex session history” shares local sessions between official and API relay modes on the **same computer**. It is not cross-device cloud synchronization. When enabled, both modes use one managed provider ID so sessions created in either mode can be resumed.

Existing sessions can be migrated explicitly. The app rewrites known provider metadata in session JSONL files and `state_5.sqlite` thread rows, first creating per-file backups, a database backup, and a source-provider ledger. Codex must be fully closed before migration or restore to prevent concurrent database writes. After unified mode is disabled, the restore action changes only records still owned by the managed unified provider; unknown providers are left untouched.

### Managed backups

On first launch, the app creates a protected `default-initial` backup. In settings you can:

- save the current Codex config backup
- restore a selected backup from the dropdown
- delete non-default backups

Before restore, the app creates timestamped backups of the current files, so you still have a rollback path.

### Skills board

![skills board](docs/assets/skills-board.png)

The Skills board reads bounded metadata only. It does not send full `SKILL.md` bodies to the frontend. System skills, plugin-cache skills, and protected skills are read-only; user skills can be enabled, disabled, or archived.

### Data sources

- `codex app-server` JSON-RPC:
  - `account/read`
  - `account/rateLimits/read`
  - `account/usage/read`
- Local Codex SQLite:
  - Windows: `%USERPROFILE%\.codex\state_5.sqlite`
  - macOS: `~/.codex/state_5.sqlite`
  - fallback: `.codex/sqlite/state_5.sqlite`
- Session logs:
  - `.codex/sessions/**/rollout-*.jsonl`
- Automations:
  - `.codex/automations/**/automation.toml`

### Install and run

Download Windows installers or macOS DMGs from [Releases](https://github.com/Riieco/codex-qianzong/releases), or run from source. macOS artifacts are built for both Apple Silicon (`aarch64`) and Intel (`x86_64`).

The macOS builds are ad-hoc signed and are not Apple-notarized, so macOS may show a security warning on first launch. See the release verification block at the top of this README for the current exact filenames and SHA-256 values.

Desktop shortcuts:

- Windows: `Ctrl+Alt+U`
- macOS: `Command+U`

Tray menu:

- Show / Hide
- Toggle Always On Top
- Quit

### Development

Requirements:

- Node.js 24+
- Rust 1.92+
- Microsoft WebView2 Runtime
- Windows or macOS

```powershell
npm install
npm run dev
```

Frontend-only preview:

```powershell
npm run dev:frontend
```

Verification:

```powershell
npm run lint
npm run test -- --run
npm run build
npm run rust:check
npm run rust:test
```

Desktop build:

```powershell
npm run tauri build
```

Windows artifacts are written under:

```text
src-tauri/target/release/bundle/
```

The repository's GitHub Actions installer workflow builds a Windows x64 MSI plus separate Apple Silicon and Intel DMGs. All three installers are uploaded as workflow artifacts.

### Manual GitHub Release

Maintainers can open GitHub `Actions`, select `Publish release`, click `Run workflow`, choose `main`, and enter an unused version such as `1.5.3`. The workflow automatically:

- updates the npm, Cargo, and Tauri project versions and commits them to `main`
- runs frontend tests, ESLint, and Rust tests
- builds a Windows x64 MSI plus Apple Silicon and Intel DMGs
- computes installer SHA-256 values and updates the verification block at the top of this README
- creates the `v<version>` tag and GitHub Release, then uploads the installers and `SHA256SUMS.txt`

Do not create the tag first; versions with an existing tag or Release are rejected. The repository must allow GitHub Actions with `contents: write` to push the version and README commits to `main`.

### Stack

- Tauri 2
- React 19
- TypeScript
- Vite
- Rust
- SQLite / `rusqlite`
- `toml_edit` / `serde_json`
- Tauri tray and global shortcut

### Security boundary

- The frontend does not read arbitrary local files.
- Codex config, auth files, SQLite, and JSONL reads are handled by Rust commands.
- API keys are not persisted into the app's own `settings.json` and are never returned to the frontend.
- Model discovery does not follow redirects, limits response size, and sends the Key as a Bearer header only to the configured endpoint.
- Relay credentials are endpoint-bound and stored separately from the official-login snapshot.
- The README and UI never expose raw secrets or full config contents.
- Config restore uses only app-managed backup directories and validates backup IDs.

### Documentation

- [Architecture](docs/architecture.md)
- [Data contract](docs/data-contract.md)
- [UI design](docs/ui-design.md)
- [Windows packaging](docs/windows-packaging.md)
- [macOS packaging](docs/macos-packaging.md)

### License

MIT License. See [LICENSE](LICENSE).
