# DEVLOG · 详细开发记录

commit message 只留一行摘要。这里存「为什么」——踩过的坑、偏离规范的
取舍、当时的环境约束。**每条以 commit hash 为键，与 GitHub 上一一对应。**

已推送的 hash 可在 <https://github.com/Ytadpole/devClip/commits/main> 查到。

## 怎么读这份文件

条目**按 `git log --reverse` 的顺序**排列，也就是时间顺序。校验：

```bash
grep -oE '^## [0-9a-f]{7}' docs/DEVLOG.md | awk '{print $2}' > /tmp/dl.txt
git log --format='%h' --reverse > /tmp/gl.txt
diff /tmp/dl.txt /tmp/gl.txt
```

**本文件原本是 gitignore 的本地笔记**，`2c02b75` 起纳入版本库。
所以那之前的条目是**事后补录**的：那些 commit 推送时并没有带上对应
记录，是本文件入库时一次补上的。内容本身当时就写在本地笔记里，
只是补录时点晚于 commit —— 读的时候不必当成「当时就记下了」。

最后一条没有条目：它就是让这个文件进入版本库的那次提交，
条目无法包含自己这个尚未产生的 hash。

---

## 4e0aee8 · 2026-09-27 · chore: 初始化 DevClip 工程

`create-tauri-app` 生成的空工程，加 Tailwind v4 / cmdk / zustand。

包名从 `tauri-app` 改为 `devclip` 时要同步四处，任漏一处编译失败：
`Cargo.toml` 的 `[package] name`、`[lib] name`（`tauri_app_lib` →
`devclip_lib`）、`main.rs` 的 `devclip_lib::run()`、`tauri.conf.json` 的
`productName`。

---



## 8948819 · 2026-09-27 · docs: 新增 AGENTS.md 并重写 README

给后续会话的速查表。原则：只记录从 config 和代码推不出、或容易推错的事。

---



## af22454 · 2026-09-27 · docs: AGENTS.md 只留耐久内容

把本机状态（nvm 路径、sudo 限制、网络可达性）从项目文档里拆出去。
那些属于机器，换台机器就不成立，不该随项目分发。

---



## 0dc574c · 2026-09-27 · docs: 改用 OpenCode 两级 AGENTS.md 机制

项目级 `AGENTS.md` 留架构约束与易错细节；机器级事实移到全局
`~/.config/opencode/AGENTS.md`。

---



## 39b2fa5 · 2026-09-27 · chore: 停止追踪 .vscode/

个人编辑器配置，不该进版本库。

---



## 534eeea · 2026-09-27 · feat: 补完阶段 1 的虚拟列表、防抖与右键菜单

补 `docs/06` 清单剩下的四项：虚拟列表、80ms 搜索防抖、右键菜单、
mock 图片数据（内联 SVG 占位，行内出缩略图）。顺带清掉三处死代码：
`ALL_TYPES`、`clearStatus`，以及 `Palette` 里那个 `onClick` 是
`ALL_TYPES.forEach(() => {})` 的「共 N 条」假按钮。

**虚拟化与 cmdk 冲突，不是并列关系。** cmdk 定位当前项靠
`listInnerRef.querySelectorAll('[cmdk-item][aria-selected="true"]')`，
↑↓ 也在已渲染的项之间前后跳。窗口化之后它只看得到窗口内的项，
当前项会算错，`Enter` 更是**静默失效**——找不到节点派发选择事件，
不报错也不响应。所以键盘导航从 cmdk 手里接管（捕获阶段 +
`stopPropagation` 阻断），并守住一条硬约束：

> selected 那一项必须始终留在 DOM 里。

`VirtualList` 用两段区间（主窗口 + 单独钉住滚走的选中项）兜底，
键盘移动时再由 layout effect 拉回可视区。

**行高成了跨文件常量。** `VirtualList.ROW_H` 必须和 `ItemRow` 的
`h-[62px]` 一起改。中间踩了一次：Tailwind 是 border-box，
`border-l-2` 的 2px 算在高度里，先写的 60px 让内容溢出 2px。

