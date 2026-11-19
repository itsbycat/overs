use crate::model::*;
use std::path::PathBuf;
use winreg::RegKey;
use winreg::enums::*;

/// Enumerate the four Uninstall registry views (HKLM/HKCU x 64/32-bit), dedupe, sort.
pub fn load() -> Vec<Program> {
    let mut out: Vec<Program> = Vec::new();
    for hive in [Hive::Lm, Hive::Cu] {
        for v32 in [false, true] {
            let flags = KEY_READ | view_flag(v32);
            let Ok(root) = hive.key().open_subkey_with_flags(UNINSTALL, flags) else {
                continue;
            };
            for kn in root.enum_keys().flatten() {
                let Ok(k) = root.open_subkey_with_flags(&kn, flags) else {
                    continue;
                };
                if let Some(p) = read(&k, hive, v32, &kn) {
                    merge(&mut out, p);
                }
            }
        }
    }
    out.sort_by_key(|p| p.name.to_lowercase());
    out
}

fn s(k: &RegKey, n: &str) -> Option<String> {
    k.get_value::<String, _>(n)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn read(k: &RegKey, hive: Hive, v32: bool, key_name: &str) -> Option<Program> {
    let name = s(k, "DisplayName")?;
    let system = k.get_value::<u32, _>("SystemComponent").unwrap_or(0) == 1;
    if k.get_value::<String, _>("ParentKeyName").is_ok() {
        return None; // updates / patches
    }
    let mut uninstall = s(k, "UninstallString").or_else(|| s(k, "QuietUninstallString"))?;

    // MSI: make sure removal uses /X rather than /I.
    let low = uninstall.to_lowercase();
    if low.contains("msiexec") {
        uninstall = uninstall.replace("/I{", "/X{").replace("/i{", "/X{");
    }

    uninstall = edge_force(uninstall);

    let explicit = s(k, "InstallLocation")
        .or_else(|| s(k, "Inno Setup: App Path"))
        .or_else(|| s(k, "InstallDir"))
        .map(|v| PathBuf::from(v.trim_matches('"')))
        .filter(|p| p.is_dir() && !protected(p));

    let install_location = explicit.or_else(|| recover(&uninstall, s(k, "DisplayIcon")));

    Some(Program {
        name,
        publisher: s(k, "Publisher"),
        version: s(k, "DisplayVersion"),
        install_location,
        uninstall,
        size_kb: k.get_value::<u32, _>("EstimatedSize").unwrap_or(0) as u64,
        hive,
        view32: v32,
        key_name: key_name.to_string(),
        system,
    })
}

/*
 * Microsoft Edge / WebView2 ship an uninstall command (`setup.exe --uninstall --system-level`)
 * that Edge's own installer refuses on most Windows builds. Its `--force-uninstall` switch
 * skips that check, so add it when the command targets Edge or WebView2.
 */
fn edge_force(mut cmd: String) -> String {
    let low = cmd.to_lowercase();
    let edge = low.contains(r"\microsoft\edge\application\")
        || low.contains(r"\microsoft\edgewebview\application\");
    if edge && low.contains("setup.exe") && !low.contains("--force-uninstall") {
        cmd.push_str(" --force-uninstall");
    }
    cmd
}

/// Heuristic InstallLocation recovery from the uninstaller / icon path.
fn recover(uninstall: &str, icon: Option<String>) -> Option<PathBuf> {
    let candidates = [Some(uninstall.to_string()), icon];
    for c in candidates.into_iter().flatten() {
        let Some(exe) = exe_of(c.split(',').next().unwrap_or(&c)) else {
            continue;
        };
        let fname = exe.file_name()?.to_string_lossy().to_lowercase();
        if fname.starts_with("msiexec") || fname.starts_with("rundll32") {
            continue;
        }
        let mut dir = exe.parent()?.to_path_buf();
        // uninstaller living in a sub-folder -> walk up
        let leaf = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if matches!(
            leaf.as_str(),
            "uninstall" | "uninst" | "remove" | "uninstaller" | "bin"
        ) {
            if let Some(p) = dir.parent() {
                dir = p.to_path_buf();
            }
        }
        if dir.is_dir() && !protected(&dir) {
            return Some(dir);
        }
    }
    None
}

/// Keep the richer of two entries describing the same app (32/64-bit, HKLM/HKCU duplicates).
fn merge(list: &mut Vec<Program>, p: Program) {
    if let Some(e) = list
        .iter_mut()
        .find(|e| e.name.eq_ignore_ascii_case(&p.name) && e.version == p.version)
    {
        if e.install_location.is_none() {
            e.install_location = p.install_location;
        }
        e.system &= p.system;
        if e.size_kb == 0 {
            e.size_kb = p.size_kb;
        }
        if e.publisher.is_none() {
            e.publisher = p.publisher;
        }
    } else {
        list.push(p);
    }
}
