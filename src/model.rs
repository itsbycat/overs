use std::fmt;
use std::path::{Path, PathBuf};
use winreg::RegKey;
use winreg::enums::*;

pub const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hive {
    Lm,
    Cu,
}

impl Hive {
    pub fn key(self) -> RegKey {
        RegKey::predef(match self {
            Hive::Lm => HKEY_LOCAL_MACHINE,
            Hive::Cu => HKEY_CURRENT_USER,
        })
    }
    pub fn short(self) -> &'static str {
        match self {
            Hive::Lm => "HKLM",
            Hive::Cu => "HKCU",
        }
    }
}

pub fn view_flag(v32: bool) -> u32 {
    if v32 {
        KEY_WOW64_32KEY
    } else {
        KEY_WOW64_64KEY
    }
}

#[derive(Clone, Debug)]
pub struct Program {
    pub name: String,
    pub publisher: Option<String>,
    pub version: Option<String>,
    pub install_location: Option<PathBuf>,
    pub uninstall: String,
    pub size_kb: u64,
    pub hive: Hive,
    pub view32: bool,
    pub key_name: String,
    // SystemComponent=1 (e.g. Microsoft Edge, WebView2, runtimes)
    pub system: bool,
}

#[derive(Clone, Debug)]
pub enum Trace {
    Path {
        path: PathBuf,
        dir: bool,
    },
    Reg {
        hive: Hive,
        view32: bool,
        path: String,
    },
    // A single registry value whose data points into the app's install folder.
    RegValue {
        hive: Hive,
        view32: bool,
        path: String,
        name: String,
    },
}

impl fmt::Display for Trace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Trace::Path { path, dir } => {
                write!(
                    f,
                    "{} {}",
                    if *dir { "[DIR] " } else { "[FILE]" },
                    path.display()
                )
            }
            Trace::RegValue {
                hive,
                view32,
                path,
                name,
            } => write!(
                f,
                "[VALUE{}] {}\\{} @ {}",
                if *view32 { "32" } else { "" },
                hive.short(),
                path,
                name
            ),
            Trace::Reg { hive, view32, path } => write!(
                f,
                "[REG{}] {}\\{}",
                if *view32 { "32" } else { "  " },
                hive.short(),
                path
            ),
        }
    }
}

// Lowercase alphanumerics only.
pub fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

// Equality, or bidirectional substring containment when both sides are > 4 chars.
pub fn loose_match(a: &str, b: &str) -> bool {
    let (a, b) = (norm(a), norm(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || (a.len() > 4 && b.len() > 4 && (a.contains(&b) || b.contains(&a)))
}

// Extract the executable path from a command line.
pub fn exe_of(cmd: &str) -> Option<PathBuf> {
    let c = cmd.trim();
    if let Some(rest) = c.strip_prefix('"') {
        return rest.split('"').next().map(PathBuf::from);
    }
    let low = c.to_lowercase();
    if let Some(i) = low.find(".exe") {
        return Some(PathBuf::from(&c[..i + 4]));
    }
    c.split_whitespace().next().map(PathBuf::from)
}

// Locations that must never be treated (or deleted) as an app's private folder.
pub fn protected(p: &Path) -> bool {
    if p.components().count() < 4 {
        return true;
    }
    let fix = |s: String| s.to_lowercase().trim_end_matches('\\').to_string();
    let l = fix(p.to_string_lossy().into_owned());
    if let Ok(w) = std::env::var("windir") {
        let w = fix(w);
        if l == w || l.starts_with(&format!("{w}\\")) {
            return true;
        }
    }
    let mut bad: Vec<String> = [
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "CommonProgramFiles",
        "CommonProgramFiles(x86)",
        "APPDATA",
        "LOCALAPPDATA",
        "ProgramData",
        "USERPROFILE",
        "PUBLIC",
        "TEMP",
        "TMP",
    ]
    .iter()
    .filter_map(|v| std::env::var(v).ok())
    .map(fix)
    .collect();
    if let Ok(u) = std::env::var("USERPROFILE") {
        bad.push(fix(format!("{u}\\Documents")));
        bad.push(fix(format!("{u}\\Desktop")));
    }
    bad.iter().any(|b| *b == l)
}

// Top-level folder names that are shared with the OS / vendors and never claimed.
pub fn shared_name(n: &str) -> bool {
    matches!(
        norm(n).as_str(),
        "microsoft"
            | "windows"
            | "classes"
            | "wow6432node"
            | "policies"
            | "clients"
            | "registeredapplications"
            | "common"
            | "commonfiles"
            | "packages"
            | "temp"
            | "programs"
            | "adobe"
            | "google"
            | "intel"
            | "nvidia"
            | "amd"
    )
}
