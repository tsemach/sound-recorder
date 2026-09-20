---
name: release
description: Prompts the developer for a version number and automates the version bump, committing, pushing the release branch, and creating a Pull Request to master.
---

# Release Automation Skill

This skill guides you in executing the release process for `coral-studio`.

## Workflow

1. **Ask for Version:**
   If the user did not specify a version number, ask them to provide one (e.g., `0.2.0`). Check the current version first with `node -p "require('./package.json').version"`.

2. **Create the release branch:**
   From an up-to-date `master`, cut a branch named `release/v<version>`:
   ```bash
   git checkout master && git pull
   git checkout -b release/v<version>
   ```

3. **Bump the version:**
   Update `version` in `package.json` (no `tauri.conf.json` or other version-bearing file exists in this repo — `package.json` is the single source of truth). `pnpm version <version> --no-git-tag-version` does this without creating a commit/tag itself, so you can commit deliberately:
   ```bash
   pnpm version <version> --no-git-tag-version
   git add package.json
   git commit -m "chore: release v<version>"
   ```

4. **Verify the build before pushing:**
   ```bash
   pnpm lint
   pnpm build
   ```
   If either fails, stop and report — do not push a broken release branch.

5. **Push Release Branch:**
   ```bash
   git push -u origin release/v<version>
   ```

6. **Create Pull Request:**
   Open a Pull Request to `master` using the GitHub CLI:
   ```bash
   gh pr create --title "Release v<version>" --body "Automated release version bump to v<version>"
   ```

7. **Completion Summary:**
   Confirm the Pull Request has been created and its URL, and tell the user to merge it to `master`. Merging to `master` is what triggers Vercel's production deployment for this project (no separate CI/CD release build step exists here).
