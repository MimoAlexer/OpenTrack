import { defineConfig, devices } from '@playwright/test';

const baseURL = process.env.OPENTRACK_BROWSER_URL ?? 'http://localhost:18081';
const origin = new URL(baseURL);
if (origin.protocol !== 'http:' || !['localhost', '127.0.0.1'].includes(origin.hostname)
    || origin.port !== '18081' || origin.username || origin.password || origin.search || origin.hash || origin.pathname !== '/') {
  throw new Error('Browser acceptance requires its dedicated HTTP loopback origin.');
}

export default defineConfig({
  testDir: './e2e',
  testMatch: '**/*.browser.ts',
  timeout: 60_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  workers: 1,
  retries: 0,
  forbidOnly: true,
  reporter: './e2e/receipt-reporter.ts',
  outputDir: '../work/browser-acceptance/output',
  use: {
    baseURL,
    trace: 'off',
    screenshot: 'off',
    video: 'off',
    locale: 'en-US',
    timezoneId: 'UTC',
    actionTimeout: 10_000,
    serviceWorkers: 'block',
  },
  projects: [
    { name: 'chromium', use: { ...devices['Desktop Chrome'] } },
    { name: 'firefox', use: { ...devices['Desktop Firefox'] } },
    { name: 'webkit', use: { ...devices['Desktop Safari'] } },
  ],
});