**mock 补了 120 条批量数据。** 28 条手写数据在 420px 视口里只占 7 行，
触发不了窗口滚动，虚拟化有没有生效根本看不出来。

---



## fbc207c · 2026-09-27 · fix: 选中态色条错位并溢出到下一行

**这个 bug 比 `534eeea` 更早存在，被固定行高放大了。** 浏览器里量出来
才发现——纯读代码不会注意到。

`absolute` 元素的包含块是 padding box，所以 `left-0` 落在 `border-l-2`
内侧 2px；类名里又没写 `top`，`top: auto` 会退回静态位置，也就是
`py-2.5` 之下的 10px。行高固定成 62px 之后，色条那 10px 溢出正好
压到下一行，肉眼很明显；之前行高是 `auto` 时它按比例摊在行尾。

改成 `left-[-2px] top-0`。绝对定位色条时这两点都要显式写。

---



## 9141237 · 2026-09-27 · chore: 引入 Playwright 端到端验收

18 条用例，`pnpm test` 通过 `pretest` 先做 `tsconfig.e2e.json` 类型检查。

**动手前先验了测试本身能不能抓回归**，改坏三处确认会红：

| 改坏的地方 | 结果 |
|---|---|
| 摘掉 `VirtualList` 的 pin 区间 | pin 用例 + `End` 跳转一起红 |
| 只改 `ItemRow` 行高、不动 `ROW_H` | 3 条几何用例 + 色条对齐一起红 |
| 色条改回 `left-0` | 红（实测偏 10px / 2px） |

第二条尤其有用——`ROW_H` 和 `h-[62px]`「要一起改」此前只是注释里的
一句话，现在有测试兜着了。

**揪出一条假通过。** 原本用「列表重绘了几次」验证防抖，把
`SEARCH_DEBOUNCE` 改成 0 它照样绿——store 的请求序号会丢弃过期响应，
有无防抖都只重绘一次，**重绘次数观测不到查询次数**。已删，换成能观测的
「输入即时回显」，并注明防抖间隔为什么测不了：要测得往 `api` 层加计数器，
那是改被测代码换可观测性。

**没测防抖间隔是有意的**，`AGENTS.md` 里也记了这一条，免得后来者当成漏测。

---



## cc64440 · 2026-09-28 · feat: 阶段 2 接线，前端打通 Tauri IPC

新增 `src/lib/tauri.ts`，11 个 `invoke` 封装一一对应 `lib.rs` 里的 11 个
`#[tauri::command]`。`backend.ts` 的 `impls` 表注册 `tauri: tauriApi`，
**前端零改动**即可通过 `VITE_BACKEND=tauri` 切换数据源。

`ActionResult` 用 struct + `skip_serializing_if` 序列化为
`{ ok, value | error }`，与 `api.ts` 的 union type 对齐。

**踩坑：Tauri 2.12 的 `IpcResponse` 需要 `Deserialize`。** 只 derive
`Serialize` 会报 19 个 `Settings: IpcResponse` 未实现。

**环境障碍（已解决）：** `mint-refresh-cache` 卡死 20 天 17 小时，
攥着 apt 锁。`sudo kill -9` 清掉即可，锁是内核文件锁，进程一死立刻释放，
不用删文件。Rust 装在 `~/.rustup`，但非交互 shell 不读 `.bashrc`，
每条 cargo 命令前要 `. "$HOME/.cargo/env"`。

版本不匹配警告（Rust tauri 2.12.0 vs npm @tauri-apps/api 2.11.1）
暂未处理，IPC 实测可用。

---



## 397562a · 2026-09-28 · feat: 阶段 3 detect.rs，13 条内容识别规则

第一个 Rust 文件。纯函数、无 async、无系统调用。67 条单测，
86 条带标注真实内容准确率 100%，clippy 0 警告。

**三个 bug 都是测试逼出来的，不是读代码看出来的：**

