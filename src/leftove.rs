use crate::model::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use winreg::enums::*;
use winreg::types::FromRegValue;

fn env(n: &str) -> Option<PathBuf> {
    std::env::var_os(n).map(PathBuf::from)
}

fn matches_any(s: &str, names: &[String]) -> bool {
    names.iter().any(|n| loose_match(s, n))
}

pub fn scan(p: &Program) -> Vec<Trace> {
    let mut out: Vec<Trace> = Vec::new();

    let mut names = vec![p.name.clone()];
    if let Some(leaf) = p.install_location.as_ref().and_then(|l| l.file_name()) {
        names.push(leaf.to_string_lossy().into_owned());
    }
    let publ = p.publisher.as_deref().filter(|x| !shared_name(x));

    // file system
    if let Some(loc) = &p.install_location {
        if loc.exists() && !protected(loc) {
            out.push(Trace::Path {
                path: loc.clone(),
                dir: true,
            });
        }
    }

    let mut roots: Vec<PathBuf> = ["APPDATA", "LOCALAPPDATA", "ProgramData"]
        .iter()
        .filter_map(|v| env(v))
        .collect();
    if let Some(u) = env("USERPROFILE") {
        roots.push(u.join("Documents"));
    }
    // Start Menu program groups
    if let Some(a) = env("APPDATA") {
        roots.push(a.join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Some(a) = env("ProgramData") {
        roots.push(a.join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    for r in &roots {
        dir_scan(r, &names, publ, &mut out);
    }

    // shortcuts (.lnk) by name on Desktop / Start Menu root
    let mut lnk_roots: Vec<PathBuf> = vec![];
    if let Some(u) = env("USERPROFILE") {
        lnk_roots.push(u.join("Desktop"));
    }
    if let Some(u) = env("PUBLIC") {
        lnk_roots.push(u.join("Desktop"));
    }
    lnk_roots.extend(roots.iter().filter(|r| r.ends_with("Programs")).cloned());
    for r in lnk_roots {
        lnk_scan(&r, &names, &mut out);
    }

    // registry
    out.push(Trace::Reg {
        hive: p.hive,
        view32: p.view32,
        path: format!(r"{UNINSTALL}\{}", p.key_name),
    });
    for hive in [Hive::Cu, Hive::Lm] {
        for v32 in [false, true] {
            reg_scan(hive, v32, &names, publ, &mut out);
        }
    }

    // dedupe
    let mut seen = HashSet::new();
    out.retain(|t| seen.insert(t.to_string().to_lowercase()));
    out
}

/*
 * Everything related to the app: the normal leftover scan plus a deep registry walk for values
 * pointing into the install folder, App Paths entries and Windows Installer (MSI) records.
 */
pub fn find_related(p: &Program) -> Vec<Trace> {
    let mut out = scan(p);
    if let Some(loc) = &p.install_location {
        let needle = loc.to_string_lossy().to_lowercase();
        for hive in [Hive::Cu, Hive::Lm] {
            for v32 in [false, true] {
                walk(hive, v32, "Software", 0, &needle, &mut out);
                app_paths(hive, v32, &needle, &mut out);
            }
        }
    }
    msi(p, &mut out);
    let mut seen = HashSet::new();
    out.retain(|t| seen.insert(t.to_string().to_lowercase()));
    out
}

const SKIP: [&str; 9] = [
    r"software\microsoft\windows\currentversion\sidebyside",
    r"software\microsoft\fusion",
    r"software\microsoft\windows\currentversion\installer",
    r"software\microsoft\windows\currentversion\app paths",
    r"software\microsoft\.netframework",
    r"software\microsoft\windows\currentversion\component based servicing",
    r"software\microsoft\windows\currentversion\uninstall",
    r"software\wow6432node",
    r"software\classes",
];

fn walk(hive: Hive, v32: bool, path: &str, depth: usize, needle: &str, out: &mut Vec<Trace>) {
    if depth > 8 || out.len() > 2000 || SKIP.contains(&path.to_lowercase().as_str()) {
        return;
    }
    let Ok(k) = hive
        .key()
        .open_subkey_with_flags(path, KEY_READ | view_flag(v32))
    else {
        return;
    };
    for r in k.enum_values() {
        let Ok((name, v)) = r else { continue };
        if v.vtype == REG_SZ || v.vtype == REG_EXPAND_SZ {
            if let Ok(s) = String::from_reg_value(&v) {
                if s.to_lowercase().contains(needle) {
                    out.push(Trace::RegValue {
                        hive,
                        view32: v32,
                        path: path.to_string(),
                        name,
                    });
                }
            }
        }
    }
    for sub in k.enum_keys().flatten() {
        walk(hive, v32, &format!(r"{path}\{sub}"), depth + 1, needle, out);
    }
}

fn app_paths(hive: Hive, v32: bool, needle: &str, out: &mut Vec<Trace>) {
    let base = r"Software\Microsoft\Windows\CurrentVersion\App Paths";
    let flags = KEY_READ | view_flag(v32);
    let Ok(root) = hive.key().open_subkey_with_flags(base, flags) else {
        return;
    };
    for sub in root.enum_keys().flatten() {
        let Ok(k) = root.open_subkey_with_flags(&sub, flags) else {
            continue;
        };
        let def: String = k.get_value("").unwrap_or_default();
        if def.to_lowercase().contains(needle) {
            out.push(Trace::Reg {
                hive,
                view32: v32,
                path: format!(r"{base}\{sub}"),
            });
        }
    }
}

/// Windows Installer packs a product GUID by reversing hex groups.
fn pack_guid(g: &str) -> Option<String> {
    let h: String = g.trim_matches(|c| c == '{' || c == '}').replace('-', "");
    if h.len() != 32 || !h.is_ascii() {
        return None;
    }
    let rev = |s: &str| s.chars().rev().collect::<String>();
    let mut o = rev(&h[0..8]);
    o += &rev(&h[8..12]);
    o += &rev(&h[12..16]);
    for i in (16..32).step_by(2) {
        o += &rev(&h[i..i + 2]);
    }
    Some(o)
}

fn msi(p: &Program, out: &mut Vec<Trace>) {
    let Some(packed) = pack_guid(&p.key_name) else {
        return;
    };
    let base = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Installer\UserData";
    let flags = KEY_READ | KEY_WOW64_64KEY;
    let lm = Hive::Lm.key();
    if let Ok(ud) = lm.open_subkey_with_flags(base, flags) {
        for sid in ud.enum_keys().flatten() {
            let path = format!(r"{base}\{sid}\Products\{packed}");
            if lm.open_subkey_with_flags(&path, flags).is_ok() {
                out.push(Trace::Reg {
                    hive: Hive::Lm,
                    view32: false,
                    path,
                });
            }
        }
    }
    let path = format!(r"SOFTWARE\Classes\Installer\Products\{packed}");
    if lm.open_subkey_with_flags(&path, flags).is_ok() {
        out.push(Trace::Reg {
            hive: Hive::Lm,
            view32: false,
            path,
        });
    }
}

fn dir_scan(root: &Path, names: &[String], publ: Option<&str>, out: &mut Vec<Trace>) {
    let Ok(rd) = fs::read_dir(root) else { return };
    for e in rd.flatten() {
        let path = e.path();
        if !path.is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if shared_name(&name) {
            continue;
        }
        if matches_any(&name, names) {
            out.push(Trace::Path { path, dir: true });
        } else if publ.map_or(false, |pb| loose_match(&name, pb)) {
            let kids: Vec<PathBuf> = fs::read_dir(&path)
                .map(|r| {
                    r.flatten()
                        .map(|k| k.path())
                        .filter(|k| k.is_dir())
                        .collect()
                })
                .unwrap_or_default();
            let hit = kids.iter().find(|k| {
                k.file_name()
                    .map_or(false, |n| matches_any(&n.to_string_lossy(), names))
            });
            if let Some(h) = hit {
                // publisher folder only contains this app -> take the whole publisher folder
                let target = if kids.len() == 1 {
                    path.clone()
                } else {
                    h.clone()
                };
                out.push(Trace::Path {
                    path: target,
                    dir: true,
                });
            }
        }
    }
}

fn lnk_scan(root: &Path, names: &[String], out: &mut Vec<Trace>) {
    let Ok(rd) = fs::read_dir(root) else { return };
    for e in rd.flatten() {
        let path = e.path();
        let is_lnk = path
            .extension()
            .map_or(false, |x| x.eq_ignore_ascii_case("lnk"));
        if is_lnk {
            if let Some(stem) = path.file_stem() {
                if matches_any(&stem.to_string_lossy(), names) {
                    out.push(Trace::Path { path, dir: false });
                }
            }
        }
    }
}

fn reg_scan(hive: Hive, v32: bool, names: &[String], publ: Option<&str>, out: &mut Vec<Trace>) {
    let flags = KEY_READ | view_flag(v32);
    let Ok(sw) = hive.key().open_subkey_with_flags("Software", flags) else {
        return;
    };
    for sk in sw.enum_keys().flatten() {
        if shared_name(&sk) {
            continue;
        }
        if matches_any(&sk, names) {
            out.push(Trace::Reg {
                hive,
                view32: v32,
                path: format!(r"Software\{sk}"),
            });
        } else if publ.map_or(false, |pb| loose_match(&sk, pb)) {
            let Ok(pk) = sw.open_subkey_with_flags(&sk, flags) else {
                continue;
            };
            let kids: Vec<String> = pk.enum_keys().flatten().collect();
            if let Some(h) = kids.iter().find(|k| matches_any(k, names)) {
                let path = if kids.len() == 1 {
                    format!(r"Software\{sk}")
                } else {
                    format!(r"Software\{sk}\{h}")
                };
                out.push(Trace::Reg {
                    hive,
                    view32: v32,
                    path,
                });
            }
        }
    }
}
