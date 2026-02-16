# OpenClaw Overlay 🦞

Always-on-top HUD overlay for [OpenClaw](https://github.com/openclaw/openclaw) — shows context windows, rate limits, and session status at a glance.

![Windows](https://img.shields.io/badge/platform-Windows-blue)
![Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange)
![License](https://img.shields.io/badge/license-MIT-green)

## What it does

A small frosted-glass widget that sits on top of your other windows and shows:

- **Context window usage** for all active sessions (main, sub-agents, cron jobs)
- **Rate limits** (hourly/weekly) with visual progress bars
- **Provider usage** aggregated across sessions
- **Session metadata** (model, tokens used, last activity, compaction count)

Data comes from your OpenClaw dev-tools API (port 8005 by default).

## Install

Download the latest `.msi` or `.exe` installer from [Releases](https://github.com/solomonneas/openclaw-overlay/releases).

## Prerequisites

This overlay reads data from an API endpoint. You need one of:

1. **OpenClaw dev-tools-api** running on `localhost:8005` with a `GET /api/overlay/status` endpoint
2. Or any HTTP server returning the expected JSON shape (see below)

## API Contract

The overlay polls `GET http://localhost:8005/api/overlay/status` every 15 seconds.

Expected response shape:

```json
{
  "sessions": [
    {
      "label": "main",
      "model": "anthropic/claude-opus-4-6",
      "contextPercent": 45,
      "tokensUsed": 92000,
      "tokensMax": 200000,
      "lastActivity": "2026-02-16T20:00:00Z",
      "compactionCount": 2,
      "isMain": true,
      "isCron": false
    }
  ],
  "rateLimits": {
    "hourly": { "percentLeft": 82, "resetIn": "2h 33m" },
    "weekly": { "percentLeft": 80, "resetIn": "4d 18h" }
  },
  "codex24h": {
    "totalTokens": 0,
    "inputTokens": 0,
    "outputTokens": 0,
    "sessions": 0
  }
}
```

## Build from source

Requirements: [Rust](https://rustup.rs/), [Node.js](https://nodejs.org/) 18+

```bash
git clone https://github.com/solomonneas/openclaw-overlay.git
cd openclaw-overlay
npm install
npx tauri build
```

The installer will be in `src-tauri/target/release/bundle/`.

For development:

```bash
npx tauri dev
```

## Features

- 🪟 **Always on top** — stays visible while you work
- 🎨 **Frosted glass UI** — dark theme, translucent background
- 🔄 **Auto-refresh** every 15 seconds
- 🦞 **System tray** — minimize to tray, click to restore
- 📊 **Color-coded bars** — green/yellow/orange/red based on usage

## Tech Stack

- [Tauri 2](https://tauri.app/) (Rust + WebView2)
- Vanilla HTML/CSS/JS (no framework, ~600 lines)
- [reqwest](https://docs.rs/reqwest) for API calls from Rust

## License

MIT
