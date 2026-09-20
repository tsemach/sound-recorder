# sound-app: Next.js → Vite Migration (PR 1 of 8) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert `apps/sound-app` from a Next.js 16 App Router scaffold to a Vite + React 19 + TypeScript SPA, with zero rendered-output change, as step 1 of the 8-PR sound-app Tauri migration.

**Architecture:** Match the pattern `packages/ui` already uses in this monorepo (a React package with no Next.js dependency): `tsconfig.json` extends `@workspace/typescript-config/react-library.json`, `eslint.config.js` uses `@workspace/eslint-config/react-internal`. Introduce Vitest + Testing Library as the repo's first test runner, wired through a new Turbo `test` task.

**Tech Stack:** Vite, @vitejs/plugin-react, React 19, TypeScript, Vitest, @testing-library/react, Tailwind v4 (via existing `@workspace/ui` PostCSS config, unchanged).

**Spec:** `docs/superpowers/specs/2026-09-20-sound-app-tauri-migration-design.md`

## Global Constraints

- No Tauri code in this PR — `src-tauri/` doesn't exist until PR 2.
- No behavior/UI change beyond what the tooling swap forces — the rendered page must look and behave identically to the pre-migration Next.js version.
- `@workspace/ui`, `packages/eslint-config`, `packages/typescript-config` are consumed as-is; no changes to those packages.
- Version pinning: install devDependencies via `pnpm add -D <pkg>` without hardcoded version numbers — let pnpm resolve current-latest-compatible.
- Every `pnpm` verification command must exit 0 before a task is considered done.

---

## Task 1: Vite skeleton, Next.js removed, trivial placeholder page

**Files:**
- Delete: `apps/sound-app/app/layout.tsx`, `apps/sound-app/app/page.tsx`, `apps/sound-app/next.config.ts`, `apps/sound-app/next-env.d.ts`
- Create: `apps/sound-app/index.html`, `apps/sound-app/vite.config.ts`, `apps/sound-app/src/main.tsx`, `apps/sound-app/src/vite-env.d.ts`, `apps/sound-app/public/favicon.ico` (moved from `apps/sound-app/app/favicon.ico`)
- Modify: `apps/sound-app/package.json`, `apps/sound-app/tsconfig.json`, `apps/sound-app/eslint.config.js`, `apps/sound-app/components.json`

**Interfaces:**
- Produces: `src/main.tsx` mounts into `<div id="root">` (from `index.html`) — Task 3 replaces its placeholder render call with `<App />`.

- [ ] **Step 1: Remove Next.js dependency and add Vite**

```bash
cd apps/sound-app
pnpm remove next
pnpm add -D vite @vitejs/plugin-react
```

- [ ] **Step 2: Delete Next.js files, move favicon**

```bash
cd apps/sound-app
git rm app/layout.tsx app/page.tsx next.config.ts next-env.d.ts
mkdir -p public
git mv app/favicon.ico public/favicon.ico
rmdir app 2>/dev/null || true
```

- [ ] **Step 3: Create `apps/sound-app/index.html`**

```html
<!doctype html>
<html lang="en" class="antialiased">
  <head>
    <meta charset="UTF-8" />
    <link rel="icon" href="/favicon.ico" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Sound Recorder</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

- [ ] **Step 4: Create `apps/sound-app/vite.config.ts`**

```ts
import path from "node:path"

import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  server: {
    port: 1420,
    strictPort: true,
  },
})
```

- [ ] **Step 5: Create `apps/sound-app/src/vite-env.d.ts`**

```ts
/// <reference types="vite/client" />
```

- [ ] **Step 6: Create `apps/sound-app/src/main.tsx` (trivial placeholder)**

```tsx
import { StrictMode } from "react"
import { createRoot } from "react-dom/client"

import "@workspace/ui/globals.css"

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <div>Sound Recorder</div>
  </StrictMode>
)
```

- [ ] **Step 7: Update `apps/sound-app/package.json`**

Replace `scripts` with:
```json
"scripts": {
  "dev": "vite",
  "build": "vite build",
  "lint": "eslint",
  "format": "prettier --write \"**/*.{ts,tsx}\"",
  "typecheck": "tsc --noEmit"
}
```
Remove `next` from `dependencies` (already done by Step 1's `pnpm remove`, verify it's gone). Keep `@workspace/ui`, `lucide-react`, `next-themes`, `react`, `react-dom` as-is.

- [ ] **Step 8: Update `apps/sound-app/tsconfig.json`**

```json
{
  "extends": "@workspace/typescript-config/react-library.json",
  "compilerOptions": {
    "paths": {
      "@/*": ["./src/*"],
      "@workspace/ui/*": ["../../packages/ui/src/*"]
    }
  },
  "include": ["vite.config.ts", "src/**/*.ts", "src/**/*.tsx"],
  "exclude": ["node_modules"]
}
```

- [ ] **Step 9: Update `apps/sound-app/eslint.config.js`**

```js
import { config } from "@workspace/eslint-config/react-internal"

