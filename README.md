# DevClip

> 面向开发者的跨平台剪贴板 + 开发工具箱。

```
复制 → 自动保存 → 快捷键呼出 → 搜索 → 选择 → 粘贴
```

它不是"又一个剪贴板管理器"，而是在剪贴板之上叠加一层**开发者语义**：自动识别内容类型（JSON / JWT / SQL / Base64 / URL / UUID / IP / 异常栈 / Git Commit / Markdown），并针对每种类型提供可执行操作（格式化、压缩、解码、解释……）。

---

## 三层能力

```
                DevClip
                   │
        ┌──────────┼──────────┐
        ↓          ↓          ↓
   Clipboard    Toolbox      AI
        │          │          │   (P2)
   History     JSON       Explain
   Search      SQL        Translate
   Favorite    JWT        Fix
   Tags        Base64     Summarize
```

## 当前进度

**阶段 0–1 完成，下一步阶段 2。** Tauri 2 脚手架 + React 调色板界面，
数据来自内存 mock，尚未接 Rust。

| 阶段 | 内容 | 状态 |
|---|---|---|
| 0 | 环境 + 脚手架 | ✅ |
| 1 | UI + mock 数据 | ✅ |
| 2 | 端口-适配器接线（换 tauri 后端） | ⬜ |
| 3 | `detect.rs` 内容识别器（纯 Rust） | ⬜ |
| 4 | SQLite + FTS5 历史记录 | ⬜ |
| 5 | 监听 + 快捷键 + 粘贴 | ⬜ |

阶段 2 起需要 Rust 工具链与 webkit2gtk 开发包，本机尚未安装。

## 开发

```bash
pnpm install
pnpm dev       # http://localhost:1420
pnpm build     # tsc 类型检查 + vite 打包
pnpm test      # 端到端验收（Playwright，会自动起 dev server）
```

首次跑 `pnpm test` 要先下载浏览器，约 115MB：

```bash
pnpm exec playwright install chromium
```

`pnpm tauri dev` 需要 Rust 与 webkit2gtk，尚未安装。

## 技术栈

| 模块 | 选择 |
|---|---|
| UI | React 19 + TypeScript 6 |
| 构建 | Vite 8 |
| 桌面框架 | Tauri 2 |
| 系统层 | Rust（阶段 3 起） |
| 数据 | SQLite + FTS5 trigram（阶段 4） |
| 状态 | Zustand |
| 样式 | Tailwind CSS v4 + cmdk |
| 验收 | Playwright（`e2e/`） |
| 包管理 | pnpm |

## 文档

设计文档位于 [`docs/`](docs/)，**因用户要求暂不入版本库**，仅存在于本地工作副本。

| 文档 | 内容 |
|---|---|
| [01 环境搭建](docs/01-环境搭建.md) | 三平台开发环境安装与验证 |
| [02 架构设计](docs/02-架构设计.md) | 分层边界、端口-适配器、事件流 |
| [03 数据库设计](docs/03-数据库设计.md) | SQLite schema、FTS5、去重、图片与敏感信息 |
| [04 内容识别与工具箱](docs/04-内容识别与工具箱.md) | 13 条识别规则 + 操作矩阵 |
| [05 平台差异与风险](docs/05-平台差异与风险.md) | 剪贴板/粘贴/快捷键三平台差异与降级策略 |
| [06 路线图与任务清单](docs/06-路线图与任务清单.md) | 阶段划分、完成标准、风险台账 |

## 平台路线

```
V0.1  macOS
V0.2  macOS + Linux
V0.3  macOS + Linux + Windows
```

## 平台

```
macOS · Windows · Linux(X11)
```