- **base64url 字母表误用标准 base64。** 用了 `+` `/`，而 base64url 是
  `-` `_`。JWT 签名段里的 `_` 因此被拒，`jwt_valid` 一直红。
  当时 `jwt_minimal` 造的样本不含这两个字符，**它一直是绿的**——
  测试数据的运气好掩盖了实现错误。已补 `jwt_with_url_safe_chars`，
  验证过改回错字符集时它会红。
- **DDL 规则用 `contains` 吞散文。** 为了让 `CREATE TABLE t (id INT)`
  单条即命中而放宽，结果 `we will create table later` 也被判成 SQL。
  这是**自己新加的反例测试**抓到的，修完 bug 顺手补边界测试立刻又
  抓出第二个。已改成必须匹配行首。
- **异常「必须含包名前缀」挡掉 Python 异常。** `RuntimeError: msg` 没有
  包名。现在规则是「有包名限定 **或** 冒号后带 message」，
  反例 `there was an Exception yesterday`。

**两处刻意偏离 `docs/04` 的原始描述**，因为实测中原描述不成立：

- **围栏代码块单独即判 markdown**，不要求凑满 2 类。它和 code 的区别
  就在这对反引号，要求第二类会误判成 code。
- **DDL 单关键字即判 SQL**，`SELECT` 之类仍需 2 个。
  `CREATE TABLE users (...)` 全文只有一个 SQL 关键字。

**「6379」被判成 json 是正确行为**，裸数字是合法 JSON。精度测试里原本
期望 text，是标注写错了，已改。这是 JSON 规则优先级的必然结果。

`Image` 变体走非文本剪贴板 flavor，不经过 `detect(&str)`，但按 `docs/04`
保留以对齐前端 `ContentType`，标了 `#[allow(dead_code)]` 说明理由。

---



## 8e5861a · 2026-09-28 · feat: 新增 home/ 官网首页，含成长历程时间树

单文件纯 HTML，无构建、无外部依赖、无 CDN 请求，断网可开。

10 个节点按阶段划分排布，绿/蓝/灰三色圆点区分已完成 / 进行中 /
待开始。数字取自实际而非估计：85 = 67 条 Rust 单测 + 18 条 e2e，
100% = 86 条带标注样本的实际准确率。

**验证方式。** 装了 Playwright 在 `/tmp/opencode/verify`（不进仓库），
用 chromium headless 实际点一遍：四个宽度（320/390/768/1280）检查
导航完整性与横向溢出，再截图目视确认渲染。

**修掉自己埋的问题。** 第一版移动端样式有
`nav a:nth-child(n+3){display:none}`，会连带把「历程」链接也藏掉，
而时间树恰恰是这页主内容。已改成收紧间距但四条全留。

---



## 9032c3e · 2026-09-28 · docs: commit message 改为精简，复杂记录移入 DEVLOG

用户要求 commit message 精简，复杂的「为什么」存入本地文件，且要与
GitHub 上的 commit **一一对应**。

规则本身写进了全局 `~/.config/opencode/AGENTS.md`（对所有项目生效）
和项目级 `AGENTS.md`（只对 DevClip 生效）：

- 默认只写标题，确有必要时正文不超过 5 行
- 复杂内容进 `docs/DEVLOG.md`，以 commit hash 为键
- 一次提交一个主题

**为什么放 gitignore 内。** 这些记录的价值在于「事后回查为什么当时
这么写」，而不是让 clone 的人读到。放进版本库会让仓库历史里塞满
过程性叙述，而 GitHub 上的 commit 本身才是权威的变更记录。

本文件建立时已核对：DEVLOG 里的 hash 与 `git log` 完全一致，
顺序也一致。`git check-ignore` 确认它不会进版本库。

---



## e717391 · 2026-09-28 · docs: 官网成长历程改为三段式结构

用户要求成长历程聚焦「做了什么 / 实现了什么 / 提升了什么」，
而不是实现过程。

