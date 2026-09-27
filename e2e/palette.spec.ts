import { test, expect, type Page } from "@playwright/test";

/**
 * 阶段 1 的端到端验收。
 *
 * 这里同时承担两件事：一是把交互点一遍，二是钉住几条容易在
 * 后续重构里悄悄坏掉的契约 —— 主要是虚拟列表的几何和
 * 「选中项必须留在 DOM 里」那条（见 docs/06 阶段 1 偏离清单）。
 */

/**
 * 行高，与 src/components/VirtualList.tsx 的 ROW_H 一致。
 *
 * 刻意不从应用里 import：验收要钉住当前契约，真导入的话
 * 改了 ROW_H 测试会跟着变，回归就悄悄溜过去了。
 */
const ROW_H = 62;

/** Command.List 上的 p-1.5 */
const PAD_Y = 6;

interface Row {
  value: string;
  h: number;
  /** 在内容坐标系里的顶端位置 */
  content: number;
  /** 由 content 反推的行号 */
  idx: number;
  misaligned: boolean;
  offscreen: boolean;
  copyCount: number;
}

interface RowInfo {
  count: number;
  selected: string[];
  rows: Row[];
}

/** 一次读回列表的全部几何信息，省得反复 evaluate */
function readRows(page: Page): Promise<RowInfo> {
  return page.evaluate(
    ({ ROW_H, PAD_Y }) => {
      const list = document.querySelector("[cmdk-list]")!;
      const lr = list.getBoundingClientRect();
      const st = list.scrollTop;
      return {
        count: document.querySelectorAll("[cmdk-item]").length,
        selected: [...document.querySelectorAll('[cmdk-item][aria-selected="true"]')].map((el) =>
          el.getAttribute("data-value")!,
        ),
        rows: [...document.querySelectorAll("[cmdk-item]")].map((el) => {
          const r = el.getBoundingClientRect();
          const content = r.top - lr.top + st - PAD_Y;
          return {
            value: el.getAttribute("data-value")!,
            h: r.height,
            content,
            idx: Math.round(content / ROW_H),
            misaligned: Math.abs(content - Math.round(content / ROW_H) * ROW_H) > 0.5,
            offscreen: r.bottom < lr.top || r.top > lr.bottom,
            copyCount: Number((el.textContent!.match(/×(\d+)/) ?? [, "1"])[1]),
          };
        }),
      };
    },
    { ROW_H, PAD_Y },
  );
}

const setScroll = (page: Page, top: number) =>
  page.evaluate((t) => {
    document.querySelector("[cmdk-list]")!.scrollTop = t;
  }, top);

/**
 * 状态栏。断言必须用 locator（自带重试）：直接读 textContent 是
 * 一次快照，而 mock 有 40ms 延迟，很容易读到还没更新的值。
 * 另外别用 div.border-t > span 去定位，工具箱那条也是 border-t。
 */
const statusBar = (page: Page) => page.locator(".text-emerald-400, .text-amber-400").first();

test.beforeEach(async ({ page }) => {
  // mock 是内存态，重新加载页面才能保证每个用例从同一起点开始
  await page.goto("/");
  await page.waitForSelector("[cmdk-item]");
});

test.describe("虚拟列表", () => {
  test("148 条只渲染窗口内的少量行", async ({ page }) => {
    const { count } = await readRows(page);
    expect(count).toBeLessThan(40);
  });

  test("行高一致且都落在行网格上", async ({ page }) => {
    const { rows } = await readRows(page);
    expect(rows.length).toBeGreaterThan(0);
    for (const r of rows) expect(r.h).toBe(ROW_H);
    for (const r of rows) expect(r.misaligned, `第 ${r.idx} 行错位`).toBe(false);
  });

  test("滚到底时末行紧贴底边", async ({ page }) => {
    await page.evaluate(() => {
      const l = document.querySelector("[cmdk-list]")!;
      l.scrollTop = l.scrollHeight;
    });
    // 得等窗口真的挪到末尾：刚设完 scrollTop 时 DOM 里还是旧窗口的行
    await expect
      .poll(async () => Math.max(-1, ...(await readRows(page)).rows.map((r) => r.idx)))
      .toBeGreaterThan(140);

    const { rows } = await readRows(page);
    expect(rows.length).toBeLessThan(40);
    for (const r of rows) expect(r.misaligned, `第 ${r.idx} 行错位`).toBe(false);

    // 底部只差一条 p-1.5 的下 padding
    const gap = await page.evaluate(() => {
      const l = document.querySelector("[cmdk-list]")!;
      const bottom = Math.max(
        ...[...document.querySelectorAll("[cmdk-item]")].map((e) => e.getBoundingClientRect().bottom),
      );
      return l.getBoundingClientRect().bottom - bottom;
    });
    expect(gap).toBeCloseTo(PAD_Y, 0);
  });

  test("滚动中段没有空洞也没有重复行", async ({ page }) => {
    await setScroll(page, 3000);
    // 旧窗口的行此时全部不可见，可视行数回到 0；等新窗口渲染出来
    await expect
      .poll(async () => (await readRows(page)).rows.filter((r) => !r.offscreen).length)
      .toBeGreaterThanOrEqual(6);

    const { rows } = await readRows(page);
    expect(new Set(rows.map((r) => r.idx)).size).toBe(rows.length);
    for (const r of rows) expect(r.misaligned, `第 ${r.idx} 行错位`).toBe(false);
  });
});

