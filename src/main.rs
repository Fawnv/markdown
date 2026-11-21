#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Markdown — a lightweight Notepad replacement that renders Markdown like GitHub.

mod install;
mod render;
mod settings;
mod textio;

use std::borrow::Cow;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use serde_json::{Value, json};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::platform::windows::{IconExtWindows, WindowBuilderExtWindows};
use tao::window::{Icon, Theme, Window, WindowBuilder};
use wry::http::{Request, Response};
use wry::{DragDropEvent, NewWindowResponse, ScrollBarStyle, WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows};

use settings::{Settings, tr};
use textio::{Enc, Eol};

const ORIGIN: &str = "http://app.localhost/";
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

enum UserEvent {
    Ipc(String),
    Rendered(u64, String),
    FileChanged(SystemTime),
    DragEnter,
    DragLeave,
    Drop(Vec<PathBuf>),
}

/// State shared with the custom-protocol handler and the file watcher thread.
#[derive(Default)]
struct Shared {
    path: Option<PathBuf>,
    mtime: Option<SystemTime>,
}

struct Doc {
    path: Option<PathBuf>,
    enc: Enc,
    eol: Eol,
    dirty: bool,
    mtime: Option<SystemTime>,
    warned_changed: bool,
}

impl Doc {
    fn untitled() -> Self {
        Doc { path: None, enc: Enc::Utf8 { bom: false }, eol: Eol::Crlf, dirty: false, mtime: None, warned_changed: false }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let lang = settings::lang();

    if args.iter().any(|a| a.eq_ignore_ascii_case("--install")) {
        match install::install(lang) {
            Ok(_) => message(None, MessageLevel::Info, tr(lang, "installed")),
            Err(e) => message(None, MessageLevel::Error, &format!("{}\n\n{e}", tr(lang, "install_failed"))),
        }
        return;
    }
    if args.iter().any(|a| a.eq_ignore_ascii_case("--uninstall")) {
        let quiet = args.iter().any(|a| a.eq_ignore_ascii_case("--quiet"));
        if quiet || confirm(None, tr(lang, "uninstall_confirm")) {
            let _ = install::uninstall();
            if !quiet {
                message(None, MessageLevel::Info, tr(lang, "uninstalled"));
            }
        }
        return;
    }

    let file = args.iter().find(|a| !a.starts_with("--")).map(|a| {
        let p = PathBuf::from(a);
        if p.is_absolute() { p } else { std::env::current_dir().map(|d| d.join(&p)).unwrap_or(p) }
    });

    render::preload();
    if let Err(e) = run(file, lang) {
        message(None, MessageLevel::Error, &format!("Markdown: {e}"));
    }
}

fn run(initial: Option<PathBuf>, lang: &'static str) -> Result<(), Box<dyn std::error::Error>> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let settings = settings::load();
    let dark = settings.wants_dark();

    let icon = Icon::from_resource(1, None).ok();
    let window = WindowBuilder::new()
        .with_title("Markdown")
        .with_inner_size(LogicalSize::new(settings.width.max(420.0), settings.height.max(300.0)))
        .with_min_inner_size(LogicalSize::new(420.0, 300.0))
        .with_maximized(settings.maximized)
        .with_visible(false)
        .with_theme(Some(if dark { Theme::Dark } else { Theme::Light }))
        .with_window_icon(icon.clone())
        .with_taskbar_icon(icon)
        .build(&event_loop)?;

    let shared = Arc::new(Mutex::new(Shared::default()));
    let shell = include_str!("../ui/index.html")
        .replace("{{THEME}}", if dark { "dark" } else { "light" })
        .replace("{{LANG}}", lang);

    let mut web_context = WebContext::new(Some(settings::data_dir().join("WebView2")));
    let webview = {
        let shared = shared.clone();
        let p_ipc = proxy.clone();
        let p_drop = proxy.clone();
        WebViewBuilder::new_with_web_context(&mut web_context)
            .with_url(ORIGIN)
            .with_custom_protocol("app".into(), move |_id, req| serve(&shell, &shared, req))
            .with_ipc_handler(move |req: Request<String>| {
                let _ = p_ipc.send_event(UserEvent::Ipc(req.into_body()));
            })
            .with_drag_drop_handler(move |ev| {
                let _ = p_drop.send_event(match ev {
                    DragDropEvent::Enter { .. } => UserEvent::DragEnter,
                    DragDropEvent::Drop { paths, .. } => UserEvent::Drop(paths),
                    DragDropEvent::Leave => UserEvent::DragLeave,
                    _ => return true,
                });
                true
            })
            .with_navigation_handler(|url| url.starts_with(ORIGIN))
            .with_new_window_req_handler(|url, _| {
                if is_web_url(&url) {
                    shell_open(OsStr::new(&url));
                }
                NewWindowResponse::Deny
            })
            .with_background_color(if dark { (13, 17, 23, 255) } else { (255, 255, 255, 255) })
            .with_browser_accelerator_keys(false)
            .with_scroll_bar_style(ScrollBarStyle::FluentOverlay)
            .with_devtools(cfg!(debug_assertions))
            .build(&window)?
    };

    let render_tx = spawn_renderer(proxy.clone());
    spawn_watcher(shared.clone(), proxy.clone());

    let mut app = App {
        window,
        webview,
        _web_context: web_context,
        shared,
        render_tx,
        lang,
        doc: Doc::untitled(),
        initial,
        settings,
    };

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::Ipc(msg)) => app.on_ipc(&msg, control_flow),
            Event::UserEvent(UserEvent::Rendered(seq, html)) => {
                app.js(json!({ "type": "rendered", "seq": seq, "html": html }));
            }
            Event::UserEvent(UserEvent::FileChanged(mtime)) => app.on_file_changed(mtime),
            Event::UserEvent(UserEvent::DragEnter) => app.js(json!({ "type": "drag", "over": true })),
            Event::UserEvent(UserEvent::DragLeave) => app.js(json!({ "type": "drag", "over": false })),
            Event::UserEvent(UserEvent::Drop(paths)) => {
                app.js(json!({ "type": "drag", "over": false }));
                if let Some(p) = paths.into_iter().find(|p| p.is_file()) {
                    app.js(json!({ "type": "dropOpen", "path": p }));
                }
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                if app.doc.dirty {
                    app.js(json!({ "type": "close-request" }));
                } else {
                    app.exit(control_flow);
                }
            }
            Event::WindowEvent { event: WindowEvent::Resized(size), .. } => {
                if !app.window.is_maximized() && !app.window.is_minimized() && size.width > 0 {
                    let logical = size.to_logical::<f64>(app.window.scale_factor());
                    app.settings.width = logical.width;
                    app.settings.height = logical.height;
                }
            }
            _ => {}
        }
    });
}

