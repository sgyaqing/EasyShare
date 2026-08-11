# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

EasyShare is a **single-executable file server** for sharing text, images, and files within a household or small group — a lightweight, login-free alternative to WeChat file transfer. Users access it via a browser-based chat interface (similar to WeChat Web). The authoritative spec is `DESIGN.md` (in Chinese); this file summarizes its key requirements.

## Tech Stack & Commands

- **Language/Framework**: Rust + Axum web framework
- **Database**: SQLite (`easyshare-files/db/easyshare.db`) for message history
- **Frontend**: framework TBD — must suit a chat UI and support as many browsers as possible
- Standard Cargo workflow applies once the crate exists: `cargo build`, `cargo run`, `cargo test`, `cargo test <name>` for a single test.

## Key Requirements & Conventions

- **Cross-platform**: must run on Linux, macOS, Windows (developed on macOS). Produce a single executable.
- **Internationalization**: logs in **plain English only**. UI language auto-detected from OS/browser language — Chinese if the environment is Chinese, otherwise English (only these two for now).
- **Server startup**: print the bound host IP and port. Default port **8972**, overridable via CLI argument. The advertised IP (title/banner/local identity) can be overridden with the `EASY_SHARE_HOST_IP` env var — mainly for containers, where auto-detection only sees the container's internal address.
- **Page title**: `EasyShare - <server IP>` (literal IP, no angle brackets).
- **No registration**: identity = client hostname + IP, displayed as `hostname (IP)`.
- **Avatars**: generated on first visit — circular icon with the hostname's first letter, background color derived from a hash of `hostname+IP` (must keep the letter legible; colors should differ across users). Stored in `easyshare-files/icons/`.
- **Runtime data**: everything lives under `easyshare-files/` next to the executable (created on first run) — uploads, `icons/` avatars, `db/` database, `logs/` log files. Migration = copy the executable + this directory.
- **Chat behavior**:
  - Anyone visiting the server sees all messages (single shared room).
  - Images render as thumbnails, clickable for full view.
  - Right-click text → "Copy" menu (copy to clipboard); right-click image/file → "Download" menu.
  - Right-click own message → "Recall" （撤回） menu to retract it.
  - Upload via drag-and-drop onto the input box or a file-picker icon.
  - Initially show only the latest 10 messages; scrolling up loads 10 more at a time (pagination).
- **Tests**: generate test code alongside the implementation.
- **.gitignore**: must ignore `DESIGN.md` itself (plus usual Rust artifacts and `easyshare-files/`).