/** @type {import("eslint").Linter.Config} */
export default config
```

- [ ] **Step 10: Update `apps/sound-app/components.json`**

Change `"rsc": true` to `"rsc": false`. Leave everything else unchanged.

- [ ] **Step 11: Verify**

```bash
cd apps/sound-app
pnpm dev &
sleep 2 && curl -sf http://localhost:1420 >/dev/null && echo "dev server OK"
kill %1
pnpm build      # expect dist/ produced, no errors
pnpm lint       # expect no errors
pnpm typecheck  # expect no errors
```

- [ ] **Step 12: Commit**

```bash
git add -A apps/sound-app
git commit -m "build(sound-app): migrate from Next.js to Vite scaffold"
```

---

## Task 2: Vitest tooling, proven with a trivial test

**Files:**
- Create: `apps/sound-app/src/test/setup.ts`
- Modify: `apps/sound-app/vite.config.ts`, `apps/sound-app/package.json`

**Interfaces:**
- Consumes: nothing from Task 1 beyond the existing `vite.config.ts` shape.
- Produces: `pnpm test` runner available for Task 3 to use.

- [ ] **Step 1: Install test dependencies**

```bash
cd apps/sound-app
pnpm add -D vitest jsdom @testing-library/react @testing-library/jest-dom
```

- [ ] **Step 2: Create `apps/sound-app/src/test/setup.ts`**

```ts
import "@testing-library/jest-dom/vitest"
```

- [ ] **Step 3: Add Vitest config to `apps/sound-app/vite.config.ts`**

Add as the first line of the file:
```ts
/// <reference types="vitest/config" />
```
Add a `test` field to the `defineConfig` object (alongside `plugins`, `resolve`, `server`):
```ts
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
  },
```

- [ ] **Step 4: Add `test` script to `apps/sound-app/package.json`**

```json
"test": "vitest run"
```

- [ ] **Step 5: Write a throwaway sanity test to prove the runner works**

Create `apps/sound-app/src/sanity.test.ts` (temporary, deleted in Task 3):
```ts
import { describe, expect, it } from "vitest"

describe("vitest sanity check", () => {
  it("runs", () => {
    expect(1 + 1).toBe(2)
  })
})
```

- [ ] **Step 6: Run test to verify it passes**

```bash
cd apps/sound-app && pnpm test
```
Expected: PASS (1 test).

- [ ] **Step 7: Commit**

```bash
git add apps/sound-app
git commit -m "test(sound-app): add Vitest + Testing Library tooling"
```

---

## Task 3: Port the actual page content (test-first)

**Files:**
- Create: `apps/sound-app/src/App.tsx`, `apps/sound-app/src/App.test.tsx`, `apps/sound-app/src/index.css`, `apps/sound-app/src/components/theme-provider.tsx`
- Delete: `apps/sound-app/src/sanity.test.ts` (from Task 2), `apps/sound-app/components/theme-provider.tsx` (old Next location), `apps/sound-app/components/.gitkeep`
- Modify: `apps/sound-app/src/main.tsx`, `apps/sound-app/index.html`

**Interfaces:**
- Consumes: `@workspace/ui/components/button` (`Button` export, unchanged), `next-themes` (`ThemeProvider`/`useTheme`, unchanged — verified framework-agnostic).
- Produces: `App` (named export from `src/App.tsx`) — a zero-prop component — consumed by `src/main.tsx`. `ThemeProvider` (named export from `src/components/theme-provider.tsx`) — consumed by `App.tsx`.

- [ ] **Step 1: Delete the Task 2 sanity test**

```bash
cd apps/sound-app
git rm src/sanity.test.ts
```

- [ ] **Step 2: Write the failing test — `apps/sound-app/src/App.test.tsx`**

```tsx
import { render, screen } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { App } from "./App"

