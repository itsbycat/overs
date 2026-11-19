use crate::model::*;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};
use sysinfo::System;
use winreg::enums::*;

/*
 * Run the program's own uninstaller and wait for it and any helper processes
 * (self-copying uninstallers spawn a child from %TEMP% or the install dir and exit).
 */
pub fn run_uninstaller(p: &Program) -> Result<(), String> {
    if p.uninstall.contains("--force-uninstall") {
        kill_edge();
    }
    let mut child = Command::new("cmd")
        .raw_arg(format!("/C {}", p.uninstall))
        .spawn()
        .map_err(|e| e.to_string())?;
    child.wait().map_err(|e| e.to_string())?;
    wait_for_children(p);
    Ok(())
}

// Edge's installer refuses to run while browser / WebView2 processes are alive.
fn kill_edge() {
    let Ok(mut sys) = System::new() else { return };
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    for pr in sys.processes().values() {
        let n = pr.name().to_string_lossy().to_lowercase();
        if n == "msedge.exe" || n == "msedgewebview2.exe" {
            pr.kill();
        }
    }
    std::thread::sleep(Duration::from_secs(1));
}

fn wait_for_children(p: &Program) {
    let temp = std::env::temp_dir().to_string_lossy().to_lowercase();
    let loc = p
        .install_location
        .as_ref()
        .map(|l| l.to_string_lossy().to_lowercase());
    let me = std::process::id();
    let start = Instant::now();
    std::thread::sleep(Duration::from_secs(2));
    let Ok(mut sys) = System::new() else {
        return;
    };
    while start.elapsed() < Duration::from_secs(300) {
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        let busy = sys.processes().values().any(|pr| {
            if pr.pid().as_u32() == me {
                return false;
            }
            let Some(exe) = pr.exe() else { return false };
            let e = exe.to_string_lossy().to_lowercase();
            let name = e.rsplit('\\').next().unwrap_or("");
            let looks_uninst =
                name.contains("unins") || name.contains("uninst") || name.contains("setup");
            loc.as_ref().map_or(false, |l| e.starts_with(l.as_str()))
                || (e.starts_with(&temp) && looks_uninst)
        });
        if !busy {
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

// Delete one trace. Returns a human-readable outcome.
pub fn delete(t: &Trace) -> Result<String, String> {
    match t {
        Trace::Path { path, dir } => {
            if protected(path) {
                return Err("refused: protected location".into());
            }
            if !path.exists() {
                return Ok("already gone".into());
            }
            let r = if *dir {
                std::fs::remove_dir_all(path)
            } else {
                std::fs::remove_file(path)
            };
            match r {
                Ok(()) => Ok("deleted".into()),
                Err(_) => {
                    schedule_reboot(path)?;
                    Ok("locked: scheduled for removal at reboot".into())
                }
            }
        }
        Trace::RegValue {
            hive,
            view32,
            path,
            name,
        } => {
            let k = hive
                .key()
                .open_subkey_with_flags(path, KEY_ALL_ACCESS | view_flag(*view32))
                .map_err(|e| e.to_string())?;
            match k.delete_value(name) {
                Ok(()) => Ok("deleted".into()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok("already gone".into()),
                Err(e) => Err(e.to_string()),
            }
        }
        Trace::Reg { hive, view32, path } => {
            let (parent, leaf) = path.rsplit_once('\\').ok_or("bad key path")?;
            let pk = hive
                .key()
                .open_subkey_with_flags(parent, KEY_ALL_ACCESS | view_flag(*view32))
                .map_err(|e| e.to_string())?;
            match pk.delete_subkey_all(leaf) {
                Ok(()) => Ok("deleted".into()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok("already gone".into()),
                Err(e) => Err(e.to_string()),
            }
        }
    }
}

fn wide(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}

// MoveFileExW(path, NULL, MOVEFILE_DELAY_UNTIL_REBOOT); directories: children first.
fn schedule_reboot(p: &Path) -> Result<(), String> {
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_DELAY_UNTIL_REBOOT, MoveFileExW};
    if p.is_dir() {
        if let Ok(rd) = std::fs::read_dir(p) {
            for e in rd.flatten() {
                let _ = schedule_reboot(&e.path());
            }
        }
    }
    let w = wide(p);
    // SAFETY: `w` is a valid NUL-terminated UTF-16 string that outlives the call.
    let ok = unsafe { MoveFileExW(w.as_ptr(), std::ptr::null(), MOVEFILE_DELAY_UNTIL_REBOOT) };
    if ok == 0 {
        Err("could not schedule reboot removal (admin rights needed?)".into())
    } else {
        Ok(())
    }
}
