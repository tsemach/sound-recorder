---
trigger: always_on
---

---
description: Core development workflow and critical thinking rules
alwaysApply: true
---

# STRICT RULES

## CRITICAL PARTNER MINDSET

Do not affirm my statements or assume my conclusions are correct. Question assumptions, offer counterpoints, test reasoning. Prioritize truth over agreement.

## EXECUTION SEQUENCE (always reply with "Applying rules X,Y,Z")

1. SEARCH FIRST - Use codebase_search/grep/web_search/MCP tools until finding similar functionality or confirming none exists. Investigate deeply, be 100% sure before implementing.
2. REUSE FIRST - Check existing functions/patterns/structure. Extend before creating new. Strive to smallest possible code changes
3. NO ASSUMPTIONS - Only use: files read, user messages, tool results. Missing info? Search then ask user.
4. CHALLENGE IDEAS - If you see flaws/risks/better approaches, say so directly
5. BE HONEST - State what's needed/problematic, don't sugarcoat to please
6. PERIODICALLY Self-check rule compliance during long conversations
7. LOG CHECK - Check the 3 service logs for errors (logs/frontend,backend,integration) after implementing changes, or debugging/fixing issues
8. RULE REFRESH - Re-check this rules file every few messages to stay compliant

## CODING STANDARDS

- Follow development-guidelines.mdc for planning, file organization, typing, clean code, and code quality
- Test your tests and run npm run lint:ci for lint check

## Github Rules

- PR content: use gh pr view for basic info
- PR metadata: use gh api repos/OWNER/REPO/pulls/PR_NUMBER for JSON
- PR files: use gh api repos/OWNER/REPO/pulls/PR_NUMBER/files
- PR comments: check 3 types - issues/comments, pulls/comments, pulls/reviews
- NEVER use fetch_pull_request tool - returns wrong data
- Line comments most important: gh api repos/OWNER/REPO/pulls/PR_NUMBER/comments
- PR description: Use .github/pull_request_template.md - extract XXXX from branch name

## PROHIBITED ACTIONS

- DO NOT WRITE DOCS UNLESS EXPLICITLY ASKED TO
- NEVER run npm/yarn start commands - assume dev servers always running
c
