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

## 文档

| 文档 | 内容 |
|---|---|
| [01 环境搭建](docs/01-环境搭建.md) | Linux Mint / macOS / Windows 的开发环境安装步骤与验证方法 |
| [02 架构设计](docs/02-架构设计.md) | 分层边界、端口-适配器、Rust 模块划分、事件流 |
| [03 数据库设计](docs/03-数据库设计.md) | SQLite schema、FTS5 全文搜索、去重与图片存储策略 |
| [04 内容识别与工具箱](docs/04-内容识别与工具箱.md) | 类型识别规则表 + 每种类型的操作矩阵 |
| [05 平台差异与风险](docs/05-平台差异与风险.md) | 三平台剪贴板/粘贴/快捷键/权限差异与降级策略 |
| [06 路线图与任务清单](docs/06-路线图与任务清单.md) | 阶段划分、完成标准、Rust 学习阶梯 |

---

## 技术栈

| 模块 | 选择 | 原因 |
|---|---|---|
| UI | React + TypeScript | 复杂交互、生态成熟 |
| 构建 | Vite | 快 |
| 桌面框架 | Tauri 2 | 跨平台、体积与内存占用低 |
| 系统层 | Rust | 剪贴板、快捷键、模拟按键、文件 |
| 数据 | SQLite（rusqlite） | 本地工具标配 |
| 搜索 | SQLite FTS5 + trigram 分词器 | 子串级全文搜索，适合代码 |
| 状态 | Zustand | 简单够用 |
| 样式 | Tailwind CSS v4 + shadcn/ui + cmdk | 快速开发，调色板交互现成 |
| 包管理 | pnpm | Tauri 前后端双包，节省磁盘 |
| AI | OpenAI 兼容 API | P2 再接入 |
| CI | GitHub Actions + tauri-action | 三平台自动构建 |
| 发布 | GitHub Releases | 开源分发 |

## 平台路线

```
V0.1  macOS
V0.2  macOS + Linux
V0.3  macOS + Linux + Windows
```

## 当前状态

**文档阶段。** 尚未初始化工程，环境未安装（Node / Rust / webkit2gtk 均缺失）。
下一步见 [01 环境搭建](docs/01-环境搭建.md)。