**为什么这个要求是对的。** 原来每条都在讲怎么实现的（base64url
字母表写错、DDL 规则吞散文），这些是 DEVLOG 的内容。官网读者关心
的是「这东西现在能干什么」，过程细节只对维护者有价值。

**「提升」那栏刻意落在能力或结果上**，不罗列技术：
「从『存剪贴板』变成『理解剪贴板』」「从『看』升级为『改』」
「建立『放心用它管理剪贴板』的信任基础」。

顺带把内部文件名从标题里去掉（`detect.rs` → 内容识别器、
`db.rs/repo.rs` → 持久化与搜索），技术版本仍保留在技术栈那节。

---



## fad0c5f · 2026-09-28 · docs: 导航与正文顺序对齐，末屏合并

**导航与正文顺序原本不一致。** 导航是 能力→历程→技术栈→提交，
正文是 能力→历程→进度→提交→技术栈。已统一为
能力→历程→进度→提交。

顺带修了两个真问题：

- 「当前进度」section 缺 `id`，锚点指向不存在的元素
- **末尾 section 的锚点天然失效。** 浏览器已到底，点「技术栈」只会
  停在页脚位置，看着像没反应。

**这里我走错了两次。** 起初用加内边距补滚动余量：96→200→280→400px，
锚点确实能滚到位，但技术栈和页脚之间留出 280~340px 空洞。方向
本身就错了——问题不是余量不够，而是「最后一屏无论怎么点都只能滚到底」。

正确解法是把提交记录、技术栈、页脚合并成一屏（`h2.sub` 次级标题），
页面从 5 屏变 4 屏，导航项全落在可定位位置，尾部无空洞。

**检测方式上的教训：** 期间两次用同页改 `location.hash` 测锚点，
而 `scroll-behavior: smooth` 下它根本不滚动，测出来的数据是假的
（scrollY=0）。改成 Playwright 真实点击并等 700ms 后才拿到可信数值。

---



## 7cd8a24 · 2026-09-28 · feat: 官网加 logo 与明暗主题切换

**logo 换了两次才对。** 第一版是抽象的「剪贴板 + 光标符」，
凭品类想象画的，跟 DevClip 长什么样无关。第二版画成界面缩影
（调色板窗口 + 类型色条），但那也是我编的。

**顺手把双主题的地基打好**：把散在页面里的 `#09090b`、
`#ffffff14` 等硬编码色值全抽成 CSS 变量，徽标半透明底改用
`color-mix()` 跟着主色走。

**对比度是被脚本抓出来的，不是肉眼。** 写了个检测脚本算实际
渲染的 WCAG 对比度，发现提交 hash 和页脚只有 2.5:1——暗色下几乎
读不出来。调 `--faint`/`--mute` 后两套主题都 ≥ 4.5:1。

亮色背景用 `#fafafa` 而非纯白（长时间读不刺眼），边框和色值
相应加深以保持对比。

主题脚本放在 `<head>` 首次绘制前执行，否则会先闪一下暗色再切亮色。

---



## a3e75b6 · 2026-09-28 · fix: logo 改为界面缩影（已被 50d4525 取代）

中间版本，短暂存在于 main 上。保留记录是因为它体现了 logo 的
迭代路径，且其中的一个技术点被沿用：色条必须套进板身的
`<clipPath>`，否则直角矩形会在板身圆角处溢出。

这个 bug 是 8 倍放大截图才看出来的，1:1 尺寸下完全正常。
SVG 图形必须放大验证。

---



## 50d4525 · 2026-09-28 · fix: logo 改用软件真实应用图标

用户指出任务栏那个图标可以当 logo。查 `src-tauri/icons/` 确认
就是 Tauri 脚手架默认图标，PNG 带 tRNS 透明块。

**纠正我之前一个没验证的结论。** 上一版我说这个图标「白底下会
露出黑方块背景」——错的，实测四角 `alpha=0` 全透明，没有黑方块。

但真问题确实存在，只是原因不同：**图标中央的 S 形主体是纯白**
（采样得 `[255,255,255,255]`），白底上主体会整个消失，只剩两道
弧线。所以亮色主题要垫一块 `#18181b` 深色圆角底，暗色直接用。

