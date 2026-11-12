//! Per-user registration in Windows (HKCU only, no admin rights needed):
//! Win+R "markdown", Start menu, "Open with", file associations, Default Apps,
//! "Open with Markdown" context menu and an entry in "Apps & features".

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use winreg::RegKey;
use winreg::enums::*;

use crate::settings::{self, tr};

pub const MD_EXTS: &[&str] = &["md", "markdown", "mdown", "mkd", "mkdn", "mdwn", "mdtxt", "mdtext"];
pub const TEXT_EXTS: &[&str] = &[
    "txt", "text", "log", "ini", "cfg", "conf", "json", "jsonc", "yaml", "yml", "toml", "xml", "csv", "tsv",
    "env", "properties", "nfo", "gitignore", "gitattributes", "editorconfig",
];

const PROGID_MD: &str = "Markdown.Document";
const PROGID_TXT: &str = "Markdown.Text";
const APP_PATHS: &str = r"Software\Microsoft\Windows\CurrentVersion\App Paths\markdown.exe";
const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Markdown";
const CAPABILITIES: &str = r"Software\Markdown\Capabilities";
const CONTEXT_MENU: &str = r"Software\Classes\*\shell\Markdown";
const APPLICATION: &str = r"Software\Classes\Applications\Markdown.exe";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DETACHED_PROCESS: u32 = 0x0000_0008;

pub fn install_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("Programs").join("Markdown")
}

fn start_menu_shortcut() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .map(|a| PathBuf::from(a).join(r"Microsoft\Windows\Start Menu\Programs\Markdown.lnk"))
}

pub fn is_installed() -> bool {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(APP_PATHS)
        .and_then(|k| k.get_value::<String, _>(""))
        .map(|p| Path::new(&p).exists())
        .unwrap_or(false)
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Copy the running exe to %LOCALAPPDATA%\Programs\Markdown and register it.
pub fn install(lang: &str) -> Result<PathBuf, String> {
    let src = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = install_dir();
    let exe = dir.join("Markdown.exe");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if !same_file(&src, &exe) {
        if let Err(e) = std::fs::copy(&src, &exe) {
            return Err(if e.raw_os_error() == Some(32) { tr(lang, "in_use").to_string() } else { e.to_string() });
        }
    }
    register(&exe, &dir, lang).map_err(|e| e.to_string())?;
    if let Some(lnk) = start_menu_shortcut() {
        create_shortcut(&lnk, &exe, &dir, tr(lang, "app_desc"));
    }
    notify_shell();
    Ok(exe)
}

fn register(exe: &Path, dir: &Path, lang: &str) -> std::io::Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let exe_s = exe.display().to_string();
    let open_cmd = format!("\"{exe_s}\" \"%1\"");
    let icon = format!("\"{exe_s}\",0");

    // Win+R "markdown" (same mechanism as "paintdotnet")
    let (k, _) = hkcu.create_subkey(APP_PATHS)?;
    k.set_value("", &exe_s)?;
    k.set_value("Path", &dir.display().to_string())?;

    // ProgIDs
    for (progid, name) in [(PROGID_MD, tr(lang, "doc_type")), (PROGID_TXT, tr(lang, "text_type"))] {
        let (k, _) = hkcu.create_subkey(format!(r"Software\Classes\{progid}"))?;
        k.set_value("", &name)?;
        k.set_value("FriendlyTypeName", &name)?;
        let (i, _) = k.create_subkey("DefaultIcon")?;
        i.set_value("", &icon)?;
        let (c, _) = k.create_subkey(r"shell\open\command")?;
        c.set_value("", &open_cmd)?;
    }

    // Extensions: always in "Open with"; .md also becomes default when nothing else claimed it.
    for ext in MD_EXTS {
        let (k, _) = hkcu.create_subkey(format!(r"Software\Classes\.{ext}"))?;
        let (o, _) = k.create_subkey("OpenWithProgids")?;
        o.set_value(PROGID_MD, &"")?;
        let current: String = k.get_value("").unwrap_or_default();
        if current.is_empty() {
            k.set_value("", &PROGID_MD)?;
        }
        if k.get_value::<String, _>("Content Type").is_err() {
            k.set_value("Content Type", &"text/markdown")?;
        }
        if k.get_value::<String, _>("PerceivedType").is_err() {
            k.set_value("PerceivedType", &"text")?;
        }
    }
    for ext in TEXT_EXTS {
        let (k, _) = hkcu.create_subkey(format!(r"Software\Classes\.{ext}\OpenWithProgids"))?;
        k.set_value(PROGID_TXT, &"")?;
    }

    // Applications\Markdown.exe — name and verbs for "Open with"
    let (k, _) = hkcu.create_subkey(APPLICATION)?;
    k.set_value("FriendlyAppName", &"Markdown")?;
    let (c, _) = k.create_subkey(r"shell\open\command")?;
    c.set_value("", &open_cmd)?;
    let (i, _) = k.create_subkey("DefaultIcon")?;
    i.set_value("", &icon)?;
    let (s, _) = k.create_subkey("SupportedTypes")?;
    for ext in MD_EXTS.iter().chain(TEXT_EXTS) {
        s.set_value(format!(".{ext}"), &"")?;
    }

    // Right-click any file → "Open with Markdown"
    let (k, _) = hkcu.create_subkey(CONTEXT_MENU)?;
    k.set_value("", &tr(lang, "verb"))?;
    k.set_value("MUIVerb", &tr(lang, "verb"))?;
    k.set_value("Icon", &icon)?;
    let (c, _) = k.create_subkey("command")?;
    c.set_value("", &open_cmd)?;

    // Settings → Default apps
    let (k, _) = hkcu.create_subkey(CAPABILITIES)?;
    k.set_value("ApplicationName", &"Markdown")?;
    k.set_value("ApplicationDescription", &tr(lang, "app_desc"))?;
    k.set_value("ApplicationIcon", &icon)?;
    let (fa, _) = k.create_subkey("FileAssociations")?;
    for ext in MD_EXTS {
        fa.set_value(format!(".{ext}"), &PROGID_MD)?;
    }
    for ext in TEXT_EXTS {
        fa.set_value(format!(".{ext}"), &PROGID_TXT)?;
    }
    let (ra, _) = hkcu.create_subkey(r"Software\RegisteredApplications")?;
    ra.set_value("Markdown", &CAPABILITIES)?;

    // Apps & features
    let (k, _) = hkcu.create_subkey(UNINSTALL)?;
    k.set_value("DisplayName", &"Markdown")?;
    k.set_value("DisplayIcon", &format!("{exe_s},0"))?;
    k.set_value("DisplayVersion", &env!("CARGO_PKG_VERSION"))?;
    k.set_value("Publisher", &"Markdown")?;
    k.set_value("InstallLocation", &dir.display().to_string())?;
    k.set_value("UninstallString", &format!("\"{exe_s}\" --uninstall"))?;
    k.set_value("QuietUninstallString", &format!("\"{exe_s}\" --uninstall --quiet"))?;
    k.set_value("NoModify", &1u32)?;
    k.set_value("NoRepair", &1u32)?;
    let size_kb = std::fs::metadata(exe).map(|m| (m.len() / 1024) as u32).unwrap_or(0);
    k.set_value("EstimatedSize", &size_kb)?;
    Ok(())
}

