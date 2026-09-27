# AGENTS.md

给后续 OpenCode 会话的速查表。只记录从 config 和代码里推不出、或容易推错的事。

本机环境事实（Node/pnpm 位置、sudo 限制、网络可达性）在全局
`~/.config/opencode/AGENTS.md`，不在本文件重复。

## 架构：两个咽喉点

前端不碰系统能力，所有调用收敛到两处：

- `src/lib/api.ts` — 唯一契约，`ClipboardApi`（11 个方法）
- `src/lib/backend.ts` — 唯一分叉点，`impls` 表 + `VITE_BACKEND`

规则：

- 组件与 store **不得**直接调 `invoke()`，一律走 `api.*`
- 过滤逻辑归后端（阶段 4 = SQLite FTS5），前端不做本地过滤
- 新增接口方法要**同时**改 `api.ts` 和 `mock.ts`，否则 UI 静默失效

## 校验

没有 eslint、prettier。类型检查有两个入口，都要过：

- `pnpm build` = `tsc && vite build` —— 只管 `src/`，**tsc 就是应用侧的类型检查**
- `pnpm typecheck:e2e` = `tsc -p tsconfig.e2e.json` —— 管 `e2e/` 和
  `playwright.config.ts`。测试代码刻意与 `src/` 分开：它要 node 类型，
  产物也不进 vite 打包。`pnpm test` 会先跑它（`pretest`）

`tsconfig.json` 开了 `noUnusedLocals` 和 `noUnusedParameters` —— 多一个没用到的
import 就构建失败。

### e2e 测试

`pnpm test` = Playwright 跑 `e2e/`。首次需要下载浏览器：
`pnpm exec playwright install chromium`（约 115MB，装在 `~/.cache/ms-playwright`）。

它会自动起 `pnpm dev`；本地已经开着就复用（1420 是 `strictPort`，抢不到就失败）。

几条写测试时踩到的，改测试前先看一眼：

- **断言一律用 locator**（`toHaveText` / `toHaveCount` / `toContainText`），
  它们自带重试。`textContent()` 是一次快照，mock 有 40ms 延迟，很容易读到旧值
- **别用 `div.border-t > span` 找状态栏** —— 工具箱那条也是 `border-t`，
  会先匹配到它。状态栏用 `.text-emerald-400, .text-amber-400`
- **`e2e/` 里不 import 应用代码**。行高常量是故意写死的 `62`：导入了的话
  改了 `ROW_H` 测试会跟着变，回归就悄悄溜过去
- **没测防抖间隔**，这是有意的。store 的请求序号会丢弃过期响应，所以把
  `SEARCH_DEBOUNCE` 改成 0 也只会重绘一次 —— 查询次数在 UI 上观测不到。
  要测得往 `api` 层加计数器，属于改被测代码换可观测性

已验证能抓到的回归（改坏后确实会红）：摘掉 `VirtualList` 的 pin 区间、
只改 `ItemRow` 的行高而不动 `ROW_H`。

Rust 侧：`cd src-tauri && cargo check` 比重开 `pnpm tauri dev` 快得多。

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
- **`docs/` 不在版本库中** —— 设计文档是 gitignored 的本地文件，不要假设 clone 后能看到
- 无 CI、无 pre-commit hook

## 提交信息

用户明确要求标准格式，不要用 Markdown 小节或长清单：

- 标题 ≤ 50 列，`<type>: <祈使句摘要>`
- 正文纯文本段落，折行 ≤ 72 列（中文按双宽计算）
- 只写「为什么」和代码看不出来的约束，不逐个罗列文件名
- 20~40 行为宜，超过 50 行就该拆成多次提交

类型前缀：`feat` `fix` `chore` `refactor` `docs` `perf`

推送前先问用户，不要自行 `git push`。