`128x128.png` 以 base64 内嵌（2124 字符），单文件性质没破，
零外部请求，顺手当了 favicon。旧的 SVG 几何与 8 个配色变量一并清除，
没留死代码。

---

## 114469d · 2026-09-28 · feat: 阶段 4 SQLite + FTS5，本地历史库

数据从内存 mock 换成真实 SQLite。分层：`db.rs` 管连接与 schema，
`repo.rs` 管全部 SQL，`lib.rs` 只做「取连接 → 调 repo → 返回」。
删掉了 lib.rs 里与 repo 重复的类型定义和 `sample_items()`，
文件从 300+ 行降到 230 行。

**踩坑一：trigram 搜不到两字词。** 测试搜「中文」命中 0 条——trigram
按 3 字符切分，「中文」只有 2 个字。中文里两字词极常见（会议、用户、
配置），这是硬伤。改成 `>=3` 字符走 FTS5，`<3` 字符回退 `LIKE`。
顺带给 `LIKE` 补了 `%` `_` 转义（不转义的话搜「50%」会匹配全表，
这是 LIKE 相比 MATCH 必须额外付的代价）。

**踩坑二：seed 的验证用例选错了值，一度误判成 bug。** 拿
`orders_42` 当查询词，命中 0 条。查 FTS 索引才发现内容里根本没这个串
——模板按 `i % 13` 取模，只有 `orders_1/14/27/…`。改用 `orders_14`
后正常命中 2 条（一条 SQL、一条 markdown 模板里也有）。
**不是代码问题，是我的验证没对上数据。**

**踩坑三（编译期）：**
- `format!` 会把模板里的 `{i}` 当占位符，seed 的 13 个模板里
  有一半因此编不过。改用普通字符串 + `.replace("{i}", ...)`
- `db`/`repo` 模块是私有的，`examples/seed.rs` 访问不到，
  改成 `pub mod`
- `rusqlite::Connection` 不是 `Send`，而 Tauri 的 managed state 要求
  `Send + Sync`，必须 `Mutex` 包一层
- `std::io::Error` 不会自动转 `rusqlite::Error`，`create_dir_all`
  那里得显式包一层 `DbError::Io`

**顺带做的两件事：**

- **`store.ts` 补了错误处理。** 阶段 4 起 Rust 返回 `Err(String)`，
  之前 rejected promise 会被静默吞掉——数据库出错时界面一片空白，
  用户只看到「搜不出来」。现在 `attempt()` 接住并把后端的中文提示
  显示在状态栏。这里我第一版写了个根本没执行传入函数的 `attempt`，
  是 tsc 也没能发现的那种蠢错（`pnpm build` 过了但逻辑是空的），
  重读代码时才发现。
- **`tauri.ts` 补 `tauriDb.add()`**，给阶段 5 的剪贴板监听用，
  刻意不放进 `ClipboardApi`——那是历史读写的契约，入库是另一回事。

**性能验收（1000 条，验收线 50ms）：**

```
入库 1320ms   子串 ser 3.05ms   精确 orders_14 3.82ms
子串 oduction 4.35ms   两字中文 LIKE 2.94ms   全量 13.11ms
类型过滤 sql 1.33ms
```

**库位置** `~/.local/share/com.devclip.app/devclip.db`，WAL 模式。
`cargo run --example seed` 灌 1000 条假数据。注意 `--release` 首次
构建会超过 15 分钟，用 debug 跑就够。

---


---



---

## 2da5db1 · 2026-09-29 · chore: Playwright 降到 1.57.0 以支持 macOS 13

1.58 起官方不再为 macOS 13 构建浏览器，`playwright install` 直接报
does not support chromium on mac13。1.57.0 是最后支持该系统的版本，
18 条 e2e 在 macOS 13.6.9 上全通过。