struct App {
    window: Window,
    webview: WebView,
    _web_context: WebContext,
    shared: Arc<Mutex<Shared>>,
    render_tx: mpsc::Sender<(u64, String)>,
    lang: &'static str,
    doc: Doc,
    initial: Option<PathBuf>,
    settings: Settings,
}

impl App {
    fn js(&self, v: Value) {
        let _ = self.webview.evaluate_script(&format!("window.__recv({v})"));
    }

    fn on_ipc(&mut self, raw: &str, cf: &mut ControlFlow) {
        let Ok(v) = serde_json::from_str::<Value>(raw) else { return };
        match v["cmd"].as_str().unwrap_or("") {
            "ready" => {
                self.js(json!({
                    "type": "init",
                    "lang": self.lang,
                    "settings": self.settings.ui_json(),
                    "installed": install::is_installed(),
                }));
                match self.initial.take() {
                    Some(p) => self.load(p, None, false),
                    None => self.new_doc(),
                }
            }
            "shown" => {
                self.window.set_visible(true);
                self.window.set_focus();
                let _ = self.webview.focus();
            }
            "render" => {
                let seq = v["seq"].as_u64().unwrap_or(0);
                let text = v["text"].as_str().unwrap_or("").to_string();
                let _ = self.render_tx.send((seq, text));
            }
            "dirty" => {
                self.doc.dirty = v["value"].as_bool().unwrap_or(false);
                self.update_title();
            }
            "eol" => self.doc.eol = Eol::parse(v["value"].as_str().unwrap_or("crlf")),
            "save" => {
                let text = v["text"].as_str().unwrap_or("");
                let eol = Eol::parse(v["eol"].as_str().unwrap_or("crlf"));
                self.save(text, eol, v["as"].as_bool().unwrap_or(false));
            }
            "open" => {
                if self.confirm_discard(&v)
                    && let Some(p) = self.pick_open()
                {
                    self.load(p, None, false);
                }
            }
            "openPath" => {
                if let Some(p) = v["path"].as_str().map(PathBuf::from)
                    && self.confirm_discard(&v)
                {
                    self.load(p, None, false);
                }
            }
            "new" => {
                if self.confirm_discard(&v) {
                    self.new_doc();
                }
            }
            "newWindow" => spawn_self(None),
            "openLink" => self.open_link(&v),
            "openExternal" => {
                if let Some(url) = v["url"].as_str().filter(|u| is_web_url(u)) {
                    shell_open(OsStr::new(url));
                }
            }
            "reveal" => {
                if let Some(p) = &self.doc.path {
                    reveal_in_explorer(p);
                }
            }
            "close" => {
                if self.confirm_discard(&v) {
                    self.exit(cf);
                }
            }
            "settings" => {
                // Re-read so several open windows don't overwrite each other's changes.
                let mut s = settings::load();
                s.merge_ui(&v["settings"]);
                self.settings.merge_ui(&v["settings"]);
                settings::save(&s);
            }
            "theme" => {
                let dark = v["dark"].as_bool().unwrap_or(false);
                self.window.set_theme(Some(if dark { Theme::Dark } else { Theme::Light }));
            }
            "install" => {
                match install::install(self.lang) {
                    Ok(_) => message(Some(&self.window), MessageLevel::Info, tr(self.lang, "installed")),
                    Err(e) => message(
                        Some(&self.window),
                        MessageLevel::Error,
                        &format!("{}\n\n{e}", tr(self.lang, "install_failed")),
                    ),
                }
                self.js(json!({ "type": "installed", "value": install::is_installed() }));
            }
            "uninstall" => {
                if confirm(Some(&self.window), tr(self.lang, "uninstall_confirm")) {
                    let _ = install::uninstall();
                    message(Some(&self.window), MessageLevel::Info, tr(self.lang, "uninstalled"));
                    self.js(json!({ "type": "installed", "value": false }));
                }
            }
            _ => {}
        }
    }

