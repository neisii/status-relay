<div align="center">
  <img src="src-tauri/icons/128x128.png" alt="StatusRelay Icon" width="128">
  <h1>StatusRelay</h1>
  <p><strong>A sleek, deeply integrated macOS tray application to monitor Atlassian Statuspage services.</strong></p>
</div>

<br/>

![StatusRelay App Running](assets/app-screenshot.png)

## Overview

StatusRelay is a native macOS menu bar (tray) application that monitors external infrastructure health (e.g., Claude, Cloudflare). It hides the dock icon and quietly polls endpoints in the background, minimizing CPU footprint and allowing you to check real-time service health with zero friction.

Features include:
- **Dynamic Tray Badges:** Overlays a clean native color dot (🟢 Operational, 🟡 Degraded, 🔴 Outage) right over the app logo on your menu bar.
- **Native Notifications:** Triggers macOS notification center alerts instantly when a service degrades or recovers.
- **Ultra-lightweight:** Powered by **Tauri v2** + **Rust** (`tokio` async). Zero heavy UI components.

---

## 🚀 Getting Started

### Prerequisites
Make sure you have [Node.js](https://nodejs.org/) and [Rust](https://www.rust-lang.org/) installed.

### Setup

1. **Install dependencies:**
```bash
npm install
```

2. **Run in development mode:**
```bash
npm run tauri dev
```
> **Note:** Because this app runs strictly in `Accessory` mode, it will **not** open a standard window or show in your Dock. Look toward your macOS top-right menu bar to interact with it!

3. **Build the Standalone Application:**
```bash
npm run tauri build
```
Once the process finishes, you can find the highly optimized compiled application at `src-tauri/target/release/bundle/macos/StatusRelay.app`.

---

## Technical Stack
- **Rust backend**: Handles the HTTP polling via `reqwest` and state diff calculation.
- **Async Runtime**: `tokio` for decoupled background sleeps and HTTP concurrency.
- **Tauri Ecosystem**: Handles the macOS Tray construction and native APIs (`tauri-plugin-notification`).

## Architecture Highlights
The core of this system intercepts Atlassian `v2/status.json` payloads, parses them generically, caches state changes inside a `tokio::sync::Mutex` wrapper, and procedurally overwrites RGBA variables on the window icon to natively embed dynamic macOS "badges". No heavy UI frameworks are loaded!
