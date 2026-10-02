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
        │          │          │   (P2，未做)
   History     JSON       Explain
   Search      SQL        Translate
   Favorite    JWT        Fix
   Tags (P1)   Base64     Summarize
               URL / UUID
```

标签那一格是设计稿里留的 P1，`src/` 里还没有对应实现。

## 当前进度

**阶段 0–8 完成，阶段 9 Windows 代码完成（待真机验证）。** macOS 与
Linux(X11) 上剪贴板监听、全局快捷键、模拟粘贴、托盘常驻、敏感内容
到期清理都已真机跑通。工具箱 19 个变换动作 + 「在浏览器打开」+
条目编辑（⌘E 内置编辑器），**点动作即粘贴**：写回剪贴板 → 收面板 →
激活原窗口 → 发键，一步到位。Windows 监听/粘贴已实现，还没有
真机可验，CI 编译检查与本机探针 crate 兜底。

| 阶段 | 内容 | 状态 |
|---|---|---|
| 0 | 环境 + 脚手架 | ✅ |
| 1 | UI + mock 数据 | ✅ |
| 2 | 端口-适配器接线（换 tauri 后端） | ✅ |
| 3 | `detect.rs` 内容识别器（纯 Rust） | ✅ 67 测试，准确率 100% |
| 4 | SQLite + FTS5 历史记录 | ✅ 1000 条子串搜索 2.5ms |
| 5 | 监听 + 快捷键 + 粘贴 + 托盘 | ✅ macOS / Linux 真机 |
| 6 | 开发者工具箱 + 条目编辑 | ✅ 19 变换 + 1 打开，点动作即粘贴 |
| 7 | 敏感信息 + 亮色主题 + 设置 | ✅ |
| 8 | Linux(X11) Selection 接管 | ✅ 真机 5/5 |
| 9 | Windows 监听 + 粘贴 | 代码完成；无真机，CI 编译检查兜底 |
| 10 | CI 出包 + 发布 | `release.yml` 已写，**一次都还没跑过**；macOS 未签名 |

测试共 273 条：Rust 单测 229（`cargo test --lib`）+ 端到端 44
（`pnpm test`）。CI 跑三平台矩阵，另有一道 macOS 真机构建检查；
推 `v*` tag 会走 `release.yml`，用 tauri-action 出三平台安装包。

数据落在 `~/.local/share/com.devclip.app/devclip.db`（WAL 模式）。
`cd src-tauri && cargo run --example seed` 可灌 1000 条假数据试搜索性能。

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

上面的命令只跑前端。起真实窗口需要 Rust 与 webkit2gtk，Linux 上
装齐这几个包（与 CI 一致）：

```bash
sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev \
  librsvg2-dev libxdo-dev patchelf
```

之后 `pnpm tauri dev` 起开发窗口，`pnpm tauri build` 产出
deb / rpm / AppImage 三种包。

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

设计文档与开发日志都在 [`docs/`](docs/) 里，已入库。

| 文档 | 内容 |
|---|---|
| [01 环境搭建](docs/01-环境搭建.md) | 三平台开发环境安装与验证 |
| [02 架构设计](docs/02-架构设计.md) | 分层边界、端口-适配器、事件流 |
| [03 数据库设计](docs/03-数据库设计.md) | SQLite schema、FTS5、去重、图片与敏感信息 |
| [04 内容识别与工具箱](docs/04-内容识别与工具箱.md) | 13 条识别规则 + 操作矩阵 |
| [05 平台差异与风险](docs/05-平台差异与风险.md) | 剪贴板/粘贴/快捷键三平台差异与降级策略 |
| [06 路线图与任务清单](docs/06-路线图与任务清单.md) | 阶段划分、完成标准、风险台账 |
| [DEVLOG](docs/DEVLOG.md) | 按提交记录的开发日志，一条提交对一条 |

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