pub fn uninstall() -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for key in [
        APP_PATHS.to_string(),
        UNINSTALL.to_string(),
        r"Software\Markdown".to_string(),
        CONTEXT_MENU.to_string(),
        APPLICATION.to_string(),
        format!(r"Software\Classes\{PROGID_MD}"),
        format!(r"Software\Classes\{PROGID_TXT}"),
    ] {
        let _ = hkcu.delete_subkey_all(&key);
    }
    if let Ok(k) = hkcu.open_subkey_with_flags(r"Software\RegisteredApplications", KEY_ALL_ACCESS) {
        let _ = k.delete_value("Markdown");
    }
    for ext in MD_EXTS.iter().chain(TEXT_EXTS) {
        if let Ok(k) = hkcu.open_subkey_with_flags(format!(r"Software\Classes\.{ext}"), KEY_ALL_ACCESS) {
            if let Ok(o) = k.open_subkey_with_flags("OpenWithProgids", KEY_ALL_ACCESS) {
                let _ = o.delete_value(PROGID_MD);
                let _ = o.delete_value(PROGID_TXT);
            }
            if k.get_value::<String, _>("").map(|v| v == PROGID_MD).unwrap_or(false) {
                let _ = k.delete_value("");
            }
        }
    }
    if let Some(lnk) = start_menu_shortcut() {
        let _ = std::fs::remove_file(lnk);
    }
    notify_shell();

    // Files: the exe may be running (this process), so delete after it exits.
    let dirs = [install_dir(), settings::data_dir()];
    let list = dirs.iter().map(|d| format!("'{}'", ps_escape(&d.display().to_string()))).collect::<Vec<_>>().join(",");
    let script = format!(
        "for($i=0;$i -lt 120;$i++){{Start-Sleep -Milliseconds 500;Remove-Item -LiteralPath {list} -Recurse -Force -ErrorAction SilentlyContinue;if(-not (Test-Path -LiteralPath {first})){{break}}}}",
        first = format_args!("'{}'", ps_escape(&dirs[0].display().to_string()))
    );
    let _ = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn();
    Ok(())
}

fn ps_escape(s: &str) -> String {
    s.replace('\'', "''")
}

fn create_shortcut(lnk: &Path, exe: &Path, dir: &Path, desc: &str) {
    if let Some(parent) = lnk.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}');$s.TargetPath='{}';$s.WorkingDirectory='{}';$s.IconLocation='{},0';$s.Description='{}';$s.Save()",
        ps_escape(&lnk.display().to_string()),
        ps_escape(&exe.display().to_string()),
        ps_escape(&dir.display().to_string()),
        ps_escape(&exe.display().to_string()),
        ps_escape(desc),
    );
    let _ = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

fn notify_shell() {
    use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED as _, SHCNF_IDLIST, std::ptr::null(), std::ptr::null()) };
}