**连带影响**：浏览器 build 号从 1099 变 1200，本机
`~/.cache/ms-playwright` 里的旧 chromium 失效，`pnpm test` 会 18 条
全挂在 `Executable doesn't exist`。重跑 `pnpm exec playwright install
chromium` 即可，不是代码问题。

---

## a7e66de · 2026-09-29 · chore: 对齐 Tauri JS 包版本到 2.12.0

Rust 侧 `tauri = "2"` 解析到 2.12.0，JS 侧还停在 2.11.1。开发时一直
有版本不匹配的告警，两边对齐后消失。

---

## a1fb817 · 2026-09-29 · feat: macOS 剪贴板监听、全局快捷键与回前台粘贴

阶段 5 的主体。新增 `clipboard.rs`（334 行）。

**用 arboard 而不是自己写 NSPasteboard**：跨平台、无 unsafe，
而且 macOS 上读剪贴板不需要任何授权（TCC 只拦辅助功能与
Apple Events），所以前端权限表也不用加东西。

**踩坑一：读前台应用不能用 osascript。** 早先走
`tell application "System Events"`，那会触发 TCC 的「自动化」授权
弹窗。弹窗不出现时 osascript 一直等，而调用方当时**持着数据库锁**
—— 整个应用假死（实测复现）。换成 `NSWorkspace` 的进程内调用，
微秒级返回。`frontmost_app_never_blocks` 那条测试就是防这个回归的。

**踩坑二：模拟按键要走 osascript，不是 CGEvent。** CGEvent 在没有
辅助功能授权时是**静默丢弃**的 —— 用户只看到光标闪一下，没有任何
提示。osascript 会以 `-1719` 明确退出，能翻译成「去 系统设置 →
隐私与安全性 → 辅助功能 勾选 DevClip」。

但 osascript 可能卡在 TCC 弹窗上等一个永远不会来的回答，所以外面
包了 3 秒硬超时 + kill，否则每次粘贴失败都会留下一个僵尸进程。

**降级路径**：快捷键注册不上（被占或系统不给权限）就把窗口改为
常驻显示。这时用户如果还进不来，就只剩杀进程重装 —— 比多弹一个
窗口糟糕得多。

---

## 7e53b1b · 2026-09-29 · feat: 前端订阅剪贴板变化并接入窗口收起

`ClipboardApi` 加 `subscribe(onChanged): () => void`，store 在
`init` 里订阅一次。回调**不带载荷**，只触发 refresh —— 前端不关心
是哪条内容入库的。

**顺手修的真 bug**：`backend.ts` 里写死 `import.meta.env.VITE_BACKEND
|| "mock"`，结果 `pnpm tauri dev` 起来的真应用也走 mock，Rust 那套
剪贴板和数据库一行都不执行。靠 .env 也不合适 —— e2e 跑在浏览器里，
一旦全局设成 tauri，所有 invoke 都会 reject。改成运行时探测
`isTauri()`，两种场景才对。

---

## ea56af7 · 2026-09-29 · feat: 敏感内容不入库，修复同一内容重复复制不计数

补上两处远端缺的。

**敏感扫描**（docs/07 的 P0）。剪贴板历史是**明文存盘**的，用户
粘进来的 API key 半年后还能搜出来。新增 `sensitive.rs`，插在
`capture()` 之前 —— 入完库再判等于没判，密钥已经落到磁盘上了。
也不 emit：前端收到事件会刷新列表，而列表里并没有新东西。

规则只做**明确无疑**的（私钥块、AKIA+16、ghp_+36、sk_live_、
xoxb-）。宁可漏报也不误报 —— 把一段普通 SQL 判成密钥，
用户会立刻关掉这个功能。

`has_token` 第一版只判了 `prefix.len() < len`，结果复制一个 4 字符
的 `"AKIA"` 就 panic 在切片上 —— 而这条路径跑在监听线程里，
一个 panic 意味着监听整个死掉。`short_input_does_not_panic`
这条测试就是防它的。