describe("App", () => {
  it("renders the ready message", () => {
    render(<App />)
    expect(screen.getByText("Project ready!")).toBeInTheDocument()
  })
})
```

- [ ] **Step 3: Run test to verify it fails**

```bash
cd apps/sound-app && pnpm test
```
Expected: FAIL (`Cannot find module './App'` or similar — `App.tsx` doesn't exist yet).

- [ ] **Step 4: Create `apps/sound-app/src/components/theme-provider.tsx`**

Move the content of `apps/sound-app/components/theme-provider.tsx` verbatim, **removing only the `"use client"` directive on line 1** (meaningless outside Next's RSC model). Everything else — the `next-themes` `ThemeProvider` wrapper, `isTypingTarget`, the `ThemeHotkey` component handling the `d` key — is unchanged:

```tsx
import * as React from "react"
import { ThemeProvider as NextThemesProvider, useTheme } from "next-themes"

function ThemeProvider({
  children,
  ...props
}: React.ComponentProps<typeof NextThemesProvider>) {
  return (
    <NextThemesProvider
      attribute="class"
      defaultTheme="system"
      enableSystem
      disableTransitionOnChange
      {...props}
    >
      <ThemeHotkey />
      {children}
    </NextThemesProvider>
  )
}

function isTypingTarget(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) {
    return false
  }

  return (
    target.isContentEditable ||
    target.tagName === "INPUT" ||
    target.tagName === "TEXTAREA" ||
    target.tagName === "SELECT"
  )
}

function ThemeHotkey() {
  const { resolvedTheme, setTheme } = useTheme()

  React.useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.defaultPrevented || event.repeat) {
        return
      }

      if (event.metaKey || event.ctrlKey || event.altKey) {
        return
      }

      if (event.key.toLowerCase() !== "d") {
        return
      }

      if (isTypingTarget(event.target)) {
        return
      }

      setTheme(resolvedTheme === "dark" ? "light" : "dark")
    }

    window.addEventListener("keydown", onKeyDown)

    return () => {
      window.removeEventListener("keydown", onKeyDown)
    }
  }, [resolvedTheme, setTheme])

  return null
}

export { ThemeProvider }
```

Then remove the old file:
```bash
git rm apps/sound-app/components/theme-provider.tsx apps/sound-app/components/.gitkeep
rmdir apps/sound-app/components 2>/dev/null || true
```

- [ ] **Step 5: Create `apps/sound-app/src/App.tsx`**

```tsx
import { Button } from "@workspace/ui/components/button"

import { ThemeProvider } from "./components/theme-provider"

export function App() {
  return (
    <ThemeProvider>
      <div className="flex min-h-svh p-6">
        <div className="flex max-w-md min-w-0 flex-col gap-4 text-sm leading-loose">
          <div>
            <h1 className="font-medium">Project ready!</h1>
            <p>You may now add components and start building.</p>
            <p>We&apos;ve already added the button component for you.</p>
            <Button className="mt-2">Button</Button>
          </div>
          <div className="text-muted-foreground font-mono text-xs">
            (Press <kbd>d</kbd> to toggle dark mode)
          </div>
        </div>
      </div>
    </ThemeProvider>
  )
}
```

- [ ] **Step 6: Create `apps/sound-app/src/index.css`**

```css
:root {
  --font-sans: ui-sans-serif, system-ui, sans-serif;
}
```

This fixes a real bug the migration would otherwise introduce: `packages/ui/src/styles/globals.css` sets `--font-sans: var(--font-sans)` (a self-reference with no fallback) inside its `@theme inline` block. It only worked in the Next.js scaffold because `next/font`'s generated `.variable` class set that CSS variable at runtime. Without this fix, Tailwind's `font-sans` utility would resolve to nothing and fall through to the browser's default serif font. `--font-mono` needs no equivalent fix — Tailwind v4's built-in default mono stack applies automatically since `globals.css` never overrides it.

- [ ] **Step 7: Update `apps/sound-app/src/main.tsx`**

```tsx
import { StrictMode } from "react"
import { createRoot } from "react-dom/client"

import "@workspace/ui/globals.css"
import "./index.css"

import { App } from "./App"

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>
)
```

- [ ] **Step 8: Run test to verify it passes**

```bash
cd apps/sound-app && pnpm test
```
Expected: PASS.

- [ ] **Step 9: Manual browser verification (required for this UI change)**

```bash
cd apps/sound-app && pnpm dev
```
Open the printed `http://localhost:1420` URL in a browser. Confirm:
- Text renders in a sans-serif font (not serif) — validates the Step 6 fix.
- The "Button" component renders with correct shadcn styling.
- Pressing `d` (outside any input) toggles between light and dark mode.

