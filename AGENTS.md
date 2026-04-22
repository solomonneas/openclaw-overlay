# AGENTS.md

## Project
- **Name:** OpenClaw Overlay
- **Stack:** Tauri 2, Rust, vanilla HTML/CSS/JS
- **Purpose:** always-on-top OpenClaw HUD overlay

## Architecture
- Desktop shell: `src-tauri/`
- Frontend assets/UI: `dist/`
- API dependency: polls `GET /api/overlay/status` from dev-tools-api

## Build & Verify
```bash
npm install
npx tauri dev
npx tauri build
```

After changes, verify the overlay still reads the expected API shape and renders correctly.

## Key Rules
- Keep the app lightweight. Do not add unnecessary frameworks.
- Preserve the overlay's always-on-top HUD purpose.
- Maintain compatibility with the existing `/api/overlay/status` response contract unless coordinated with the API side.
- Keep the dark frosted-glass look unless explicitly redesigning it.

## Style Guide
- No em dashes. Ever.
- Favor clarity and glanceability over dense UI.

## Git Rules
- Use conventional commits.
- Never add `Co-Authored-By` lines.
- Never mention AI tools or vendors in commit messages.
