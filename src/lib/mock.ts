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

const db: ClipboardItem[] = [
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
];

/** 工具箱动作表 —— 对应 docs/04-内容识别与工具箱.md，阶段 6 移到 Rust 侧 */
const ACTIONS: Record<ContentType, ToolboxAction[]> = {
  json: [
    { id: "json.format", label: "Format" },
    { id: "json.minify", label: "Minify" },
    { id: "json.sort_keys", label: "Sort Keys" },
    { id: "copy", label: "Copy" },
  ],
  jwt: [
    { id: "jwt.decode_header", label: "Decode Header" },
    { id: "jwt.decode_payload", label: "Decode Payload" },
    { id: "jwt.verify_exp", label: "Verify Exp" },
  ],
  sql: [
    { id: "sql.format", label: "Format" },
    { id: "sql.tables", label: "Extract Tables" },
    { id: "sql.upper_kw", label: "Upper Keywords" },
  ],
  base64: [
    { id: "base64.decode", label: "Decode" },
    { id: "base64.encode", label: "Encode" },
  ],
  uuid: [
    { id: "uuid.upper", label: "Upper" },
    { id: "uuid.lower", label: "Lower" },
    { id: "uuid.no_dash", label: "Remove Dashes" },
  ],
  url: [
    { id: "url.open", label: "Open in Browser" },
    { id: "url.strip_query", label: "Copy without Query" },
    { id: "url.domain", label: "Extract Domain" },
  ],
  code: [{ id: "code.wrap_selinux", label: "Wrap" }],
  text: [{ id: "text.trim", label: "Trim" }],
  ip: [{ id: "ip.copy", label: "Copy" }],
  exception: [{ id: "exception.first_frame", label: "First Frame" }],
  commit: [{ id: "commit.copy", label: "Copy" }],
  markdown: [{ id: "markdown.outline", label: "Outline" }],
  image: [{ id: "image.save", label: "Save As…" }],
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
    if (it) await navigator.clipboard?.writeText(it.content).catch(() => {});
    return delay(undefined);
  },

  async paste(id) {
    const it = db.find((i) => i.id === id);
    if (it) {
      await navigator.clipboard?.writeText(it.content).catch(() => {});
      it.copyCount += 1;
      it.lastCopiedAt = Date.now();
    }
    return delay(undefined);
  },

  async availableActions(t) {
    return delay(ACTIONS[t] ?? [{ id: "copy", label: "Copy" }]);
  },

  async runToolboxAction(_id, actionId): Promise<ActionResult> {
    return delay({
      ok: false,
      error: `mock 后端：动作 "${actionId}" 尚未实现，将在阶段 6 落到 Rust 侧。`,
    });
  },

  async getSettings() {
    return delay({ ...settings });
  },

  async setSettings(patch) {
    settings = { ...settings, ...patch };
    return delay({ ...settings });
  },
};