Stop the dev server (Ctrl+C) once confirmed.

- [ ] **Step 10: Commit**

```bash
git add -A apps/sound-app
git commit -m "feat(sound-app): port page content to Vite (App.tsx, theme-provider)"
```

---

## Task 4: Repo-level wiring and docs

**Files:**
- Modify: `turbo.json`, `package.json` (root), `.gitignore`, `CLAUDE.md`

- [ ] **Step 1: Update `turbo.json`**

Change the `build` task's `outputs` from `[".next/**", "!.next/cache/**"]` to `["dist/**"]` (Vite's build output directory). Add a `test` task mirroring the existing `lint`/`typecheck` shape:

```json
{
  "$schema": "https://turbo.build/schema.json",
  "ui": "tui",
  "tasks": {
    "build": {
      "dependsOn": ["^build"],
      "inputs": ["$TURBO_DEFAULT$", ".env*"],
      "outputs": ["dist/**"]
    },
    "lint": {
      "dependsOn": ["^lint"]
    },
    "format": {
      "dependsOn": ["^format"]
    },
    "typecheck": {
      "dependsOn": ["^typecheck"]
    },
    "test": {
      "dependsOn": ["^test"]
    },
    "dev": {
      "cache": false,
      "persistent": true
    }
  }
}
```

- [ ] **Step 2: Add `test` script to root `package.json`**

Add `"test": "turbo test"` to the `scripts` object.

- [ ] **Step 3: Update `.gitignore`**

Remove the `next-env.d.ts` line (under the `# typescript` section) — nothing in the repo produces that filename after this PR.

- [ ] **Step 4: Update `CLAUDE.md`**

In the "Project" section, replace the `sound-app` bullet:
```
- **sound-app** (`apps/sound-app`) — desktop app. Currently scaffolded as a Next.js frontend (the PRD calls for an eventual Tauri + Rust backend for capture/file writing/device access — not yet present in this repo).
```
with:
```
- **sound-app** (`apps/sound-app`) — desktop app. Vite + React 19 frontend (migrated off Next.js — see `docs/superpowers/specs/2026-09-20-sound-app-tauri-migration-design.md`); the PRD calls for an eventual Tauri + Rust backend for capture/file writing/device access — not yet present in this repo.
```

In the "Repo structure" section, replace:
```
- `apps/sound-app` — Next.js 16 app (React 19, Tailwind v4, shadcn/ui via `components.json`).
```
with:
```
- `apps/sound-app` — Vite + React 19 app (Tailwind v4, shadcn/ui via `components.json`, Vitest for tests). Tauri backend under `src-tauri/` lands in a follow-up PR.
```

Delete the entire "## Important: Next.js version caveat" section (including its `apps/sound-app/AGENTS.md` reference) — after this PR, no app in the repo uses Next.js, so that guidance no longer applies to anything.

- [ ] **Step 5: Full repo-level verification**

```bash
cd /home/tsemach/projects/sound-recorder
pnpm build
pnpm lint
pnpm typecheck
pnpm test
git status   # confirm no leftover Next.js files (app/, next.config.ts, next-env.d.ts, components/theme-provider.tsx gone)
```
All four `pnpm` commands must exit 0.

- [ ] **Step 6: Commit**

```bash
git add turbo.json package.json .gitignore CLAUDE.md
git commit -m "chore: wire test task into Turbo, update docs for Vite migration"
```

---

## Roadmap: remaining PRs (from the approved spec, to be planned in detail when reached)

2. **Tauri shell** — `src-tauri/` scaffold, restrictive capabilities (no shell/network plugins), `tauri.conf.json` wired to the Vite dev server on `:1420` / `dist/` build output, a trivial `ping` command proving the IPC bridge, Turbo scripts updated.
3. **State machine + mocked capture** — `RecordingState` enum, Tauri managed state, all commands/events wired end-to-end, backed by a fake timer-driven capture source. Full Record/Pause/Resume/Stop UI becomes testable without real audio.
4. **Real Linux capture** — `libpulse-binding` integration, `LinuxPulseCapture`, real `list_sources()`.
5. **WAV writer + atomic finalize** — incremental PCM writes to temp file, header patch + atomic rename on stop, orphaned-temp-file recovery.
6. **Storage checks + error surfacing** — disk-space checks, `Error` state wired to a UI error banner.
7. **Recordings list screen** — scan save dir, list/play/rename/delete/reveal.
8. **Settings screen** — save-location picker, filename template, source/quality selection persisted.
