# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Sound Recorder — a pair of local-first apps that capture digital audio currently playing on a device and save it as a file. See `PRD.md` for the full product spec (goals, non-goals, functional requirements, privacy/legal constraints). Key constraints from the PRD to keep in mind when implementing features:

- Never start recording silently/automatically; recording must always be an explicit user action and be clearly indicated while active.
- Must not bypass DRM, secure audio paths, or platform restrictions.
- Audio stays local by default — no upload, no cloud sync in this phase.
- Write audio incrementally (no full-recording-in-memory) and use temp files + atomic rename on finalize.

Two target apps described in the PRD:
- **sound-app** (`apps/sound-app`) — desktop app. Vite + React 19 frontend (migrated off Next.js — see `docs/superpowers/specs/2026-09-20-sound-app-tauri-migration-design.md`); the PRD calls for an eventual Tauri + Rust backend for capture/file writing/device access — not yet present in this repo.
- **mobile-app** (`apps/mobile-app`) — React Native mobile app. Directory exists but is not yet scaffolded (empty).

## Repo structure

This is a pnpm + Turborepo monorepo (`pnpm-workspace.yaml` includes `apps/*` and `packages/*`).

- `apps/sound-app` — Vite + React 19 app (Tailwind v4, shadcn/ui via `components.json`, Vitest for tests). Tauri backend under `src-tauri/` lands in a follow-up PR.
- `apps/mobile-app` — reserved for the React Native app; not yet implemented.
- `packages/ui` (`@workspace/ui`) — shared shadcn/ui component library consumed by apps via `@workspace/ui/components/*`, `@workspace/ui/hooks/*`, `@workspace/ui/lib/*`, and `@workspace/ui/globals.css`.
- `packages/eslint-config` (`@workspace/eslint-config`) — shared ESLint flat configs (`base.js`, `next.js`, `react-internal.js`) consumed by each app/package's own `eslint.config.js`.
- `packages/typescript-config` (`@workspace/typescript-config`) — shared `tsconfig` bases (`base.json`, `nextjs.json`, `react-library.json`).

Turbo pipeline tasks (`turbo.json`) are `build`, `lint`, `format`, `typecheck`, `test`, `dev` — each workspace package defines its own script for these, and `turbo` fans them out respecting `dependsOn: ["^task"]` ordering.

## Commands

Run from the repo root unless noted. All commands fan out across the workspace via Turbo; scope to one package with `--filter`.

```bash
pnpm install            # install all workspace deps

pnpm dev                # turbo dev (all apps, persistent/uncached)
pnpm build              # turbo build
pnpm lint               # turbo lint
pnpm format             # turbo format
pnpm typecheck          # turbo typecheck
pnpm test               # turbo test

# scope to a single package, e.g. the sound-app:
pnpm --filter sound-app dev
pnpm --filter sound-app build
pnpm --filter sound-app lint
pnpm --filter sound-app typecheck
pnpm --filter sound-app test
```

Test runner: `sound-app` has Vitest configured; run `pnpm test` to execute tests for packages that define a `test` script.

### Adding shadcn/ui components

Add new UI components from the `sound-app` workspace so they land in the shared `packages/ui` package:

```bash
pnpm dlx shadcn@latest add button -c apps/sound-app
```

Components are placed in `packages/ui/src/components` and consumed from apps via `import { Button } from "@workspace/ui/components/button"`.

## Formatting

Prettier is configured at the root (`.prettierrc`): no semicolons, double quotes, 2-space indent, `es5` trailing commas, 80-char print width, with `prettier-plugin-tailwindcss` sorting Tailwind classes against `packages/ui/src/styles/globals.css` and recognizing `cn`/`cva` calls.