test.describe("键盘导航", () => {
  test("↓ 下移一项，↑ 移回来", async ({ page }) => {
    const before = await readRows(page);
    expect(before.selected).toHaveLength(1);

    await page.locator("[cmdk-input]").press("ArrowDown");
    const down = await readRows(page);
    expect(down.selected[0]).not.toBe(before.selected[0]);
    expect(down.rows.find((r) => r.value === down.selected[0])?.idx).toBe(1);

    await page.locator("[cmdk-input]").press("ArrowUp");
    expect((await readRows(page)).selected[0]).toBe(before.selected[0]);
  });

  test("从第一项往上回到最后一项", async ({ page }) => {
    await page.locator("[cmdk-input]").press("ArrowUp");
    const s = await readRows(page);
    expect(s.rows.find((r) => r.value === s.selected[0])?.idx).toBeGreaterThan(100);
  });

  test("End 跳到最后一项且该项在 DOM 里", async ({ page }) => {
    await page.locator("[cmdk-input]").press("End");
    const s = await readRows(page);
    const idx = s.rows.find((r) => r.value === s.selected[0])?.idx;
    expect(idx).toBeGreaterThan(100);
  });
});

test.describe("选中项必须始终留在 DOM 里", () => {
  // 这是虚拟化最容易踩的坑：cmdk 靠 DOM 里的 aria-selected 找当前项，
  // 选中项一旦被滚出窗口且没有渲染，Enter 会静默失效。
  // VirtualList 用 pin 区间把它留在 DOM 里，这条用例守着它。
  test("滚远之后 Enter 仍作用在看不见的选中项上", async ({ page }) => {
    await page.locator("[cmdk-input]").press("Home");
    const top = await readRows(page);
    const pinned = top.rows.find((r) => r.idx === 0);
    expect(pinned, "首行应当已渲染").toBeDefined();
    const before = pinned!.copyCount;

    await setScroll(page, 6000);
    await expect.poll(async () => (await readRows(page)).selected.length).toBe(1);

    const far = await readRows(page);
    const stillThere = far.rows.find((r) => r.idx === 0);
    expect(stillThere, "选中项应被 pin 住").toBeDefined();
    expect(stillThere!.offscreen, "此时它应在可视区外").toBe(true);

    await page.locator("[cmdk-input]").press("Enter");
    await expect(statusBar(page)).toHaveText(/已粘贴/);

    const after = await readRows(page);
    expect(after.rows.find((r) => r.idx === 0)!.copyCount).toBeGreaterThan(before);
  });
});

test.describe("右键菜单", () => {
  test("打开后有四项", async ({ page }) => {
    await page.locator("[cmdk-item]").first().click({ button: "right" });
    await expect(page.locator('[role="menu"]')).toBeVisible();
    await expect(page.locator('[role="menu"] button')).toHaveText([
      /粘贴/,
      /复制/,
      /收藏/,
      /删除/,
    ]);
  });

  test("点某项会执行动作并关闭菜单", async ({ page }) => {
    await page.locator("[cmdk-item]").first().click({ button: "right" });
    await page.locator('[role="menu"] button', { hasText: "收藏" }).click();
    await expect(page.locator('[role="menu"]')).toHaveCount(0);
    await expect(statusBar(page)).toHaveText(/收藏/);
  });

  test("Esc 与点击空白都能关掉", async ({ page }) => {
    await page.locator("[cmdk-item]").first().click({ button: "right" });
    await expect(page.locator('[role="menu"]')).toBeVisible();
    await page.locator("[cmdk-input]").press("Escape");
    await expect(page.locator('[role="menu"]')).toHaveCount(0);

    await page.locator("[cmdk-item]").first().click({ button: "right" });
    await expect(page.locator('[role="menu"]')).toBeVisible();
    await page.mouse.click(20, 20);
    await expect(page.locator('[role="menu"]')).toHaveCount(0);
  });
});