    fn exit(&mut self, cf: &mut ControlFlow) {
        let mut s = settings::load();
        s.maximized = self.window.is_maximized();
        if !s.maximized {
            s.width = self.settings.width;
            s.height = self.settings.height;
        }
        settings::save(&s);
        *cf = ControlFlow::Exit;
    }

    // ------------------------------------------------------------ documents

    fn display_name(&self) -> String {
        self.doc
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| tr(self.lang, "untitled").to_string())
    }

    fn update_title(&self) {
        let star = if self.doc.dirty { "*" } else { "" };
        self.window.set_title(&format!("{star}{} – Markdown", self.display_name()));
    }

    fn set_path(&mut self, path: Option<PathBuf>, mtime: Option<SystemTime>) {
        self.doc.path = path.clone();
        self.doc.mtime = mtime;
        self.doc.warned_changed = false;
        let mut s = self.shared.lock().unwrap();
        s.path = path;
        s.mtime = mtime;
    }

    fn new_doc(&mut self) {
        self.doc = Doc::untitled();
        self.set_path(None, None);
        self.update_title();
        self.js(json!({
            "type": "doc", "path": null, "name": "", "text": "", "eol": "crlf",
            "encoding": self.doc.enc.label(), "isMarkdown": true,
        }));
    }

    fn load(&mut self, path: PathBuf, fragment: Option<String>, reload: bool) {
        let path = path.canonicalize().map(strip_unc).unwrap_or(path);
        let meta = std::fs::metadata(&path);
        if let Ok(m) = &meta
            && m.len() > MAX_FILE_BYTES
        {
            message(Some(&self.window), MessageLevel::Error, tr(self.lang, "too_large"));
            self.reveal_if_hidden();
            return;
        }
        let decoded = match std::fs::read(&path) {
            Ok(bytes) => textio::decode(&bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !reload => {
                // Like Notepad: opening a missing file starts a new one at that path.
                textio::Decoded { text: String::new(), enc: Enc::Utf8 { bom: false }, eol: Eol::Crlf }
            }
            Err(e) => {
                message(
                    Some(&self.window),
                    MessageLevel::Error,
                    &format!("{}\n{}\n\n{e}", tr(self.lang, "open_failed"), path.display()),
                );
                self.reveal_if_hidden();
                return;
            }
        };
        let mtime = meta.ok().and_then(|m| m.modified().ok());
        self.doc.enc = decoded.enc;
        self.doc.eol = decoded.eol;
        self.doc.dirty = false;
        self.set_path(Some(path.clone()), mtime);
        self.update_title();
        self.js(json!({
            "type": "doc",
            "path": path,
            "name": self.display_name(),
            "text": decoded.text,
            "eol": decoded.eol.as_str(),
            "encoding": decoded.enc.label(),
            "isMarkdown": is_markdown_path(&path),
            "fragment": fragment,
            "reload": reload,
        }));
    }

    /// If loading the very first file failed, still show the (empty) window.
    fn reveal_if_hidden(&mut self) {
        if !self.window.is_visible() {
            self.new_doc();
        }
    }

    fn save(&mut self, text: &str, eol: Eol, save_as: bool) -> bool {
        let path = match (&self.doc.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => match self.pick_save() {
                Some(p) => p,
                None => return false,
            },
        };
        let (bytes, used) = textio::encode(text, self.doc.enc, eol);
        if let Err(e) = std::fs::write(&path, bytes) {
            message(
                Some(&self.window),
                MessageLevel::Error,
                &format!("{}\n{}\n\n{e}", tr(self.lang, "save_failed"), path.display()),
            );
            return false;
        }
        let note = (used != self.doc.enc).then(|| tr(self.lang, "saved_utf8").replace("{}", &self.doc.enc.label()));
        self.doc.enc = used;
        self.doc.eol = eol;
        self.doc.dirty = false;
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        self.set_path(Some(path.clone()), mtime);
        self.update_title();
        self.js(json!({
            "type": "saved",
            "path": path,
            "name": self.display_name(),
            "encoding": used.label(),
            "isMarkdown": is_markdown_path(&path),
            "note": note,
        }));
        true
    }

    /// Ask to save unsaved changes. Returns false if the user cancelled.
    fn confirm_discard(&mut self, v: &Value) -> bool {
        if !self.doc.dirty {
            return true;
        }
        let question = tr(self.lang, "save_changes").replace("{}", &self.display_name());
        let answer = MessageDialog::new()
            .set_title("Markdown")
            .set_description(question)
            .set_level(MessageLevel::Warning)
            .set_buttons(MessageButtons::YesNoCancel)
            .set_parent(&self.window)
            .show();
        match answer {
            MessageDialogResult::Yes => match v["text"].as_str() {
                Some(text) => {
                    let eol = v["eol"].as_str().map(Eol::parse).unwrap_or(self.doc.eol);
                    self.save(text, eol, false)
                }
                None => false,
            },
            MessageDialogResult::No => true,
            _ => false,
        }
    }

    fn on_file_changed(&mut self, mtime: SystemTime) {
        if self.doc.mtime == Some(mtime) {
            return; // our own save
        }
        self.doc.mtime = Some(mtime);
        if self.doc.dirty {
            if !self.doc.warned_changed {
                self.doc.warned_changed = true;
                self.js(json!({ "type": "status", "key": "changedOnDisk" }));
            }
        } else if let Some(p) = self.doc.path.clone() {
            self.load(p, None, true);
        }
    }

    fn open_link(&mut self, v: &Value) {
        let Some(href) = v["href"].as_str() else { return };
        let (target, fragment) = match href.split_once('#') {
            Some((t, f)) => (t, Some(f.to_string())),
            None => (href, None),
        };
        let base = self.doc.path.as_deref().and_then(Path::parent).map(Path::to_path_buf);
        let Some(path) = resolve_local(target, base.as_deref()) else {
            self.js(json!({ "type": "status", "text": tr(self.lang, "not_found").replace("{}", target) }));
            return;
        };
        if path.is_dir() {
            shell_open(path.as_os_str());
        } else if is_markdown_path(&path) || has_ext(&path, install::TEXT_EXTS) {
            if v["newWindow"].as_bool().unwrap_or(false) {
                spawn_self(Some(&path));
            } else if self.confirm_discard(v) {
                self.load(path, fragment, false);
            }
        } else if is_executable(&path) {
            reveal_in_explorer(&path);
        } else {
            shell_open(path.as_os_str());
        }
    }

    // -------------------------------------------------------------- dialogs

    fn dialog_dir(&self) -> Option<PathBuf> {
        self.doc.path.as_deref().and_then(Path::parent).map(Path::to_path_buf)
    }

    fn pick_open(&self) -> Option<PathBuf> {
        let mut text_exts: Vec<&str> = install::TEXT_EXTS.to_vec();
        text_exts.retain(|e| !e.contains('.'));
        let mut d = rfd::FileDialog::new()
            .set_title(tr(self.lang, "open_title"))
            .add_filter(tr(self.lang, "filter_md"), install::MD_EXTS)
            .add_filter(tr(self.lang, "filter_text"), &text_exts)
            .add_filter(tr(self.lang, "filter_all"), &["*"])
            .set_parent(&self.window);
        if let Some(dir) = self.dialog_dir() {
            d = d.set_directory(dir);
        }
        d.pick_file()
    }

    fn pick_save(&self) -> Option<PathBuf> {
        let markdown = self.doc.path.as_deref().map(is_markdown_path).unwrap_or(true);
        let name = match &self.doc.path {
            Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            None => format!("{}.md", tr(self.lang, "untitled")),
        };
        let md = (tr(self.lang, "filter_md"), &["md", "markdown"][..]);
        let txt = (tr(self.lang, "filter_text"), &["txt"][..]);
        let (first, second) = if markdown { (md, txt) } else { (txt, md) };
        let mut d = rfd::FileDialog::new()
            .set_title(tr(self.lang, "save_title"))
            .add_filter(first.0, first.1)
            .add_filter(second.0, second.1)
            .add_filter(tr(self.lang, "filter_all"), &["*"])
            .set_file_name(name)
            .set_parent(&self.window);
        if let Some(dir) = self.dialog_dir() {
            d = d.set_directory(dir);
        }
        let mut p = d.save_file()?;
        if p.extension().is_none() {
            p.set_extension(if markdown { "md" } else { "txt" });
        }
        Some(p)
    }
}

