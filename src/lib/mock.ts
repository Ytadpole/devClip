/**
 * Mock 后端 —— 阶段 1 使用
 *
 * 目标：在完全不装 Rust 的情况下，把搜索 / 过滤 / 键盘导航 / 收藏的
 * 交互全部调通。数据存在内存里，刷新即重置。
 *
 * 阶段 2 会新增 src/lib/tauri.ts 实现同一个 ClipboardApi 接口，
 * 然后 backend.ts 里换一行导出即可，上层代码零改动。
 */

import type {
  ActionResult,
  ClipboardApi,
  ClipboardItem,
  ContentType,
  Query,
  Settings,
  ToolboxAction,
} from "./api";

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;
const now = Date.now();

/** 模拟一条条记录，模拟 Rust 端 detect + 去重后的入库结果 */
function item(
  id: number,
  content: string,
  contentType: ContentType,
  ago: number,
  opts: Partial<ClipboardItem> = {},
): ClipboardItem {
  return {
    id,
    content,
    contentType,
    preview: content.replace(/\s+/g, " ").slice(0, 160),
    byteSize: new TextEncoder().encode(content).length,
    copyCount: 1,
    createdAt: now - ago,
    lastCopiedAt: now - ago,
    favorite: false,
    sensitive: false,
    ...opts,
  };
}

/**
 * 图片占位。阶段 1 没有真实图片文件，内联一张 SVG 就够 ——
 * 这样 image 类型的行能真的渲染出缩略图。
 */