**去重**。原来只比内容，且记录只在剪贴板被清空时重置，于是
**连续两次复制同一段内容**时第二次被误判成重复，`copy_count`
永远停在 1。而「从历史复制一条命令、改一改、再复制一次」
恰恰是常见操作。

改成带时刻的静默期去抖。中间踩了一次坑：第一版只在内容变化时
更新时间戳，于是最快也是每 800ms 记一次，按住 Ctrl+C 十秒把
`copy_count` 刷到 11。**每次观察都推进窗口**才对 ——
按住不放就一直被判重复，安静超过一个窗口才重新计入。

---

## 92827fe · 2026-09-29 · feat: 快捷键持久化、分平台默认值与冲突预判

早先每次启动都用硬编码默认值，用户改了也没用 —— 设置项写得再
清楚也没意义。新增 `settings.rs`（原子写：先写临时文件再改名，
否则写到一半被杀会留下半个 JSON），加 `set_hotkey` /
`get_hotkey` 两个命令。

**默认值改为分平台**。docs/05 的警告是 `⌘⇧V` 是个坏默认值
（macOS 的 "Paste and Match Style"，Chrome/Slack/VSCode/JetBrains
都占着），而一个值套三个平台必有一个撞车。

**没有重写解析器。** 插件的 `Shortcut` 已经实现了 `FromStr` 和
`Display`，规范串是 `shift+alt+v` 这样修饰键顺序固定的格式。
冲突比对走 `HotKey::id()` —— 它由 `(mods.bits() << 16) | key`
算出，修饰键顺序无关，所以 `Shift+Cmd+V` 和 `Cmd+Shift+V` 天然相等。

**冲突表带原因而不只是「冲突了」**：用户需要知道**被谁占了**。
而且必须分平台 —— `Super+Shift+V` 在 macOS 冲突，在 Linux 上
恰恰是 docs/05 推荐的默认值。

---

## a70abb1 · 2026-09-29 · ci: 三平台矩阵测试与 macOS 真机检查

**删掉了一个从没跑通过的 job。** 原本想用
`cargo check --target x86_64-apple-darwin` 在 Linux 上交叉检查
macOS 代码，实际过不了：tauri 拉入 `objc2-exception-helper`，
它要编译 `.m` 文件，需要支持 `-arch x86_64` 的 C 编译器，
Linux 上没有，加 clang 也救不回来（还得有 macOS SDK 的
`objc/runtime.h`）。

我之前被一个**独立探针**误导过 —— 不带 tauri 的纯 objc2 确实能
交叉编译，于是得出「可以交叉检查」的结论。那个结论对真实 crate
不成立。留着这个 job 只会给一种从没生效的检查虚假的安心感。

`check-macos` 用真机 SDK 编译全部 target。注释里写明它只能验证
类型与 API 用法，运行时的剪贴板监听与授权检测仍然只有装了应用、
授权过的机器上才验得了。

`examples/seed.rs` 其实三个平台都能编（路径逻辑是运行时的），
原先注释里说它会编译失败是不准确的，已改掉。

---

## b597f44 · 2026-09-29 · feat: Linux X11 剪贴板监听、接管与模拟粘贴

X11 与 macOS 的机制完全不同。macOS 有 `changeCount`，改动就 +1，
轮询它即可。**X11 走 Selection 机制：没有计数器可轮询**，只有一个
CLIPBOARD selection 和它的持有者窗口。两个直接后果：

1. 只能靠持有者变化判断「有没有新内容」
2. **持有者进程一死内容就没了**（docs/05 风险 4）—— 所以剪贴板
   管理器不只是「读」，还必须**接管**，自己变成持有者并一直应答
   别人的请求。这也正是它必须常驻托盘的原因

**先写探针把事实摸清**，几条不看文档猜不到的：

- **CLIPBOARD 不是 X 预定义的原子**（PRIMARY=1、SECONDARY=2 才是），
  必须 `InternAtom` 按名问。本机值 438。写 `AtomEnum::CLIPBOARD`
  编译不过