// ------------------------------------------------------------------ threads

fn spawn_renderer(proxy: EventLoopProxy<UserEvent>) -> mpsc::Sender<(u64, String)> {
    let (tx, rx) = mpsc::channel::<(u64, String)>();
    std::thread::spawn(move || {
        while let Ok(mut job) = rx.recv() {
            while let Ok(newer) = rx.try_recv() {
                job = newer; // only the latest text matters
            }
            let html = render::render(&job.1);
            if proxy.send_event(UserEvent::Rendered(job.0, html)).is_err() {
                break;
            }
        }
    });
    tx
}

fn spawn_watcher(shared: Arc<Mutex<Shared>>, proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(800));
            let (path, known) = {
                let s = shared.lock().unwrap();
                (s.path.clone(), s.mtime)
            };
            let (Some(path), Some(known)) = (path, known) else { continue };
            let Ok(m) = std::fs::metadata(&path).and_then(|m| m.modified()) else { continue };
            if m != known {
                shared.lock().unwrap().mtime = Some(m);
                if proxy.send_event(UserEvent::FileChanged(m)).is_err() {
                    break;
                }
            }
        }
    });
}

// --------------------------------------------------------- custom protocol

fn serve(shell: &str, shared: &Mutex<Shared>, req: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let uri = req.uri();
    let (body, mime): (Cow<'static, [u8]>, &str) = match uri.path() {
        "/" | "/index.html" => (Cow::Owned(shell.as_bytes().to_vec()), "text/html; charset=utf-8"),
        "/__app/app.js" => (Cow::Borrowed(include_bytes!("../ui/app.js")), "text/javascript; charset=utf-8"),
        "/__app/app.css" => (Cow::Borrowed(include_bytes!("../ui/app.css")), "text/css; charset=utf-8"),
        "/__app/tokens.css" => (Cow::Borrowed(include_bytes!("../ui/tokens.css")), "text/css; charset=utf-8"),
        "/__app/markdown.css" => (Cow::Borrowed(include_bytes!("../ui/markdown.css")), "text/css; charset=utf-8"),
        "/__file" => {
            let raw = uri
                .query()
                .unwrap_or("")
                .split('&')
                .find_map(|kv| kv.strip_prefix("p="))
                .map(percent_decode)
                .unwrap_or_default();
            let base = shared.lock().unwrap().path.as_deref().and_then(Path::parent).map(Path::to_path_buf);
            match resolve_local(&raw, base.as_deref()).and_then(|p| std::fs::read(&p).ok().map(|b| (p, b))) {
                Some((p, bytes)) => (Cow::Owned(bytes), mime_for(&p)),
                None => return not_found(),
            }
        }
        _ => return not_found(),
    };
    Response::builder()
        .header("Content-Type", mime)
        .header("Cache-Control", "no-store")
        .body(body)
        .unwrap_or_else(|_| not_found())
}