test.describe("搜索", () => {
  test("输入即时回显，不被查询阻塞", async ({ page }) => {
    // 输入框受控于 store.query，不等防抖，所以应当逐字回显。
    // 这条守的是「别为了防抖把输入也一起 debounce 掉」。
    await page.locator("[cmdk-input]").pressSequentially("postgres", { delay: 0 });
    await expect(page.locator("[cmdk-input]")).toHaveValue("postgres");
  });

  test("输入后按内容过滤", async ({ page }) => {
    await page.locator("[cmdk-input]").fill("postgres");
    await expect(page.locator("[cmdk-item]")).toHaveCount(1);
    await expect(page.locator("[cmdk-item]")).toContainText("postgres");
  });

  test("无结果时显示空状态", async ({ page }) => {
    await page.locator("[cmdk-input]").fill("zzzz-不存在-zzzz");
    await expect(page.locator("[cmdk-item]")).toHaveCount(0);
    await expect(page.getByText("没有匹配")).toBeVisible();
  });

  // 刻意没测「防抖 80ms」本身：查询次数在 UI 上观测不到。
  // store 的请求序号会把过期响应丢掉，所以就算把
  // SEARCH_DEBOUNCE 改成 0，8 个击键也只会重绘一次 ——
  // 防抖省的是后端查询数，不是重绘数。要测它得往 api 层
  // 加计数器，那属于改动被测代码换取可观测性，暂不做。
});

test.describe("类型筛选与图片", () => {
  test("按类型筛选后可清除", async ({ page }) => {
    await page.getByRole("button", { name: "json", exact: true }).click();
    await expect(page.locator("[cmdk-item]").first()).toBeVisible();
    expect(await page.locator("[cmdk-item]").count()).toBeGreaterThan(0);

    await page.getByRole("button", { name: "清除筛选" }).click();
    await expect(page.getByRole("button", { name: "清除筛选" })).toHaveCount(0);
    expect(await page.locator("[cmdk-item]").count()).toBeGreaterThan(0);
  });

  test("图片行渲染出缩略图且解码成功", async ({ page }) => {
    await setScroll(page, 1400);
    const img = page.locator("[cmdk-item] img").first();
    await expect(img).toBeVisible();
    const size = await img.evaluate((el: HTMLImageElement) => ({
      w: el.naturalWidth,
      h: el.naturalHeight,
      ok: el.complete && el.naturalWidth > 0,
    }));
    expect(size.ok, "内联 SVG data URL 应当能解码").toBe(true);
  });
});

test("选中态色条与行左边框完全重合", async ({ page }) => {
  // absolute 的包含块是 padding box：left-0 会缩进 border-l-2 内侧 2px，
  // 不写 top 还会退回静态位置（py-2.5 之下 10px）。
  // 这条盯的就是 ItemRow 里那个 left-[-2px] top-0。
  const geo = await page.evaluate(() => {
    const row = document.querySelector('[cmdk-item][aria-selected="true"]')!;
    const r = row.getBoundingClientRect();
    const bar = [...row.children].find((c) => getComputedStyle(c).position === "absolute")!;
    const b = bar.getBoundingClientRect();
    return {
      dx: b.left - r.left,
      dy: b.top - r.top,
      dh: b.height - r.height,
      rowH: r.height,
    };
  });
  expect(geo.dx).toBeCloseTo(0, 1);
  expect(geo.dy).toBeCloseTo(0, 1);
  expect(geo.dh).toBeCloseTo(0, 1);
  expect(geo.rowH).toBe(ROW_H);
});

test("跑一轮主要交互后控制台干净", async ({ page }) => {
  const errors: string[] = [];
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));

  await page.locator("[cmdk-input]").press("ArrowDown");
  await page.locator("[cmdk-item]").first().click({ button: "right" });
  await page.keyboard.press("Escape");
  await page.locator("[cmdk-input]").fill("docker");
  await expect(page.locator("[cmdk-item]").first()).toBeVisible();
  await setScroll(page, 2000);

  expect(errors).toEqual([]);
});
