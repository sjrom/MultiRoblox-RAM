<div align="center">

<img src="MultiRoblox/src-tauri/icons/128x128.png" width="88" />

# MultiRoblox

**Run as many Roblox accounts as you want, all at once, on Windows.**

[![platform](https://img.shields.io/badge/platform-windows-0078D6?style=for-the-badge&logo=windows&logoColor=white)](https://github.com/sjrom/MultiRoblox-RAM/releases)
[![built with tauri](https://img.shields.io/badge/built%20with-tauri-24C8DB?style=for-the-badge&logo=tauri&logoColor=white)](https://tauri.app/)
[![license](https://img.shields.io/badge/license-PolyForm%20-3DDC97?style=for-the-badge)](LICENSE)
[![latest release](https://img.shields.io/github/v/tag/sjrom/MultiRoblox-RAM?style=for-the-badge&label=latest&color=FF6B6B)](https://github.com/sjrom/MultiRoblox-RAM/releases)

<br>

<img src="assets/screenshot.png" alt="MultiRoblox 2x2 Auto-Aligned Instances with Per-Account 30/15 FPS Cap" width="860" />

</div>

---

## Quick start

No installer needed. Grab the exe from [**Releases**](https://github.com/sjrom/MultiRoblox-RAM/releases) and run it.

Or build it yourself:

```bash
git clone https://github.com/sjrom/MultiRoblox-RAM.git
cd MultiRoblox-RAM/MultiRoblox
build.bat
```

> Requires the [Rust toolchain](https://rustup.rs). The finished exe lands in `dist\release_<version>\MultiRoblox_v<version>.exe`. Rebuilding removes the previous output directory for that version; other versions are kept.

To bump the app version, edit only `[package].version` in `MultiRoblox/src-tauri/Cargo.toml`, then run `build.bat`. Tauri, the release directory, and the release EXE filename use that version. Cargo updates `Cargo.lock` during the build; commit that generated change along with the version bump.

### 🛡️ Windows Defender Exclusion (False Positive Prevention)

Because MultiRoblox extracts its unsigned helper (`RobloxNative.exe`) to `%APPDATA%\multiroblox` and closes Roblox's singleton mutex/event handles across processes, Windows Defender's behavioral ML engine may falsely flag it (`Behavior:Win32/DefenseEvasion.A!ml`). `Add-MpPreference` natively supports `*` wildcards in both `-ExclusionPath` and `-ExclusionProcess`, so a single rule covers `MultiRoblox_v1.2.X.exe` across all versions. Run this command once in an **Administrator PowerShell**:

**中文說明（防止 Windows Defender 誤判隔離）：**
由於 MultiRoblox 會在啟動時將未簽章的輔助程式（`RobloxNative.exe`）釋放至 `%APPDATA%\multiroblox`，並跨處理序關閉 Roblox 的單開限制 Handle，Windows Defender 的機器學習行為偵測可能會誤判為 `Behavior:Win32/DefenseEvasion.A!ml`。`Add-MpPreference` 的 `-ExclusionPath` 與 `-ExclusionProcess` 皆原生支援 `*` 萬用字元，請以**系統管理員身分**開啟 PowerShell 執行以下指令一次，即可自動涵蓋所有 `MultiRoblox_v1.2.X.exe` 版本：

```powershell
Add-MpPreference -ExclusionPath "$env:USERPROFILE\Desktop\MultiRoblox-RAM", "$env:USERPROFILE\Desktop\MultiRoblox-RAM\MultiRoblox\dist\release_*\MultiRoblox_v*.exe", "$env:USERPROFILE\Desktop\MultiRoblox_v*.exe", "$env:APPDATA\multiroblox" -ExclusionProcess "MultiRoblox_v*.exe", "MultiRoblox.exe", "RobloxNative.exe"
```

---

## Features

### 👤 Accounts
- Launch as many accounts side by side as you want, with **Start / Stop** and per-account **Hide / Show** buttons on each running card
- Global **Hide / Show Roblox** sidebar button to hide or restore all running Roblox windows at once to save GPU/memory resources and keep your taskbar clean
- Live **per-account RAM status indicator** on each running account card (hover to view current, average, and peak `MB`), plus a **Windows taskbar badge** on MultiRoblox showing the running instance count
- **Align Roblox** button to neatly arrange running windows into a 2-column grid with one click
- Automatically reconnects to your already-running accounts if you close and reopen MultiRoblox
- Sign in through the Roblox login window, username/password, or paste a cookie directly
- Set a game ID, invite link, or private server link per account so it joins straight into that game
- Per-account **Target FPS** (`15`, `30`, or `60` FPS) and **Graphics Quality** (`Auto`, `Low`, `Medium`, or `High`) settings that apply only to that launched instance without changing your normal Roblox settings
- Nicknames, search, and filtering across all saved accounts
- Cookies encrypted and stored locally on your PC — nothing leaves your device
- Auto-relaunch on unexpected disconnect, back into the same game
- Right-click a running account to hide/show its window, free memory, or adjust its priority

### 📦 Groups
- Bundle accounts into groups (farm squad, trading alts, etc.)
- Launch or close an entire group at once, with a shared or per-account game link

### ⚙️ Settings
- **General**: multi-instance toggle, anti-AFK, auto-relaunch on disconnect, remember window positions
- **Performance**: automatic background RAM trim (enabled by default, plus manual trim) and lower background CPU usage
- **Roblox**: version lock options
- **Data & Privacy**: local encryption key protection and account data reset
- **Themes**: dark and accent color themes

### 🕹️ Anti-AFK
- Keeps your running accounts active in the background (even when minimized or hidden) so they don't get disconnected for being idle after 20 minutes

### 📜 Logs
- Built-in activity log with search (Ctrl+F)

---

## How it works

Roblox prevents multiple instances by holding a Windows mutex. MultiRoblox grabs that mutex first through a lightweight native helper (`RobloxNative.exe`, written in C#), so Roblox opens a fresh instance every time. Each account gets its own auth ticket before launch, so they all sign in as different accounts.

Login opens a native, chromeless window (no external browser involved) that reads the session cookie directly once you sign in.

If the native helper isn't shipped with a build, it compiles from the bundled source using the .NET Framework `csc.exe` already on every Windows machine.

---

## Disclaimer / 免責聲明

### English

This project is an unofficial modified version maintained by [sjrom](https://github.com/sjrom), based on the original project by [PookiePepelsss](https://github.com/PookiePepelsss/MultiRoblox-RAM). It is not developed, sponsored, endorsed, or supported by Roblox Corporation. Roblox and related trademarks belong to their respective owners.

Users must comply with the [Roblox Terms of Use](https://en.help.roblox.com/hc/en-us/articles/115004647846-Roblox-Terms-of-Use), [Community Standards](https://en.help.roblox.com/hc/en-us/articles/203313410-Roblox-Community-Standards), and the rules of each experience. This project does not guarantee that its features comply with Roblox policies or that using it will avoid account restrictions, suspension, or other enforcement actions. Do not use it for cheating, ban evasion, unauthorized access, or other prohibited activities.

This disclaimer does not grant permission from Roblox or override applicable terms or law. Report issues with this fork to [this repository](https://github.com/sjrom/MultiRoblox-RAM/issues), rather than requesting support from the original author or Roblox.

### 繁體中文

本專案為 [sjrom](https://github.com/sjrom) 維護的非官方修改版本，源自 [PookiePepelsss 的原始專案](https://github.com/PookiePepelsss/MultiRoblox-RAM)，並非由 Roblox Corporation 開發、贊助、認可或提供支援。
Roblox 名稱及相關商標屬其各自權利人所有。

使用者必須遵守 [Roblox 使用條款](https://en.help.roblox.com/hc/en-us/articles/115004647846-Roblox-Terms-of-Use)、[社群規範](https://en.help.roblox.com/hc/en-us/articles/203313410-Roblox-Community-Standards)及各遊戲規則。
本專案不保證其功能符合 Roblox 政策，也不保證使用後不會受到帳號限制、停權或其他處置。請勿用於作弊、規避停權、未經授權存取或其他違規用途。

本聲明不代表取得 Roblox 的許可，亦不取代適用條款或法律。
此 fork 的問題請向[本 repository](https://github.com/sjrom/MultiRoblox-RAM/issues) 回報，請勿向原作者或 Roblox 尋求此修改版本的技術支援。

---

## License

Free to use, modify, and share. You may not sell it, and any copy or fork must credit **pookiepepelss** as the original author.

See [LICENSE](LICENSE) for the full text.

<div align="center">

Not affiliated with Roblox Corporation.

</div>
