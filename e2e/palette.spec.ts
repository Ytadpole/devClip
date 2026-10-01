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
 *
 * 靠 data-status 而不是配色类定位：状态色要随主题换（浅色下
 * emerald-400 白底读不出来），类名一变测试就跟着碎。data-status
 * 顺带给出了 ok / warn / err，比「这个 span 恰好是绿的」语义准
 */
const statusBar = (page: Page) => page.locator("[data-status]").first();

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
    // 别用 postgres 当样本：含它的那行是敏感项，默认分组下搜不到
    await page.locator("[cmdk-input]").fill("select * from users");
    await expect(page.locator("[cmdk-item]")).toHaveCount(1);
    await expect(page.locator("[cmdk-item]")).toContainText("select * from users");
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

test.describe("工具箱动作条", () => {
  /** 工具条本体。用 data-toolbox 而不是靠按钮文字找 ——
      改一次文案不该连带改一次测试 */
  const bar = (page: Page) => page.locator("[data-toolbox]");
  const buttons = (page: Page) => page.locator("[data-toolbox] button");

  /**
   * 只看某个类型，让选中项的类型可确定。
   *
   * 必须先「清除筛选」：类型 chip 是**切换**而不是单选，
   * 直接点下一个会把上一个也留着，于是第一项仍是上次的类型
   */
  const onlyType = async (page: Page, t: string) => {
    // 没筛选时这个按钮不存在（`filtered &&`），所以不能直接点
    const clear = page.locator("button", { hasText: "清除筛选" });
    if (await clear.count()) await clear.click();
    await page.locator("button", { hasText: new RegExp(`^${t}$`) }).first().click();
    await expect(page.locator("[cmdk-item]").first()).toBeVisible();
  };

  test("动作列表由后端决定，不按类型硬编码", async ({ page }) => {
    // mock 与 Rust 的 ENTRIES 必须一致（见 mock.ts 里的说明）。
    // 以前两边各写各的：e2e 跑的是一套假的列表，
    // 而真应用里点「Sort Keys」会得到「没有这个动作」
    await onlyType(page, "json");
    await expect(buttons(page)).toHaveText([/美化/, /美化 \(4 空格\)/, /压缩/]);

    await onlyType(page, "sql");
    await expect(buttons(page)).toHaveText([/格式化/, /关键字大写/, /关键字小写/, /提取表名/]);

    // 用 url 而不是 uuid：筛选栏只有 QUICK_TYPES 那几个，
    // uuid 的 chip 不在（它的动作由 Rust 侧单元测试守着）
    await onlyType(page, "url");
    await expect(buttons(page)).toHaveText([/去掉 query/, /提取域名/]);
  });

  test("JWT 动作带着「base64 不是加密」的提示", async ({ page }) => {
    // docs/04 要求 UI 上明确标注。这句话只能从后端来 ——
    // 写在文档里没人看得到，而用户点之前就该知道
    await onlyType(page, "jwt");
    await expect(buttons(page)).toHaveText([/解 Header/, /解 Payload/, /检查过期/]);
    await expect(page.locator('[data-toolbox] button[title="base64 不是加密"]')).toHaveCount(2);
  });

  test("点动作会在状态栏给出摘要", async ({ page }) => {
    await onlyType(page, "json");
    await buttons(page).first().click();
    // 返回的是摘要而不是结果本身 —— 结果进了剪贴板，
    // 而状态栏放不下格式化后的 JSON（见 api.ts 的注释）
    await expect(statusBar(page)).toHaveText(/mock.*已复制到剪贴板/);
  });

  test("没有动作的类型不显示工具条", async ({ page }) => {
    // markdown 在阶段 6 没有动作。工具条不该留一条空壳 ——
    // 之前 mock 侧给它挂了个 Outline，两边本来就对不上
    await page.locator("[cmdk-input]").fill("# DevClip");
    await expect(page.locator("[cmdk-item]").first()).toBeVisible();
    await expect(bar(page)).toHaveCount(0);
  });
});

test.describe("敏感信息分组", () => {
  test("默认搜不到敏感项，展开分组后可见且带锁", async ({ page }) => {
    // mock 第 13 条是 sensitive 的 DATABASE_URL。默认分组下它不存在，
    // 「默认搜不到」是后端过滤的职责 —— mock 与 Rust 的 repo::list 同规则
    await page.locator("[cmdk-input]").fill("DATABASE_URL");
    await expect(page.locator("[cmdk-item]")).toHaveCount(0);
    await expect(page.getByText("没有匹配")).toBeVisible();

    await page.getByRole("button", { name: "含敏感" }).click();
    await expect(page.locator("[cmdk-item]")).toHaveCount(1);
    await expect(page.locator("[cmdk-item]").first()).toContainText("DATABASE_URL");
    // 行内要有锁标记，用户得能看出这条为什么藏着
    await expect(page.locator("[cmdk-item]").first()).toContainText("敏感");

    // 收起分组继续隐藏
    await page.getByRole("button", { name: "含敏感" }).click();
    await expect(page.locator("[cmdk-item]")).toHaveCount(0);
  });

  test("展开分组不影响普通内容的排序与可见性", async ({ page }) => {
    await page.getByRole("button", { name: "含敏感" }).click();
    await expect(page.locator("[cmdk-item]").first()).toBeVisible();
    // 148 条都在（敏感项只是多出来，不是替换别人）
    await page.locator("[cmdk-input]").press("End");
    await expect
      .poll(async () =>
        page.evaluate(() => {
          const list = document.querySelector("[cmdk-list]")!;
          const sel = document.querySelector('[cmdk-item][aria-selected="true"]')!;
          return Math.round(
            (sel.getBoundingClientRect().top - list.getBoundingClientRect().top + list.scrollTop) / 62,
          );
        }),
      )
      .toBeGreaterThan(140);
  });
});

