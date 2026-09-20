---
name: debug
description: >-
  Guides the agent in building, running, and visually inspecting this Vite app using pnpm and Playwright.
---

# Vite Application Debugging

## Overview
This skill provides a standard operating procedure for compiling, launching, and visually debugging this Vite (React) application.

## Dependencies
`@playwright/test` (devDependency) with the Chromium browser installed. Install once with:
```bash
pnpm exec playwright install chromium
```

## Quick Start

### 1. Type-check and lint
Verify the code builds cleanly before running it:
```bash
pnpm lint
```

### 2. Launch the Application (Dev Server)
Start the Vite dev server (fixed at `http://localhost:1420`, `strictPort: true`):
```bash
pnpm dev
```

### 3. Check UI Visually (Headless Playwright)
Once the dev server is up, run a Playwright script to load the page headlessly, capture a screenshot, and dump the DOM:
```bash
node scratch/check_ui.js
```
(Create `scratch/check_ui.js` if it doesn't exist yet — see the template below.)

---

## Workflow

### 1. Verify the build
```bash
pnpm lint
```
If linting/type-checking fails, **stop immediately** and report the errors to the user. Do not silently patch around compiler/lint errors without user guidance.

### 2. Running the Dev Server
```bash
pnpm dev
```
This starts Vite on port `1420` (`strictPort: true`, so it fails fast instead of picking another port if `1420` is already in use). Run it with `run_in_background` (or in a separate terminal) so you can drive it with Playwright while it stays up. For a production-accurate check, use `pnpm build && pnpm --filter sound-app preview` instead.

### 3. Playwright Headless Automation & Visual Inspection
To check how the React UI actually renders:
1. Ensure the dev server is active on `http://localhost:1420`.
2. Write/use a headless script (e.g. `scratch/check_ui.js`) to:
   * Launch Chromium via `@playwright/test`'s `chromium.launch()`.
   * Navigate to `http://localhost:1420` (or any route under test).
   * Wait for the page to be idle / key content to mount.
   * Save a screenshot (e.g. `scratch/screenshot.png`).
   * Optionally print the rendered DOM or run assertions.
3. Run it: `node scratch/check_ui.js`
4. Read the printed output and view the captured screenshot to verify correct UI behavior.

**Template** (`scratch/check_ui.js`):
```js
const { chromium } = require('@playwright/test')

;(async () => {
  const browser = await chromium.launch()
  const page = await browser.newPage()
  await page.goto('http://localhost:1420', { waitUntil: 'networkidle' })
  await page.screenshot({ path: 'scratch/screenshot.png', fullPage: true })
  console.log(await page.content())
  await browser.close()
})()
```

---

## Common Mistakes & Pitfalls

* **Using `npm` instead of `pnpm`:** This project uses `pnpm` (`pnpm-lock.yaml` is the lockfile). Never run `npm run dev` / `npm install`.
* **Forgetting the dev server is a prerequisite:** Playwright will fail to connect if `pnpm dev` isn't already running on port `1420`.
* **Port conflicts:** If port `1420` is already in use, Vite exits with an error instead of picking another port (`strictPort: true` in `vite.config.ts`). Stop the conflicting process, or pass `--port <port>` to `pnpm dev` and update the script's URL accordingly.
* **Auto-installing browser binaries silently:** If `chromium.launch()` fails because the browser isn't installed, run `pnpm exec playwright install chromium` — don't paper over it with unrelated workarounds.
