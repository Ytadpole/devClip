import { defineConfig, devices } from "@playwright/test";

/** 与 vite.config.ts 的 server.port 一致，不能改 */
const PORT = 1420;
const URL = `http://localhost:${PORT}`;

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  reporter: "list",

  use: {
    baseURL: URL,
    // 挂掉时留 trace，便于回看当时的 DOM 与控制台
    trace: "retain-on-failure",
  },

  projects: [
    {
      name: "chromium",
      // 视口要够高，否则 max-h-[min(420px,50vh)] 会取到 50vh，
      // 窗口行数变化会让行数上限的断言跟着漂
      use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 900 } },
    },
  ],

  // 跑测试前得先有 dev server 在听。本地已经开着 pnpm dev 时直接复用 ——
  // 1420 是 strictPort，抢占会直接失败，不会自动退到别的端口。
  webServer: {
    command: "pnpm dev",
    url: URL,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