test.describe("设置页", () => {
  test("打开、保存、Esc 退回", async ({ page }) => {
    await page.getByRole("button", { name: "设置" }).click();
    await expect(page.locator("[data-settings]")).toBeVisible();

    // 改保留天数并保存，状态栏要有回音
    const days = page.locator("[data-settings] input[type=number]").first();
    await days.fill("7");
    await page.getByRole("button", { name: "保存设置" }).click();
    await expect(statusBar(page)).toHaveText(/已保存/);

    await page.locator("[cmdk-input], [data-settings] input").first().press("Escape");
    await expect(page.locator("[data-settings]")).toHaveCount(0);
    await expect(page.locator("[cmdk-item]").first()).toBeVisible();
  });
});

test.describe("主题", () => {
  const html = (page: Page) => page.locator("html");

  /** 设置页里的三选一。data-theme-picker 是稳定钩子，别改成靠文字找 */
  const pick = async (page: Page, label: string) => {
    await page.getByRole("button", { name: "设置" }).click();
    await expect(page.locator("[data-settings]")).toBeVisible();
    await page.locator("[data-theme-picker] button", { hasText: label }).click();
    // setTheme 是 await api.setSettings 之后才 resolve，但 apply() 在
    // await 之前就跑完了，所以这里不用等
    await expect(page.locator(`[data-theme-picker] button:text-is("${label}")`)).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  };

  /**
   * 面板根元素的实际底色。
   *
   * 断言属性名（data-theme）只能证明 JS 跑到了，证明不了 CSS 真的
   * 换过来了 —— 少写一条 `html[data-theme="light"]` 规则的话，
   * 属性照样变，而界面还是黑的。
   *
   * 早期读的是 body：窗口透明化之后 body 不再承载底色（它是
   * transparent 的，方形底色会把圆角盖成方块），底色语义搬到了
   * data-panel-root 上
   */
  /**
   * 面板根元素的明度（OKLab L，0~1）。
   *
   * 断言属性名（data-theme）只能证明 JS 跑到了，证明不了 CSS 真的
   * 换过来了 —— 少写一条 `html[data-theme="light"]` 规则的话，
   * 属性照样变，而界面还是黑的。
   *
   * 早期读的是 body 的 rgb 通道：窗口透明化后底色语义搬到
   * data-panel-root，而 bg-panel/80 的计算值是 color-mix 的 oklab
   * 记法（canvas 也会原样吐回 oklab，不归一化），于是直接取 L
   * 分量 —— 「底色真的变白」本来就是明度断言
   */
  const panelLightness = (page: Page) =>
    page.evaluate(() => {
      const el = document.querySelector("[data-panel-root]")!;
      const bg = getComputedStyle(el).backgroundColor;
      const ok = bg.match(/oklab\(\s*([\d.]+)(%?)/);
      if (ok) return Number(ok[1]) * (ok[2] === "%" ? 0.01 : 1);
      // 兜底：哪天序列化变回 rgb，取绿色通道当明度代理
      const rgb = bg.match(/\d+(\.\d+)?/g) ?? ["0"];
      return Number(rgb[1]) / 255;
    });

  test("默认深色，切到亮色后 html 带 data-theme=light 且底色真的变白", async ({ page }) => {
    // mock 的默认设置是 dark。这条也守着「没设 data-theme 时按深色算」：
    // 属性在设置读回来之前是缺的，那几毫秒里不能闪一下亮色
    await expect(html(page)).toHaveAttribute("data-theme", "dark");
    expect(await panelLightness(page)).toBeLessThan(0.4);

    await pick(page, "亮色");
    await expect(html(page)).toHaveAttribute("data-theme", "light");
    expect(await panelLightness(page)).toBeGreaterThan(0.8);
  });

  test("切回深色能还原，不会卡在亮色", async ({ page }) => {
    await pick(page, "亮色");
    await pick(page, "深色");
    await expect(html(page)).toHaveAttribute("data-theme", "dark");
    expect(await panelLightness(page)).toBeLessThan(0.4);
  });

  test("设置页的开关标出当前主题", async ({ page }) => {
    await page.getByRole("button", { name: "设置" }).click();
    const pressed = page.locator("[data-theme-picker] button[aria-pressed=true]");
    await expect(pressed).toHaveText("深色");
    await pick(page, "亮色");
    await expect(pressed).toHaveText("亮色");
  });
});

test.describe("主题 · 跟随系统", () => {
  // 默认的 colorScheme 是 light。选「跟随系统」后应当变亮 ——
  // 这条守的是 system 不是恒等于 dark
  test.use({ colorScheme: "light" });

  test("系统是亮色时选跟随系统就变亮", async ({ page }) => {
    await page.getByRole("button", { name: "设置" }).click();
    await page.locator("[data-theme-picker] button", { hasText: "跟随系统" }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  });

  test("显式选深色不受系统影响", async ({ page }) => {
    await page.getByRole("button", { name: "设置" }).click();
    await page.locator("[data-theme-picker] button", { hasText: "深色" }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
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