- arboard 的 `write()` 是**先接管再等**，不是反过来
- `set().wait()` 在内容被覆盖时**自己返回**。这条很关键：它意味着
  「我们已经不再是持有者」是**可精确通知**的（`JoinHandle::is_finished`），
  不用去猜自己的窗口 ID
- arboard 有**进程级全局单例**，同进程内所有 `Clipboard::new()`
  **共享同一个 X11 窗口**

最后一条让我最初的验收脚本只拿到 3/5：源程序和监听器在同一进程里，
写入的 owner 窗口号前后相同，轮询根本看不见变化。**验收必须用独立
进程模拟「用户在别的程序里复制」**，否则测的是一个不存在的场景。

**监听用「owner 变化 + 每 2 秒兜底读」混合。** 纯 owner 轮询会漏掉
两种都很常见的情况：编辑器原地改内容再复制（owner 不变），以及系统
已装了别的剪贴板管理器时代替所有应用持有（owner 恒定）。

**两个 bug：**

- **字节序。** `_NET_ACTIVE_WINDOW` 是 `format=32`，x11rb 给的是
  **小端**字节，我写了 `from_be_bytes`，窗口号解析成 `0x600e003`
  而不是 `0x3e00006`。症状是 `source_app` 永远为空且不报错。
  已修 + 加测试
- **中文乱码。** `window_name` 逐字节 `as char` 没按 UTF-8 解码，
  `项目进度确认` 变成 `é¡¹ç»®è¿åº¦ç¡®è¤`

**一处诚实修正。** 我一度以为「DevClip 退出后剪贴板内容仍在」证明
接管生效。查了发现不是 —— arboard 析构时会
`ask_clipboard_manager_to_request_our_data()` 把数据交给**系统自带的
剪贴板管理器**（Cinnamon 有）。换到没装剪贴板管理器的机器上不会这样。
已验证的只是「源程序退出后内容仍在」，那才是接管的功劳。

真机验收 5/5 捕获，密钥被拦下，检测延迟约 0.5s。

---

## f708440 · 2026-09-29 · feat: Linux 与 macOS 功能对齐，加托盘与粘贴降级

**最大的一处是设计错配。** macOS 的 `frontmost_app()` 返回
bundle id（`com.apple.Safari`，稳定唯一），`activate` 靠它精确定位。
我原先让 Linux 返回**窗口标题**再靠标题匹配回去 —— 而标题会变、
可能重复，更要命的是调色板抢到焦点时记下的就是「DevClip」。
改成返回 **X11 窗口 ID**，用 `_NET_ACTIVE_WINDOW` 客户端消息精确激活
（带 `data[0]=2` 的源码指示，不带它不少 WM 会直接忽略）。

补的三处：

- **排除自己作为 `source_app`。** 调色板显示时 DevClip 就是前台窗口，
  不排除的话每条历史都写着「DevClip」，一个有用的字段就废了
- **托盘图标。** X11 上这是**可用性前提**不是装饰：内容在持有者进程
  里，进程一退就没了，所以必须常驻，而「怎么常驻」只能靠托盘 ——
  没有它用户唯一能做的就是杀进程，那恰好是最容易丢数据的操作。
  Tauri 2 把托盘内建进 `tauri::tray` 了，不需要 `tauri-plugin-tray`
  （那个 crate 在索引里已经不存在）
- **粘贴降级路径。** docs/05 风险 2 明确要求「不能让功能直接死掉」。
  模拟按键失败时改成「已复制到剪贴板，请手动按 Ctrl+Shift+V」并把
  窗口弹回来，而不是报错了事。状态加了 `warn` 一档 —— 降级不是故障，
  不该用和错误一样的语气

窗口从 800x600 带边框改成 680x480 无边框。X11 下 `decorations` 不设
默认有标题栏，而剪贴板调色板不该有最小化/关闭按钮。

**没验到的：** 全局快捷键在这个环境里无法自动验证 —— 这个 X 会话
没有活动窗口（`xdotool getactivewindow` 返回空），合成按键无处可发。
托盘图标创建无报错，但点不开验证。
