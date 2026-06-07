# 🍅 Pomodoro TUI Discord (Enhanced Fork)

Focus like a pro! A high-performance Terminal User Interface (TUI) Pomodoro timer written in Rust. Sync your sessions with Discord, listen to your favorite YouTube BGM, and track your productivity over time.

> **This is an enhanced fork of [hexbyte16/rust-pomo-discord](https://github.com/hexbyte16/rust-pomo-discord)** with Windows fixes, session tracking, custom activities, persistent settings, and productivity stats.

## What's New in This Fork

🔧 **Windows Fix** — Resolved double-firing key inputs on Windows caused by crossterm emitting both KeyPress and KeyRelease events. Pause, increment, and all keybinds now work correctly on Windows 10/11.

📋 **Session History** — All completed and abandoned sessions are logged locally to a CSV file. Browse your history from a dedicated screen accessible from the main menu. History is always sorted by date with the most recent entries at the top.

✏️ **Add & Edit Sessions** — Manually log sessions you completed away from your PC, or edit existing entries. Full form with date, activity, duration, break time, session count, and completion status.

📅 **Date Editor** — Type the date directly as 8 digits (YYYYMMDD) with dashes auto-inserted. Pre-filled with today's date when adding new entries, or the existing date when editing.

🗑️ **Delete Sessions** — Remove any entry from your session history.

📊 **Session Stats** — View per-activity stats including total sessions, total time spent, entries, average sessions per entry, and completed vs abandoned count. Accessible from the main menu.

🎯 **Custom Activities** — Add your own activities beyond the defaults (e.g. "Actuary Exam 📝"). Custom activities persist across launches and are logged in session history.

✏️ **Rename Activities** — Rename any custom activity and all historical session entries update automatically, so your stats always reflect the current name.

⏸️ **Break Duration Setting** — Customize your break duration from Settings instead of relying on the hardcoded 5/10 minute defaults.

💾 **Persistent Settings** — Theme, break duration, notifications, volume, and custom activities all save to a local config file and load automatically on launch.

🛟 **Crash-Safe Timer** — Timer state saves every second. If the app closes unexpectedly, you'll be prompted to resume your session next time you launch.

📊 **Improved Timer Display** — Timer label uses high-contrast white bold text on a dark background, ensuring the time is always readable regardless of progress bar fill level.

## Features

🎮 **Discord Rich Presence** — Live countdown timer and pause status on your Discord profile.

🎶 **YouTube BGM Importer** — Download and play background music directly from YouTube (requires yt-dlp and ffmpeg).

⌨️ **Vim-Style Navigation** — Full support for HJKL and Arrow keys throughout the app.

🎨 **Custom Themes** — Cyan, Magenta, Green, Yellow, and Red.

🔔 **System Notifications** — Get notified when a session or break ends.

⚡ **Lightweight & Fast** — Built with Rust, consumes less than 10MB of RAM.

## Installation

### Windows

Go to the [Releases](https://github.com/SillyIngenuity/rust-pomo-discord/releases) page. Download the zip, extract, and run `pomodoro-tui-discord.exe`.

### Cargo (All Platforms)

```
cargo install --git https://github.com/SillyIngenuity/rust-pomo-discord
```

### Build from Source

```
git clone https://github.com/SillyIngenuity/rust-pomo-discord
cd rust-pomo-discord
cargo build --release
```

The binary will be at `target/release/pomodoro-tui-discord`.

## Keybinds

### Main Menu
| Key | Action |
|-----|--------|
| J/K or Arrows | Navigate |
| Enter/L/Right | Select |
| A | Add custom activity |
| E | Rename custom activity |
| D | Delete custom activity |
| S | Settings |
| Q | Quit |

### Timer
| Key | Action |
|-----|--------|
| Space | Pause/Resume |
| +/- | Volume up/down |
| M | Mute/Unmute |
| H/Left/Esc | Stop & return to menu |

### Session History
| Key | Action |
|-----|--------|
| J/K | Scroll |
| A | Add manual session |
| E | Edit selected session |
| D | Delete selected session |
| Esc/Q/Left | Back |

### Session Form (Add/Edit)
| Key | Action |
|-----|--------|
| J/K | Select field |
| H/L | Adjust value |
| 0-9 | Type date (on Date field) |
| Backspace | Delete date digit |
| Enter | Save |
| Esc | Cancel |

### Session Stats
| Key | Action |
|-----|--------|
| J/K | Scroll |
| Esc/Q/Left | Back |

### Settings
| Key | Action |
|-----|--------|
| J/K | Select option |
| H/L | Change value |
| Esc/Q | Back |

## Data Storage

All data is stored locally at:

- **Windows:** `%LOCALAPPDATA%\pomodoro-tui\`
- **Linux:** `~/.local/share/pomodoro-tui/`
- **macOS:** `~/Library/Application Support/pomodoro-tui/`

Files:
- `config.ini` — Settings and custom activities
- `sessions.csv` — Session history (used by stats and history screens, auto-sorted by date)
- `timer_state.ini` — Crash recovery state (auto-deleted on normal exit)
- `bgm/` — Downloaded background music

## Privacy

This application is open source. It only communicates locally with your Discord client via IPC. No personal data is collected or sent to any server. Network access is limited to the optional YouTube BGM downloader which uses yt-dlp.

## Credits

Original project by [Islam/Hexbyte](https://github.com/hexbyte16/rust-pomo-discord). Enhanced fork by [SillyIngenuity](https://github.com/SillyIngenuity).

Made with ❤️ and 🦀
