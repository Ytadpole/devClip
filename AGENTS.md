# AGENTS.md

给后续 OpenCode 会话的速查表。只记录从 config 和代码里推不出、或容易推错的事。

本机环境事实（Node/pnpm 位置、sudo 限制、网络可达性）在全局
`~/.config/opencode/AGENTS.md`，不在本文件重复。

## 架构：两个咽喉点

前端不碰系统能力，所有调用收敛到两处：

- `src/lib/api.ts` — 唯一契约，`ClipboardApi`（16 个方法）
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
  会先匹配到它。状态栏用 `[data-status]`（`ok` / `warn` / `err`），
  千万别改成按配色类找：状态色跟着主题走，浅色下 `text-emerald-400`
  白底读不出来，类名一变测试就跟着碎
- **`e2e/` 里不 import 应用代码**。行高常量是故意写死的 `62`：导入了的话
  改了 `ROW_H` 测试会跟着变，回归就悄悄溜过去
- **没测防抖间隔**，这是有意的。store 的请求序号会丢弃过期响应，所以把
  `SEARCH_DEBOUNCE` 改成 0 也只会重绘一次 —— 查询次数在 UI 上观测不到。
  要测得往 `api` 层加计数器，属于改被测代码换可观测性

已验证能抓到的回归（改坏后确实会红）：摘掉 `VirtualList` 的 pin 区间、
只改 `ItemRow` 的行高而不动 `ROW_H`、删掉 `index.css` 里
`html[data-theme="light"]` 那组覆盖（主题测试会读 `body` 的实际底色）。

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
- **配色分两套写法，别混**（见 `index.css` 顶部注释）：中性色
  （`bg-canvas` / `text-fg` / `border-line` …）是 `@theme` 变量，默认深色，
  亮色在 `html[data-theme="light"]` 里整组覆盖；强调色（13 种类型色、
  状态色）用元素上的 `light:` 变体。**组件里不要直接写 `zinc-950`、
  `white/10` 这类字面色**，那样切不掉
- **`Settings.theme` 早就存在但以前没人消费** —— Rust 侧一直在存
  （`settings.rs` / `clamp_settings` 都已放行 `dark|light|system`）。
  这次的活儿全在前端：解析 `system`、落成 `<html data-theme>`、设置页开关
- **`src/lib/mock.ts` 是内存态** —— 刷新即重置。新功能要造数据就往这里加
- **用户启动的是 `~/soft/DevClip_*.AppImage`，不是构建输出目录里那份**
  —— 两者同名同版本号，只有 md5 不同，光看文件名分不出新旧。
  现在 `~/soft/` 那个已改成指向 `target/release/bundle/appimage/`
  的软链；**别再往 `~/soft/` 里 cp**，会把它换成实体文件、陷阱复发。
  验证前先对一下 `md5sum`，或看 `/proc/<pid>/exe` 指向哪个包
- **`docs/` 在版本库中** —— 6 份设计文档 + `DEVLOG.md` 都已入库。
  设计有变更时连同文档一起改，别让代码走在文档前面
- 有 CI（`.github/workflows/ci.yml`），无 pre-commit hook。CI 跑三平台
  矩阵 + macOS 真机检查；**没有 Linux 上的 macOS 交叉检查**，
  那条路走不通（见 ci.yml 里的说明）

## 提交信息

用户明确要求：**commit message 精简**，不要 Markdown 小节或长清单。

- 标题 ≤ 50 列，`<type>: <祈使句摘要>`，一句话说完
- **默认只写标题**；确有必要时正文不超过 5 行，纯文本段落折行 ≤ 72 列
- **复杂的「为什么」写进 `docs/DEVLOG.md`**，那条记录与 GitHub 上的
  commit **一一对应**，以 commit hash 为键。该文件已入库，所以
  「一一对应」变成了对读者的承诺，不只是自己的备忘
- 追加 DEVLOG 条目时顺序必须与 `git log --reverse` 一致。提交后校验
  （最后一条 commit 自己的条目要等**下一次**提交才写得出，所以只在
  「差一行且那一行是最新 commit」时放过）：

  ```bash
  grep -oE '^## [0-9a-f]{7}' docs/DEVLOG.md | awk '{print $2}' > /tmp/dl.txt
  git log --format='%h' --reverse > /tmp/gl.txt
  diff /tmp/dl.txt /tmp/gl.txt   # 期望只差最后一行
  ```
- 一次提交一个主题；超过 50 行的改动就拆成多次提交

类型前缀：`feat` `fix` `chore` `refactor` `docs` `perf`

推送前先问用户，不要自行 `git push`。