fn not_found() -> Response<Cow<'static, [u8]>> {
    Response::builder().status(404).body(Cow::Borrowed(&b""[..])).unwrap()
}

fn mime_for(p: &Path) -> &'static str {
    match p.extension().and_then(OsStr::to_str).map(str::to_ascii_lowercase).as_deref() {
        Some("png") | Some("apng") => "image/png",
        Some("jpg") | Some("jpeg") | Some("jfif") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("svg") => "image/svg+xml",
        Some("bmp") => "image/bmp",
        Some("ico") => "image/x-icon",
        Some("mp4") | Some("m4v") => "video/mp4",
        Some("webm") => "video/webm",
        Some("ogg") | Some("ogv") => "video/ogg",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        _ => "application/octet-stream",
    }
}

// ------------------------------------------------------------------ helpers

/// Resolve a link/image target from the document to a local path.
fn resolve_local(raw: &str, base: Option<&Path>) -> Option<PathBuf> {
    let raw = raw.split(['?', '#']).next().unwrap_or("").trim();
    if raw.is_empty() {
        return None;
    }
    let candidates: Vec<String> = {
        let decoded = percent_decode(raw);
        if decoded != raw { vec![decoded, raw.to_string()] } else { vec![decoded] }
    };
    for c in candidates {
        let c = c.strip_prefix("file:///").or_else(|| c.strip_prefix("file://")).unwrap_or(&c).to_string();
        let p = PathBuf::from(c.replace('/', "\\"));
        let full = if p.is_absolute() {
            p
        } else if let Some(rest) = c.strip_prefix('/') {
            // GitHub treats "/x" as relative to the repository root.
            let root = base.and_then(repo_root).or(base.map(Path::to_path_buf))?;
            root.join(rest.replace('/', "\\"))
        } else {
            base.map(|b| b.join(&p)).unwrap_or(p)
        };
        if full.exists() {
            return Some(full.canonicalize().map(strip_unc).unwrap_or(full));
        }
    }
    None
}