function shot(w: number, h: number, bg: string, label: string): string {
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}">` +
    `<rect width="100%" height="100%" fill="${bg}"/>` +
    `<text x="50%" y="50%" dy=".35em" text-anchor="middle" font-family="monospace" ` +
    `font-size="${Math.round(h / 4)}" fill="#ffffff" opacity=".9">${label}</text>` +
    `</svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

const SEED: ClipboardItem[] = [
  item(1, '{"name":"andy","age":18,"tags":["dev","rust"],"active":true}', "json", 2 * MIN, {
    favorite: true,
    copyCount: 3,
    sourceApp: "VS Code",
  }),
  item(2, "select * from users where id = 1", "sql", 5 * MIN, { sourceApp: "DataGrip" }),
  item(3, "https://github.com/tauri-apps/tauri", "url", 10 * MIN, { sourceApp: "Firefox" }),
  item(
    4,
    "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkFuZHkiLCJpYXQiOjE3MDAwMDAwMDB9.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c",
    "jwt",
    18 * MIN,
    { favorite: true, sourceApp: "Postman", copyCount: 2 },
  ),
  item(5, "docker ps -a --format 'table {{.Names}}\\t{{.Status}}'", "code", 24 * MIN, {
    sourceApp: "Windows Terminal",
  }),
  item(6, "6379", "text", 31 * MIN, { sourceApp: "Navicat" }),
  item(7, "redis-cli -h 10.0.3.17 -p 6379 MONITOR", "code", 42 * MIN, {
    favorite: true,
    sourceApp: "Windows Terminal",
  }),
  item(8, "3f8a9b2c-1d4e-4f6a-8b7c-2e5d9f0a3b1c", "uuid", 55 * MIN, { sourceApp: "IntelliJ IDEA" }),
  item(9, "10.0.3.17", "ip", 1 * HOUR, { sourceApp: "Xshell" }),
  item(
    10,
    "org.springframework.dao.QueryTimeoutException: Could not execute JDBC Query\n\tat org.springframework.orm.jpa.vendor.HibernateJpaDialect.convert(HibernateJpaDialect.java:275)\n\tat com.devclip.service.UserRepository.findById(UserRepository.java:42)",
    "exception",
    1.2 * HOUR,
    { sourceApp: "IntelliJ IDEA" },
  ),
  item(11, "aGVsbG8gZGV2Y2xpcA==", "base64", 1.5 * HOUR, { sourceApp: "Terminal" }),
  item(12, "kubectl get pods -n production -o wide", "code", 2 * HOUR, {
    favorite: true,
    sourceApp: "Windows Terminal",
  }),
  item(13, "export DATABASE_URL=postgres://user:pass@localhost:5432/devclip", "code", 3 * HOUR, {
    sensitive: true,
    sourceApp: "Terminal",
  }),
  item(14, "8f3e2a1b9c4d7e6f5a0b", "commit", 4 * HOUR, { sourceApp: "GitHub" }),
  item(15, "# DevClip\n\n## Roadmap\n- [x] clipboard history\n- [ ] toolbox", "markdown", 5 * HOUR, {
    sourceApp: "Obsidian",
  }),
  item(16, "BEGIN RSA PRIVATE KEY-----", "text", 6 * HOUR, { sensitive: true, sourceApp: "Terminal" }),
  item(17, "update users set email = ? where created_at < ?", "sql", 8 * HOUR, {
    copyCount: 5,
    sourceApp: "DataGrip",
  }),
  item(18, "npm install -g pnpm && pnpm create tauri-app", "code", 10 * HOUR, {
    sourceApp: "Windows Terminal",
  }),
  item(19, "192.168.1.100:5432", "text", 12 * HOUR, { sourceApp: "Navicat" }),
  item(20, '["docker","kubernetes","redis"]', "json", 1 * DAY, { sourceApp: "VS Code" }),
  item(21, "{\n  \"compilerOptions\": {\n    \"strict\": true\n  }\n}", "json", 1.3 * DAY, {
    sourceApp: "VS Code",
  }),
  item(22, "docker compose -f docker-compose.dev.yml up -d --build", "code", 1.5 * DAY, {
    sourceApp: "Windows Terminal",
  }),
  item(23, "https://registry.npmmirror.com", "url", 2 * DAY, { sourceApp: "Firefox" }),
  item(24, "DELETE FROM sessions WHERE expired_at < NOW()", "sql", 3 * DAY, {
    sourceApp: "DataGrip",
  }),
  // 图片：docs/06 要求 mock 里也得有若干张
  item(25, "[图片] 500-internal-error.png", "image", 30 * MIN, {
    imagePath: shot(320, 180, "#b91c1c", "500"),
    byteSize: 184_320,
    copyCount: 2,
    sourceApp: "Firefox",
  }),
  item(26, "[图片] docker-ps.png", "image", 2.4 * HOUR, {
    imagePath: shot(480, 270, "#1d4ed8", "ps"),
    byteSize: 92_160,
    sourceApp: "Windows Terminal",
  }),
  item(27, "[图片] 架构图 v3.png", "image", 9 * HOUR, {
    imagePath: shot(400, 400, "#047857", "v3"),
    byteSize: 340_992,
    favorite: true,
    sourceApp: "Obsidian",
  }),
  item(28, "[图片] screenshot-2026-09-26.png", "image", 1.4 * DAY, {
    imagePath: shot(360, 640, "#7c3aed", "9/26"),
    byteSize: 512_000,
    sourceApp: "Flameshot",
  }),
];

/**
 * 批量数据。
 *
 * docs/06 要求「超过 50 条必须虚拟化」，但 28 条手写数据在 420px
 * 视口里只占 7 行，压根触发不了窗口滚动，验收时看不出虚拟化到
 * 底有没有生效。这里补到 148 条，模板覆盖全部 13 种类型，
 * 按类型筛选也拉不满。
 */
const FILLER_TEMPLATES: Array<[ContentType, string]> = [
  ["json", '{"service":"api-{n}","replicas":3,"region":"cn-north-1"}'],
  ["sql", "select id, email from orders_{n} where status = $1 limit 50"],
  ["code", "kubectl rollout restart deployment/api-{n} -n production"],
  ["url", "https://internal.example.com/dashboards/{n}?range=24h"],
  ["uuid", "b7d2f4a1-9c3e-4f80-8a1d-{n}e6c93b52"],
  ["jwt", "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyLW4yJ9.sig-{n}"],
  ["ip", "172.16.{o}.24"],
  ["base64", "RGV2Q2xpcCBib2NrdW1lbnQge24ge249Cg=="],
  ["markdown", "## PR #{n}\n\n- [x] 补测试\n- [ ] 合入 release"],
  ["commit", "{n}f31c9d2b7e4567819ac0de1234567890abcd"],
  [
    "exception",
    "java.lang.IllegalStateException: bean not ready (attempt {n})\n\tat com.example.Boot.run(Boot.java:88)",
  ],
  ["text", "会议室 B-{n} 改到周四 10:00"],
  ["image", "[图片] shot-{n}.png"],
];

const FILLER_APPS = [
  "VS Code",
  "Firefox",
  "DataGrip",
  "Windows Terminal",
  "IntelliJ IDEA",
  "Postman",
  "Obsidian",
];

const FILLER_TINTS = ["#0f766e", "#4338ca", "#a16207", "#be123c", "#15803d"];

const FILLERS: ClipboardItem[] = Array.from({ length: 120 }, (_, k) => {
  const [t, tpl] = FILLER_TEMPLATES[k % FILLER_TEMPLATES.length];
  const n = String(1000 + k);
  // {o} 专供 IP 的第三段，保证还是合法的 0-255
  const content = tpl.replace(/\{n\}/g, n).replace(/\{o\}/g, String((k % 250) + 2));
  const opts: Partial<ClipboardItem> = {
    copyCount: (k % 4) + 1,
    sourceApp: FILLER_APPS[k % FILLER_APPS.length],
  };
  if (t === "image") {
    opts.imagePath = shot(220, 150, FILLER_TINTS[k % FILLER_TINTS.length], n);
    opts.byteSize = 40_000 + (k % 9) * 30_000;
  }
  return item(100 + k, content, t, (k + 1) * 7 * MIN, opts);
});

const db: ClipboardItem[] = [...SEED, ...FILLERS];

/**
 * 动作列表。必须与 `src-tauri/src/toolbox/mod.rs` 的 ENTRIES 对齐。
 *
 * 以前这里是各写各的：Rust 侧返回 Format / Minify，mock 侧返回
 * Format / Minify / Sort Keys，看起来「差不多」，于是 e2e 跑的是
 * 一套假的动作列表，而真应用里点「Sort Keys」会得到「没有这个动作」。
 * 对齐的成本是每次加动作改两处，值得
 */
const ACTIONS: Record<ContentType, ToolboxAction[]> = {
  json: [
    { id: "json.format", label: "美化", hint: "2 空格缩进" },
    { id: "json.format4", label: "美化 (4 空格)", hint: "4 空格缩进" },
    { id: "json.minify", label: "压缩", hint: "压成一行" },
  ],
  jwt: [
    { id: "jwt.decode_header", label: "解 Header", hint: "base64 不是加密" },
    { id: "jwt.decode_payload", label: "解 Payload", hint: "base64 不是加密" },
    { id: "jwt.verify_exp", label: "检查过期", hint: "只读 exp，不验签" },
  ],
  base64: [
    { id: "b64.decode", label: "解码" },
    { id: "b64.decode_urlsafe", label: "解码 (url-safe)", hint: "字母表 -_ 而非 +/" },
    { id: "b64.encode", label: "编码", hint: "任意文本 → Base64" },
    { id: "b64.encode_urlsafe", label: "编码 (url-safe)", hint: "字母表 -_ 而非 +/" },
  ],
  sql: [
    { id: "sql.format", label: "格式化", hint: "关键字大写 + 换行" },
    { id: "sql.upper", label: "关键字大写" },
    { id: "sql.lower", label: "关键字小写" },
    { id: "sql.tables", label: "提取表名", hint: "只解析，不连库" },
  ],
  url: [
    { id: "url.strip_query", label: "去掉 query" },
    { id: "url.domain", label: "提取域名" },
  ],
  uuid: [
    { id: "uuid.upper", label: "转大写" },
    { id: "uuid.lower", label: "转小写" },
    { id: "uuid.no_dashes", label: "去横线", hint: "MySQL bin(16) 用这个" },
  ],
  // text / code 挂的是 base64 编码类动作 —— 与 Rust 侧一致
  text: [
    { id: "b64.encode", label: "编码", hint: "任意文本 → Base64" },
    { id: "b64.encode_urlsafe", label: "编码 (url-safe)", hint: "字母表 -_ 而非 +/" },
  ],
  code: [
    { id: "b64.encode", label: "编码", hint: "任意文本 → Base64" },
    { id: "b64.encode_urlsafe", label: "编码 (url-safe)", hint: "字母表 -_ 而非 +/" },
  ],
  ip: [],
  commit: [],
  exception: [],
  markdown: [],
  image: [],
};

/**
 * mock 侧的最小实现。真的变换在 Rust 里，这里只挑几个纯 JS 能做的，
 * 让浏览器模式下工具条不是死的。
 *
 * 没实现的动作返回「这个后端没实现」而不是崩掉：mock 的定位是
 * 调交互，不是复刻后端
 */
const MOCK_RUNNERS: Record<string, (s: string) => string> = {
  "json.format": (s) => JSON.stringify(JSON.parse(s), null, 2),
  "json.format4": (s) => JSON.stringify(JSON.parse(s), null, 4),
  "json.minify": (s) => JSON.stringify(JSON.parse(s)),
  "b64.encode": (s) => btoa(String.fromCharCode(...new TextEncoder().encode(s))),
  "b64.encode_urlsafe": (s) =>
    MOCK_RUNNERS["b64.encode"](s).replace(/\+/g, "-").replace(/\//g, "_"),
  "b64.decode": (s) => new TextDecoder().decode(Uint8Array.from(atob(s), (c) => c.charCodeAt(0))),
  "uuid.upper": (s) => s.trim().toUpperCase(),
  "uuid.lower": (s) => s.trim().toLowerCase(),
  "uuid.no_dashes": (s) => s.trim().replace(/-/g, ""),
  "url.domain": (s) => {
    try {
      const h = new URL(s.trim()).hostname;
      return h.replace(/^www\./, "");
    } catch {
      return s.trim();
    }
  },
  "url.strip_query": (s) => {
    const i = s.indexOf("?");
    return i < 0 ? s : s.slice(0, i);
  },
  "sql.upper": (s) =>
    s.replace(/\b(select|from|where|join|insert|into|values|update|set|delete|order|group|by)\b/gi,
      (w) => w.toUpperCase()),
  "sql.lower": (s) =>
    s.replace(/\b(select|from|where|join|insert|into|values|update|set|delete|order|group|by)\b/gi,
      (w) => w.toLowerCase()),
};

let settings: Settings = {
  hotkey: "Alt+Shift+V",
  maxItems: 1000,
  retentionDays: 30,
  maxImageBytes: 10 * 1024 * 1024,
  theme: "dark",
  sensitiveAutoExpire: true,
};

const delay = <T,>(v: T, ms = 40): Promise<T> => new Promise((r) => setTimeout(() => r(v), ms));

export const mockApi: ClipboardApi = {
  async list(q: Query) {
    let out = [...db];

    // 与 Rust 的 repo::list 对齐：敏感项默认不参与任何结果，
    // 展开「含敏感」分组（includeSensitive）才可见
    if (!q.includeSensitive) out = out.filter((i) => !i.sensitive);
    if (q.favoriteOnly) out = out.filter((i) => i.favorite);
    if (q.types?.length) out = out.filter((i) => q.types!.includes(i.contentType));
    if (q.since) out = out.filter((i) => i.lastCopiedAt >= q.since!);

    const text = q.text?.trim().toLowerCase();
    if (text) {
      out = out.filter(
        (i) =>
          i.content.toLowerCase().includes(text) ||
          (i.sourceApp ?? "").toLowerCase().includes(text),
      );
    }

    out.sort((a, b) => {
      if (a.favorite !== b.favorite) return a.favorite ? -1 : 1;
      return b.lastCopiedAt - a.lastCopiedAt;
    });

    return delay(q.limit ? out.slice(0, q.limit) : out);
  },

  async get(id) {
    return delay(db.find((i) => i.id === id) ?? null);
  },

  async toggleFavorite(id) {
    const it = db.find((i) => i.id === id);
    if (!it) return false;
    it.favorite = !it.favorite;
    return delay(it.favorite);
  },

  async remove(ids) {
    for (let i = ids.length - 1; i >= 0; i--) {
      const k = db.findIndex((x) => x.id === ids[i]);
      if (k >= 0) db.splice(k, 1);
    }
    return delay(undefined);
  },

  async clearAll() {
    db.length = 0;
    return delay(undefined);
  },

  async copyToClipboard(id) {
    const it = db.find((i) => i.id === id);
    // 图片没有真实文件，往剪贴板里写文件名只会误导
    if (it && it.contentType !== "image") {
      await navigator.clipboard?.writeText(it.content).catch(() => {});
    }
    return delay(undefined);
  },

  async paste(id) {
    const it = db.find((i) => i.id === id);
    if (it) {
      if (it.contentType !== "image") {
        await navigator.clipboard?.writeText(it.content).catch(() => {});
      }
      it.copyCount += 1;
      it.lastCopiedAt = Date.now();
    }
    return delay(undefined);
  },

  async availableActions(t) {
    return delay(ACTIONS[t] ?? []);
  },

  async runToolboxAction(id, actionId): Promise<ActionResult> {
    const run = MOCK_RUNNERS[actionId];
    if (!run) {
      return delay({
        ok: false,
        error: `mock 后端没有实现「${actionId}」，用 pnpm tauri dev 看真的`,
      });
    }
    const item = db.find((x) => x.id === id);
    if (!item) return delay({ ok: false, error: `mock 后端：第 ${id} 条不存在` });
    try {
      // 与 Rust 侧同约定：结果「写回剪贴板」，返回的是一句摘要。
      // 摘要的措辞也照着 Rust 那边的形状写（label + 体积 + 去向），
      // 这样同一句断言在两个后端下都成立
      const value = run(item.content);
      const label = ACTIONS[item.contentType].find((a) => a.id === actionId)?.label ?? actionId;
      return delay({
        ok: true,
        value: `mock：${label} ${value.length} 字符，已复制到剪贴板（mock 不真的写）`,
      });
    } catch (e) {
      return delay({ ok: false, error: `mock：${e instanceof Error ? e.message : String(e)}` });
    }
  },

  async getSettings() {
    return delay({ ...settings });
  },

  async setSettings(patch) {
    settings = { ...settings, ...patch };
    return delay({ ...settings });
  },

  async setHotkey(accel) {
    // 没有真的注册这回事，浏览器里没有全局快捷键可占。
    // 存下来让设置页回显即可
    settings = { ...settings, hotkey: accel };
    return delay(accel);
  },

  subscribe() {
    // 没有真实剪贴板，也就没有事件可听。空函数即可 ——
    // 界面本来就该在没有新内容时保持原样
    return () => {};
  },
};
