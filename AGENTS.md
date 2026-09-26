# AGENTS.md

给后续 OpenCode 会话的速查表。只记录从 config 和代码里推不出、或容易推错的事。

## 能力边界（最大的坑）

Rust 工具链和 `libwebkit2gtk-4.1-dev` **都还没装**：

| 命令 | 状态 |
|---|---|
| `pnpm dev` | 可用 → http://localhost:1420 |
| `pnpm build` | 可用，**且是唯一的校验手段** |
| `pnpm tauri dev` / `pnpm tauri build` | 必然失败，不要尝试 |

`src-tauri/` 目前是 create-tauri-app 的原始模板，`lib.rs` 只有一句 `run()`，没有任何自有模块。

Node 与 pnpm 装在 `~/.nvm`，非交互 shell 不会读 `.bashrc`，直接跑 `pnpm` 会 command not found。先执行：

```bash
export NVM_DIR="$HOME/.nvm"; . "$NVM_DIR/nvm.sh"
```

只用 pnpm —— `package.json` 的 `packageManager` 锁到 12.6.0。

## 架构：两个咽喉点

前端不碰系统能力，所有调用收敛到两处：

- `src/lib/api.ts` — 唯一契约，`ClipboardApi`（11 个方法）
- `src/lib/backend.ts` — 唯一分叉点，`impls` 表 + `VITE_BACKEND`

规则：

- 组件与 store **不得**直接调 `invoke()`，一律走 `api.*`
- 过滤逻辑归后端（阶段 4 = SQLite FTS5），前端不做本地过滤
- 新增接口方法要**同时**改 `api.ts` 和 `mock.ts`，否则 UI 静默失效

## docs/ 被 git 忽略，但它是架构事实来源

`.gitignore:11` 忽略 `docs/*`。六份设计文档只存在于本地工作副本，`git status` 和 clone 都看不到。

改架构前先读对应文档。实现与文档冲突时两边都要更新 —— 但永远不要 `git add docs/`，确需入库时用 `git add -f docs/`。

内容：环境搭建 / 架构设计 / 数据库设计 / 内容识别与工具箱 / 平台差异与风险 / 路线图与任务清单。

## 校验

没有 eslint、prettier，也没有测试框架。`pnpm build` = `tsc && vite build`，**tsc 就是类型检查**。

`tsconfig.json` 开了 `noUnusedLocals` 和 `noUnusedParameters` —— 多一个没用到的 import 就构建失败。

Rust 侧（装好之后）：`cd src-tauri && cargo check` 比重开 `pnpm tauri dev` 快得多。

## 改包名要同步四处

`tauri-app` → `devclip` 时任漏一处都会编译失败：

1. `src-tauri/Cargo.toml` `[package] name`
2. `src-tauri/Cargo.toml` `[lib] name`（`tauri_app_lib` → `devclip_lib`）
3. `src-tauri/src/main.rs` 的 `devclip_lib::run()`
4. `src-tauri/tauri.conf.json` 的 `productName`

## 容易错的细节

- **端口 1420 且 `strictPort: true`** —— 被占用会直接失败。不要改端口，`tauri.conf.json` 的 `devUrl` 依赖这个值
- **Tailwind v4 没有 `tailwind.config.js`** —— 主题配在 `src/index.css` 的 `@theme` 里
- **`src/lib/mock.ts` 是内存态** —— 刷新即重置。新功能要造数据就往这里加
- **`clsx` / `tailwind-merge` 已安装但全项目零引用** —— shadcn/ui 预留，可卸
- 无 CI、无 pre-commit hook

## 提交信息

用户明确要求标准格式，不要用 Markdown 小节或长清单：

- 标题 ≤ 50 列，`<type>: <祈使句摘要>`
- 正文纯文本段落，折行 ≤ 72 列（中文按双宽计算）
- 只写「为什么」和代码看不出来的约束，不逐个罗列文件名
- 20~40 行为宜，超过 50 行就该拆成多次提交

类型前缀：`feat` `fix` `chore` `refactor` `docs` `perf`

## 推送

远程已配 `git@github.com:Ytadpole/devClip.git`，但**从未推送**。推不推由用户决定，不要自行 `git push`。
