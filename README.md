# ove.rs (`overs`)

A fast, lightweight terminal uninstaller and leftover cleaner for Windows, built in Rust.

Windows uninstallers love leaving behind folders in `AppData`, stray Start Menu shortcuts, and forgotten registry keys. **ove.rs** lets you browse installed programs, run their uninstallers (or bypass broken ones), inspect everything an app touches, and review leftovers before deleting them—all from your terminal.

---

## Features

- **Complete program list** — Reads all four Windows Uninstall registry views (`HKLM` / `HKCU`, 64-bit & 32-bit), deduplicates entries, and recovers missing install paths automatically.
- **Uninstall + leftover scan** — Runs the program's native uninstaller, waits for it (and any spawned helper processes in `%TEMP%` or the install folder) to finish, then scans for leftover files, shortcuts, and registry keys.
- **Force remove** — Skip broken or missing uninstallers and jump straight to scanning and cleaning up leftovers.
- **Deep "Find Related" inspection** — Audit everything tied to an app before touching a single file: install directory, `AppData` / `LocalAppData` / `ProgramData` / `Documents`, Desktop & Start Menu shortcuts, `Software` registry keys, registry values pointing into the install folder, `App Paths`, and Windows Installer (MSI) records. Export the full list to a `.txt` file with one keystroke.
- **Interactive review before deletion** — Nothing is deleted blindly. Every leftover appears in a checklist so you can toggle individual items on or off.
- **Handles stubborn apps & locked files**:
  - Automatically switches MSI uninstall commands from `/I` to `/X`.
  - Closes running Microsoft Edge / WebView2 processes and passes `--force-uninstall` when uninstalling Edge/WebView2.
  - Schedules locked files and folders for automatic removal on the next reboot (`MoveFileExW`).
- **Built-in safety guardrails** — Refuses to claim or delete protected directories (`Windows`, `Program Files`, `AppData`, `Documents`, `Desktop`, etc.), skips shared OS/vendor folders (`Microsoft`, `Google`, `Adobe`, `Intel`, `NVIDIA`, `AMD`, etc.), and warns before removing `[SYS]` system components.

---

## Requirements

- **Windows** (uses the Windows Registry and Win32 file APIs)
- **Rust 1.85+** (Edition 2024)

> **Tip:** Run `overs` from an **Administrator** terminal (Windows Terminal / PowerShell) if you want to remove system-wide (`HKLM`) registry entries, delete folders inside `Program Files`, or schedule locked files for removal at reboot.

---

## Getting Started

Clone the repo and run it with Cargo:

```powershell
git clone https://github.com/itsbycat/overs.git
cd overs
cargo run --release
```

Or install it locally so you can run `overs` from anywhere:

```powershell
cargo install --path .
overs
```

---

## Controls

### Program List

| Key | Action |
| --- | --- |
| `↑` / `↓` or `j` / `k` | Move selection |
| `PageUp` / `PageDown` | Jump 10 items |
| `/` | Filter by program name or publisher (`Enter` or `Esc` when done) |
| `s` | Show / hide system components (`[SYS]`) |
| `i` | Find all related files & registry entries |
| `Enter` | Uninstall selected program, then scan for leftovers |
| `f` | Force remove (skip uninstaller, scan leftovers only) |
| `r` | Reload the installed program list |
| `q` | Quit |

### Leftovers Checklist

| Key | Action |
| --- | --- |
| `↑` / `↓` or `j` / `k` | Move selection |
| `Space` | Toggle item on / off |
| `a` | Select all / none |
| `Enter` | Delete all checked items |
| `Esc` | Cancel and go back |

### Related Locations View (`i`)

| Key | Action |
| --- | --- |
| `↑` / `↓` or `j` / `k` | Move selection |
| `PageUp` / `PageDown` | Jump 10 items |
| `w` | Save report to `<program_name>_related.txt` |
| `Enter` | Open in checklist to select items for deletion |
| `Esc` / `q` | Back to program list |

---

## What Gets Scanned?

When scanning for leftovers or related locations, `ove.rs` checks:

- **Filesystem**: The app's install folder, plus matching folders in `%APPDATA%`, `%LOCALAPPDATA%`, `%ProgramData%`, `%USERPROFILE%\Documents`, and user/all-users Start Menu `Programs` folders.
- **Shortcuts**: Matching `.lnk` files on your Desktop, Public Desktop, and Start Menu.
- **Registry**: The app's `Uninstall` entry, matching keys under `HKCU\Software` and `HKLM\Software` (both 64-bit and 32-bit views).
- **Deep scan (`i`)**: Additionally walks `Software` for registry values referencing the install path, checks `App Paths`, and looks up packed Windows Installer (MSI) product keys.

Each entry is tagged so you know exactly what you're looking at:

- `[DIR]` / `[FILE]` — Folder or file on disk
- `[REG]` / `[REG32]` — Registry key (64-bit / 32-bit view)
- `[VALUE]` / `[VALUE32]` — Individual registry value

---

## Project Structure

```text
src/
├── main.rs       # Terminal UI (ratatui + crossterm) and event loop
├── discover.rs   # Enumerates & deduplicates installed apps from the registry
├── leftove.rs    # Filesystem, shortcut, registry, App Paths, and MSI scanning
├── remove.rs     # Uninstaller execution, process waiting, deletion & reboot scheduling
└── model.rs      # Core types, loose name matching, and protected-path safeguards
```

---

## Disclaimer

`ove.rs` modifies your filesystem and Windows Registry. While it includes protected-path checks and always lets you review items before deletion, **double-check the checklist**—especially when using force-remove (`f`) or touching `[SYS]` system components.