fn repo_root(start: &Path) -> Option<PathBuf> {
    start.ancestors().find(|d| d.join(".git").exists()).map(Path::to_path_buf)
}

fn strip_unc(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        Some(rest) => PathBuf::from(format!(r"\\{}", &rest[4..])),
        None => p,
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(c: u8) -> Option<u8> {
    (c as char).to_digit(16).map(|d| d as u8)
}

fn has_ext(p: &Path, list: &[&str]) -> bool {
    p.extension()
        .and_then(OsStr::to_str)
        .map(|e| list.iter().any(|x| x.eq_ignore_ascii_case(e)))
        .unwrap_or(false)
}

fn is_markdown_path(p: &Path) -> bool {
    has_ext(p, install::MD_EXTS)
}

fn is_executable(p: &Path) -> bool {
    has_ext(
        p,
        &["exe", "com", "bat", "cmd", "ps1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "msi", "msp", "scr", "pif", "hta", "cpl", "lnk", "reg", "jar", "appx", "msix"],
    )
}

fn is_web_url(u: &str) -> bool {
    let l = u.to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://") || l.starts_with("mailto:") || l.starts_with("ftp://")
}

fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

fn shell_open(target: &OsStr) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let file = wide(target);
    let verb = wide(OsStr::new("open"));
    unsafe {
        ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
    }
}

fn reveal_in_explorer(p: &Path) {
    let _ = Command::new("explorer.exe").raw_arg(format!("/select,\"{}\"", p.display())).spawn();
}

fn spawn_self(file: Option<&Path>) {
    if let Ok(exe) = std::env::current_exe() {
        let mut c = Command::new(exe);
        if let Some(f) = file {
            c.arg(f);
        }
        let _ = c.spawn();
    }
}

fn message(parent: Option<&Window>, level: MessageLevel, text: &str) {
    let mut d = MessageDialog::new().set_title("Markdown").set_description(text).set_level(level).set_buttons(MessageButtons::Ok);
    if let Some(w) = parent {
        d = d.set_parent(w);
    }
    d.show();
}

fn confirm(parent: Option<&Window>, text: &str) -> bool {
    let mut d = MessageDialog::new()
        .set_title("Markdown")
        .set_description(text)
        .set_level(MessageLevel::Warning)
        .set_buttons(MessageButtons::YesNo);
    if let Some(w) = parent {
        d = d.set_parent(w);
    }
    d.show() == MessageDialogResult::Yes
}
