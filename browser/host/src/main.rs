#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

//! Zyla Browser host.
//!
//! The window holds one chrome page (the TSGPUI bundle: the Arc-style sidebar with Spaces, favorites,
//! pinned and regular tabs, or a tab strip above the toolbar) covering the whole window, and one child
//! WebView per tab placed to the right of the sidebar and below the toolbar. Only the active tab's WebView is
//! visible, or the two panes of a split view side by side. Pages never talk to the host except through the shortcut navigation in `content.js`, and
//! only the chrome page has an IPC channel.

#[allow(dead_code)]
mod package;
mod address;
mod session;
mod vault;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    borrow::Cow,
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[cfg(windows)]
use tao::platform::windows::IconExtWindows;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    window::{Icon, Window, WindowBuilder},
};
#[cfg(windows)]
use wry::WebViewBuilderExtWindows;
use wry::{
    NewWindowResponse, PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder,
    dpi::{LogicalPosition, LogicalSize as WryLogicalSize},
    http::{Request, Response, header::CONTENT_TYPE},
};
use zeroize::{Zeroize, Zeroizing};

/// Height of the top toolbar (navigation and address row plus its load line). Must match `styles.css`.
const TOP_BAR_HEIGHT: f64 = 46.0;
const BOOKMARK_BAR_HEIGHT: f64 = 34.0;
/// Height of the tab strip shown above the toolbar when tabs are on top. Must match `styles.css`.
const TAB_STRIP_HEIGHT: f64 = 38.0;
const SIDEBAR_DEFAULT_WIDTH: f64 = 260.0;
const SIDEBAR_MIN_WIDTH: f64 = 200.0;
const SIDEBAR_MAX_WIDTH: f64 = 420.0;
/// Tints a Space can take. The chrome page maps each name to its colours.
const TINTS: [&str; 5] = ["graphite", "indigo", "forest", "rose", "amber"];
const CHROME_URL: &str = "tsgpui://localhost/index.html";
const NEW_TAB_URL: &str = "tsgpui://localhost/newtab";
const HISTORY_URL: &str = "tsgpui://localhost/history";
const SETTINGS_URL: &str = "tsgpui://localhost/settings";
const NEW_TAB_HTML: &str = include_str!("../../src/newtab.html");
const HISTORY_HTML: &str = include_str!("../../src/history.html");
const SETTINGS_HTML: &str = include_str!("../../src/settings.html");
const FONT: &[u8] = include_bytes!("../../assets/public-sans-latin.woff2");
const LOGO: &[u8] = include_bytes!("../../assets/logo.png");
const CONTENT_SCRIPT: &str = include_str!("content.js");
const CLOSED_TAB_LIMIT: usize = 20;
/// Space between the two panes of a split view. The chrome background shows through it as a divider.
const SPLIT_GAP: f64 = 4.0;

/// Messages posted into the event loop from other threads or from webview callbacks.
enum UserEvent {
    /// Raw JSON from the chrome page.
    Chrome(String),
    /// A shortcut name typed in a page (see `content.js`).
    Shortcut(String),
    /// A page changed its URL without performing a full navigation (for example, history.pushState).
    UrlChange { id: u32, url: String },
    /// A tab's own page event.
    Tab { id: u32, event: TabEvent },
    /// A page asked to open a link in a new tab (target=_blank, window.open).
    OpenTab(String),
    /// A short message for the chrome page (downloads).
    Notify(String),
}

enum TabEvent {
    Title(String),
    Favicon(String),
    Load { started: bool, url: String },
}

/// Every command the chrome page or a shortcut can send. `cmd` selects the variant.
#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "kebab-case")]
enum Command {
    Ready,
    StartupComplete,
    NewTab,
    CloseTab { id: Option<u32> },
    ReopenTab,
    SelectTab { id: u32 },
    NextTab,
    PrevTab,
    Navigate { text: String },
    Back,
    Forward,
    Reload,
    Stop,
    Home,
    History,
    Settings,
    TabsTop,
    TabsSide,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    #[serde(rename = "devtools")]
    DevTools,
    FocusAddress,
    Bookmark,
    ToggleSidebar,
    SetTabsOnTop { top: bool },
    SplitView,
    SidebarWidth { width: f64 },
    SidebarSide { side: String },
    SelectSpace { id: u32 },
    NewSpace,
    DeleteSpace { id: u32 },
    GetVault,
    SavePassword { host: String, username: String, password: String },
    DeletePassword { id: u64 },
    FillPassword { id: u64 },
    SaveCard { name: String, number: String, exp_month: u8, exp_year: u16 },
    DeleteCard { id: u64 },
    FillCard { id: u64 },
    PickExtension,
    SetTint { tint: String },
    /// `section` is "favorite", "pinned" or "regular"; the tab is placed before `before`, or last.
    MoveTab { id: u32, before: Option<u32>, section: String },
    SetPinned { id: u32, pinned: bool, favorite: bool },
}

struct Tab {
    id: u32,
    webview: WebView,
    title: String,
    url: String,
    favicon: Option<String>,
    loading: bool,
    zoom: f64,
    started: Option<Instant>,
    load_ms: Option<u64>,
    space: u32,
    pinned: bool,
    favorite: bool,
}

struct RestoredTab {
    index: usize,
    saved: session::SavedTab,
    space: u32,
    last_active: bool,
}

/// A Space: its own tabs, tint and sidebar. Switching Spaces keeps every tab alive.
struct Space {
    id: u32,
    name: String,
    tint: String,
    last_active: Option<u32>,
}

struct Browser {
    window: Window,
    chrome: WebView,
    tabs: Vec<Tab>,
    restore_queue: VecDeque<RestoredTab>,
    next_restore_at: Instant,
    active: Option<u32>,
    /// The two tabs shown side by side, left then right. `active` is one of them while this is set.
    split: Option<(u32, u32)>,
    next_id: u32,
    closed: Vec<(String, f64)>,
    spaces: Vec<Space>,
    active_space: u32,
    next_space_id: u32,
    sidebar_visible: bool,
    sidebar_width: f64,
    sidebar_side: String,
    tabs_on_top: bool,
    web_context: WebContext,
    proxy: EventLoopProxy<UserEvent>,
    pages: Arc<RwLock<String>>,
    chrome_ready: bool,
    startup_covering: bool,
    downloads: PathBuf,
    session_path: PathBuf,
    vault_path: PathBuf,
    extensions_dir: PathBuf,
    extensions_enabled: bool,
    dirty: bool,
    last_save: Instant,
    exit: bool,
    bundle_file: Option<PathBuf>,
    bundle_modified: Option<SystemTime>,
}

fn main() -> Result<()> {
    let bundle = package::load().map_err(anyhow::Error::msg)?;
    if bundle.renderer != "webview" {
        bail!("This is the browser host, but the bundle selects renderer '{}'.", bundle.renderer);
    }
    let chrome_html = bundle.webview_html.clone().context("bundle is missing webview_html")?;
    let bundle_file = package::bundle_path().map_err(anyhow::Error::msg)?;
    let bundle_modified = bundle_file.as_deref().and_then(|file| std::fs::metadata(file).and_then(|m| m.modified()).ok());

    let data = session::data_dir();
    let downloads = downloads_dir();
    let extensions_dir = data.join("extensions");
    std::fs::create_dir_all(&data).ok();
    std::fs::create_dir_all(&downloads).ok();
    std::fs::create_dir_all(&extensions_dir).ok();
    let extensions_enabled = extension_count(&extensions_dir) > 0;

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    // The Z is embedded as icon resource 1 by build.rs; the title bar needs it set on the window itself.
    #[cfg(windows)]
    let window_icon = Icon::from_resource(1, None).ok();
    #[cfg(not(windows))]
    let window_icon = None;
    let window = WindowBuilder::new()
        .with_title(bundle.title.clone())
        .with_window_icon(window_icon)
        .with_inner_size(LogicalSize::new(f64::from(bundle.width), f64::from(bundle.height)))
        .with_min_inner_size(LogicalSize::new(480.0, 320.0))
        .build(&event_loop)
        .context("Unable to create the browser window")?;

    let mut web_context = WebContext::new(Some(data.join("WebView2")));
    let pages = Arc::new(RwLock::new(chrome_html));
    let chrome_pages = Arc::clone(&pages);
    let ipc_proxy = proxy.clone();
    // The chrome covers the whole window. It is created first, so the tab pages sit above it.
    let size = window.inner_size().to_logical::<f64>(window.scale_factor());
    let mut chrome_builder = WebViewBuilder::new_with_web_context(&mut web_context);
    #[cfg(windows)]
    {
        chrome_builder = chrome_builder
            .with_browser_extensions_enabled(extensions_enabled)
            .with_extensions_path(extensions_dir.clone());
    }
    let chrome = chrome_builder
        .with_bounds(rect(0.0, 0.0, size.width, size.height))
        .with_url(CHROME_URL)
        .with_custom_protocol("tsgpui".into(), move |_id, request| asset(&chrome_pages, request.uri().path()))
        .with_ipc_handler(move |request: Request<String>| {
            let _ = ipc_proxy.send_event(UserEvent::Chrome(request.into_body()));
        })
        .with_background_color((18, 19, 23, 255))
        .with_hotkeys_zoom(false)
        .with_devtools(true)
        .build_as_child(&window)
        .context("Unable to create the chrome WebView")?;

    let mut browser = Browser {
        window,
        chrome,
        tabs: Vec::new(),
        restore_queue: VecDeque::new(),
        next_restore_at: Instant::now(),
        active: None,
        split: None,
        next_id: 1,
        closed: Vec::new(),
        spaces: vec![Space { id: 1, name: "Personal".into(), tint: TINTS[0].into(), last_active: None }],
        active_space: 1,
        next_space_id: 2,
        sidebar_visible: true,
        sidebar_width: SIDEBAR_DEFAULT_WIDTH,
        sidebar_side: "left".into(),
        tabs_on_top: false,
        web_context,
        proxy,
        pages,
        chrome_ready: false,
        startup_covering: true,
        downloads,
        session_path: data.join("session.json"),
        vault_path: data.join("vault.bin"),
        extensions_dir,
        extensions_enabled,
        dirty: false,
        last_save: Instant::now(),
        exit: false,
        bundle_file,
        bundle_modified,
    };
    browser.restore();

    event_loop.run(move |event, _target, control_flow| {
        let wake_ms = if browser.restore_queue.is_empty() { 500 } else { 100 };
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(wake_ms));
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => browser.exit = true,
            Event::WindowEvent { event: WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. }, .. } => {
                browser.layout();
                browser.push_state();
            }
            Event::UserEvent(user) => browser.on_user_event(user),
            Event::MainEventsCleared => browser.tick(),
            _ => {}
        }
        if browser.exit {
            browser.save_session();
            *control_flow = ControlFlow::Exit;
        }
    });
}

impl Browser {
    fn on_user_event(&mut self, event: UserEvent) {
        match event {
            UserEvent::Chrome(body) => match serde_json::from_str::<Command>(&body) {
                Ok(command) => self.handle(command),
                Err(error) => eprintln!("Zyla Browser: unknown chrome message {body}: {error}"),
            },
            UserEvent::Shortcut(name) => {
                if let Some(number) = name.strip_prefix("tab-").and_then(|digits| digits.parse::<usize>().ok()) {
                    self.select_index(number);
                } else {
                    match serde_json::from_value::<Command>(json!({ "cmd": name })) {
                        Ok(command) => self.handle(command),
                        Err(_) => eprintln!("Zyla Browser: unknown shortcut {name}"),
                    }
                }
            }
            UserEvent::OpenTab(url) => {
                if let Ok(id) = self.open(&url, 1.0) {
                    self.select(id);
                }
            }
            UserEvent::Notify(text) => self.send_chrome(json!({ "type": "notice", "text": text })),
            UserEvent::Tab { id, event } => self.on_tab(id, event),
            UserEvent::UrlChange { id, url } => self.on_url_change(id, url),
        }
    }

    fn on_tab(&mut self, id: u32, event: TabEvent) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else { return };
        match event {
            TabEvent::Title(title) => tab.title = title,
            TabEvent::Favicon(favicon) => {
                if let Ok(parsed) = url::Url::parse(&favicon) {
                    if matches!(parsed.scheme(), "http" | "https") {
                        tab.favicon = Some(parsed.to_string());
                    }
                }
            }
            TabEvent::Load { started: true, url } => {
                tab.loading = true;
                tab.url = url;
                tab.favicon = None;
                tab.started = Some(Instant::now());
                tab.load_ms = None;
            }
            TabEvent::Load { started: false, url } => {
                tab.loading = false;
                tab.url = url;
                tab.load_ms = tab.started.take().map(|start| start.elapsed().as_millis() as u64);
            }
        }
        self.dirty = true;
        self.update_title();
        self.push_state();
    }

    fn on_url_change(&mut self, id: u32, url: String) {
        let Some(parsed) = url::Url::parse(&url).ok().filter(|url| matches!(url.scheme(), "http" | "https")) else { return };
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else { return };
        tab.url = parsed.to_string();
        self.dirty = true;
        self.push_state();
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Ready => {
                self.chrome_ready = true;
                self.push_state();
            }
            Command::StartupComplete => {
                self.startup_covering = false;
                self.layout();
            }
            Command::NewTab => self.open_and_select(NEW_TAB_URL),
            Command::History => self.open_and_select(HISTORY_URL),
            Command::Settings => self.open_and_select(SETTINGS_URL),
            Command::TabsTop => {
                self.tabs_on_top = true;
                self.layout();
                self.dirty = true;
                self.push_state();
            }
            Command::TabsSide => {
                self.tabs_on_top = false;
                self.layout();
                self.dirty = true;
                self.push_state();
            }
            Command::CloseTab { id } => {
                if let Some(id) = id.or(self.active) {
                    self.close(id);
                }
            }
            Command::ReopenTab => {
                if let Some((url, zoom)) = self.closed.pop() {
                    if let Ok(id) = self.open(&url, zoom) {
                        self.select(id);
                    }
                }
            }
            Command::SelectTab { id } => self.select(id),
            Command::NextTab => self.step_tab(1),
            Command::PrevTab => self.step_tab(-1),
            Command::Navigate { text } => match address::resolve(&text) {
                Some(url) => match self.active_tab() {
                    Some(tab) => {
                        let _ = tab.webview.load_url(&url);
                        self.focus_active();
                    }
                    None => self.open_and_select(&url),
                },
                None => {}
            },
            Command::Back => self.eval_active("history.back()"),
            Command::Forward => self.eval_active("history.forward()"),
            Command::Reload => {
                if let Some(tab) = self.active_tab() {
                    let _ = tab.webview.reload();
                }
            }
            Command::Stop => self.eval_active("window.stop()"),
            Command::Home => match self.active_tab() {
                Some(tab) => {
                    let _ = tab.webview.load_url(NEW_TAB_URL);
                }
                None => self.open_and_select(NEW_TAB_URL),
            },
            Command::ZoomIn => self.zoom_active(0.1),
            Command::ZoomOut => self.zoom_active(-0.1),
            Command::ZoomReset => self.zoom_active(0.0),
            Command::DevTools => {
                if let Some(tab) = self.active_tab() {
                    tab.webview.open_devtools();
                }
            }
            Command::FocusAddress => {
                let _ = self.chrome.focus();
                self.send_chrome(json!({ "type": "command", "name": "focus-address" }));
            }
            Command::Bookmark => self.send_chrome(json!({ "type": "command", "name": "bookmark" })),
            Command::ToggleSidebar => {
                self.sidebar_visible = !self.sidebar_visible;
                self.layout();
                self.dirty = true;
                self.push_state();
            }
            Command::SplitView => self.toggle_split(),
            Command::SetTabsOnTop { top } => {
                self.tabs_on_top = top;
                self.layout();
                self.dirty = true;
                self.push_state();
            }
            Command::SidebarWidth { width } => {
                // Dragging the edge sends many widths: only the page layout changes, no state push.
                self.sidebar_width = width.clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
                self.layout();
                self.dirty = true;
            }
            Command::SidebarSide { side } => {
                if side == "left" || side == "right" {
                    self.sidebar_side = side;
                    self.layout();
                    self.dirty = true;
                    self.push_state();
                }
            }
            Command::SelectSpace { id } => self.select_space(id),
            Command::NewSpace => self.new_space(),
            Command::DeleteSpace { id } => self.delete_space(id),
            Command::GetVault => self.send_vault_state(),
            Command::SavePassword { host, username, password } => self.save_password(host, username, password),
            Command::DeletePassword { id } => self.delete_password(id),
            Command::FillPassword { id } => self.fill_password(id),
            Command::SaveCard { name, number, exp_month, exp_year } => self.save_card(name, number, exp_month, exp_year),
            Command::DeleteCard { id } => self.delete_card(id),
            Command::FillCard { id } => self.fill_card(id),
            Command::PickExtension => self.pick_extension(),
            Command::SetTint { tint } => {
                if TINTS.contains(&tint.as_str()) {
                    let active = self.active_space;
                    if let Some(space) = self.spaces.iter_mut().find(|space| space.id == active) {
                        space.tint = tint;
                    }
                }
                self.dirty = true;
                self.push_state();
            }
            Command::MoveTab { id, before, section } => self.move_tab(id, before, &section),
            Command::SetPinned { id, pinned, favorite } => self.set_pinned(id, pinned, favorite),
        }
    }

    fn tick(&mut self) {
        if let Some(file) = self.bundle_file.clone() {
            let changed = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
            if changed.is_some() && changed != self.bundle_modified {
                match package::load_bundle(&file) {
                    Ok(next) => {
                        if let Some(html) = next.webview_html {
                            if let Ok(mut current) = self.pages.write() {
                                *current = html;
                            }
                            self.chrome_ready = false;
                            let _ = self.chrome.evaluate_script("window.location.reload()");
                            self.bundle_modified = changed;
                        }
                    }
                    Err(error) => eprintln!("Zyla Browser reload failed: {error}"),
                }
            }
        }
        if !self.restore_queue.is_empty() && Instant::now() >= self.next_restore_at {
            if let Some(tab) = self.restore_queue.pop_front() {
                let _ = self.open_restored_tab(tab);
            }
            // Stagger WebView creation so a large session does not stall the window or overwhelm
            // WebView2 by starting every saved site at once.
            self.next_restore_at = Instant::now() + Duration::from_millis(200);
        }
        if self.restore_queue.is_empty() && self.dirty && self.last_save.elapsed() > Duration::from_millis(400) {
            self.save_session();
        }
    }

    fn restore(&mut self) {
        let saved = session::load(&self.session_path).filter(|session| !session.tabs.is_empty());
        let Some(session) = saved else {
            self.open_and_select(NEW_TAB_URL);
            return;
        };
        // Sessions saved before Spaces existed keep the default Space.
        if !session.spaces.is_empty() {
            self.spaces = session
                .spaces
                .iter()
                .map(|space| Space { id: space.id, name: space.name.clone(), tint: space.tint.clone(), last_active: None })
                .collect();
            self.next_space_id = self.spaces.iter().map(|space| space.id).max().unwrap_or(0) + 1;
        }
        let first_space = self.spaces[0].id;
        self.active_space = if self.spaces.iter().any(|space| space.id == session.active_space) {
            session.active_space
        } else {
            first_space
        };
        self.sidebar_visible = session.sidebar_visible;
        self.sidebar_width = session.sidebar_width.clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
        self.sidebar_side = if session.sidebar_side == "right" { "right" } else { "left" }.into();
        self.tabs_on_top = session.tabs_on_top;

        let target_index = session
            .tabs
            .get(session.active)
            .filter(|tab| !tab.url.is_empty())
            .map(|_| session.active)
            .or_else(|| session.tabs.iter().rposition(|tab| !tab.url.is_empty()));
        let Some(target_index) = target_index else {
            self.open_and_select(NEW_TAB_URL);
            return;
        };

        let target = self.restored_tab(&session, target_index, first_space);
        for index in 0..session.tabs.len() {
            if index == target_index || session.tabs[index].url.is_empty() { continue; }
            self.restore_queue.push_back(self.restored_tab(&session, index, first_space));
        }

        // Open the last active page first so the browser can become useful immediately. The rest
        // are constructed a little at a time by tick(), preserving their saved order in the tab list.
        match self.open_restored_tab(target) {
            Some(id) => self.select(id),
            None => self.open_and_select(NEW_TAB_URL),
        }
    }

    fn restored_tab(&self, session: &session::Session, index: usize, first_space: u32) -> RestoredTab {
        let saved = session.tabs[index].clone();
        let space = if self.spaces.iter().any(|space| space.id == saved.space) { saved.space } else { first_space };
        let last_active = session.spaces.iter().any(|entry| entry.id == space && entry.last_active == Some(index));
        RestoredTab { index, saved, space, last_active }
    }

    fn open_restored_tab(&mut self, restored: RestoredTab) -> Option<u32> {
        let id = match self.open(&restored.saved.url, restored.saved.zoom) {
            Ok(id) => id,
            Err(error) => {
                eprintln!("Zyla Browser: could not restore {}: {error:#}", restored.saved.url);
                return None;
            }
        };
        let tab = self.tabs.pop()?;
        let mut tab = tab;
        tab.space = restored.space;
        tab.pinned = restored.saved.pinned;
        tab.favorite = restored.saved.pinned && restored.saved.favorite;
        let insert_at = restored.index.min(self.tabs.len());
        self.tabs.insert(insert_at, tab);
        if restored.last_active {
            if let Some(space) = self.spaces.iter_mut().find(|space| space.id == restored.space) {
                space.last_active = Some(id);
            }
        }
        self.dirty = true;
        self.place_tabs();
        self.push_state();
        Some(id)
    }

    fn save_session(&mut self) {
        // Keep the original session intact if the browser closes before background restoration ends.
        if !self.restore_queue.is_empty() { return; }
        let index_of = |id: u32| self.tabs.iter().position(|tab| tab.id == id);
        let session = session::Session {
            tabs: self
                .tabs
                .iter()
                .map(|tab| session::SavedTab {
                    url: tab.url.clone(),
                    zoom: tab.zoom,
                    space: tab.space,
                    pinned: tab.pinned,
                    favorite: tab.favorite,
                })
                .collect(),
            active: self.active.and_then(index_of).unwrap_or(0),
            spaces: self
                .spaces
                .iter()
                .map(|space| session::SavedSpace {
                    id: space.id,
                    name: space.name.clone(),
                    tint: space.tint.clone(),
                    last_active: space.last_active.and_then(index_of),
                })
                .collect(),
            active_space: self.active_space,
            sidebar_visible: self.sidebar_visible,
            sidebar_width: self.sidebar_width,
            sidebar_side: self.sidebar_side.clone(),
            tabs_on_top: self.tabs_on_top,
        };
        if let Err(error) = session::save(&self.session_path, &session) {
            eprintln!("Zyla Browser: could not save the session: {error}");
        }
        self.dirty = false;
        self.last_save = Instant::now();
    }

    /// Creates a child WebView for a tab in the active Space. It starts hidden until `select` shows it.
    fn open(&mut self, url: &str, zoom: f64) -> Result<u32> {
        let id = self.next_id;
        self.next_id += 1;
        let bounds = self.content_rect();
        let pages = Arc::clone(&self.pages);
        let shortcut_proxy = self.proxy.clone();
        let title_proxy = self.proxy.clone();
        let load_proxy = self.proxy.clone();
        let popup_proxy = self.proxy.clone();
        let done_proxy = self.proxy.clone();
        let downloads = self.downloads.clone();
        let extensions_dir = self.extensions_dir.clone();
        let extensions_enabled = self.extensions_enabled;
        let mut builder = WebViewBuilder::new_with_web_context(&mut self.web_context);
        #[cfg(windows)]
        {
            builder = builder
                .with_browser_extensions_enabled(extensions_enabled)
                .with_extensions_path(extensions_dir);
        }
        let webview = builder
            .with_bounds(bounds)
            .with_url(url)
            .with_custom_protocol("tsgpui".into(), move |_id, request| asset(&pages, request.uri().path()))
            .with_initialization_script(CONTENT_SCRIPT)
            .with_hotkeys_zoom(false)
            .with_devtools(true)
            .with_navigation_handler(move |next: String| match next.strip_prefix("tsgpui-cmd://") {
                Some(name) => {
                    if let Some(url) = name.strip_prefix("url?value=") {
                        if let Ok(parsed) = url::Url::parse(&format!("https://zyla.invalid/?{url}")) {
                            if let Some((_, target)) = parsed.query_pairs().find(|(key, _)| key == "value") {
                                let _ = shortcut_proxy.send_event(UserEvent::UrlChange { id, url: target.into_owned() });
                            }
                        }
                    } else if let Some(icon) = name.strip_prefix("favicon?value=") {
                        if let Ok(parsed) = url::Url::parse(&format!("https://zyla.invalid/?{icon}")) {
                            if let Some((_, target)) = parsed.query_pairs().find(|(key, _)| key == "value") {
                                let _ = shortcut_proxy.send_event(UserEvent::Tab {
                                    id,
                                    event: TabEvent::Favicon(target.into_owned()),
                                });
                            }
                        }
                    } else {
                        let _ = shortcut_proxy.send_event(UserEvent::Shortcut(name.to_string()));
                    }
                    false
                }
                None => true,
            })
            .with_new_window_req_handler(move |next: String, _features| {
                if is_sign_in_popup(&next) {
                    return NewWindowResponse::Allow;
                }
                let _ = popup_proxy.send_event(UserEvent::OpenTab(next));
                NewWindowResponse::Deny
            })
            .with_document_title_changed_handler(move |title: String| {
                let _ = title_proxy.send_event(UserEvent::Tab { id, event: TabEvent::Title(title) });
            })
            .with_on_page_load_handler(move |event: PageLoadEvent, next: String| {
                let started = matches!(event, PageLoadEvent::Started);
                let _ = load_proxy.send_event(UserEvent::Tab { id, event: TabEvent::Load { started, url: next } });
            })
            .with_download_started_handler(move |next: String, path: &mut PathBuf| {
                *path = download_target(&downloads, path.as_path(), &next);
                true
            })
            .with_download_completed_handler(move |_next, path: Option<PathBuf>, ok: bool| {
                let name = path
                    .as_deref()
                    .and_then(Path::file_name)
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let text = if ok { format!("Downloaded {name}") } else { format!("Download failed: {name}") };
                let _ = done_proxy.send_event(UserEvent::Notify(text));
            })
            .build_as_child(&self.window)
            .with_context(|| format!("Unable to create a tab for {url}"))?;
        if zoom != 1.0 {
            let _ = webview.zoom(zoom);
        }
        self.tabs.push(Tab {
            id,
            webview,
            title: "New tab".into(),
            url: url.to_string(),
            favicon: None,
            loading: true,
            zoom,
            started: None,
            load_ms: None,
            space: self.active_space,
            pinned: false,
            favorite: false,
        });
        self.dirty = true;
        Ok(id)
    }

    fn open_and_select(&mut self, url: &str) {
        match self.open(url, 1.0) {
            Ok(id) => self.select(id),
            Err(error) => eprintln!("Zyla Browser: could not open {url}: {error:#}"),
        }
    }

    /// Shows one tab and hides every other tab. A tab in another Space switches to that Space.
    /// Selecting one of the two split panes keeps the split; selecting any other tab ends it.
    fn select(&mut self, id: u32) {
        let Some(space) = self.tabs.iter().find(|tab| tab.id == id).map(|tab| tab.space) else { return };
        self.active = Some(id);
        self.active_space = space;
        if let Some(entry) = self.spaces.iter_mut().find(|entry| entry.id == space) {
            entry.last_active = Some(id);
        }
        if !self.in_split(id) {
            self.split = None;
        }
        self.place_tabs();
        self.focus_active();
        self.dirty = true;
        self.update_title();
        self.push_state();
    }

    fn in_split(&self, id: u32) -> bool {
        self.split.is_some_and(|(left, right)| id == left || id == right)
    }

    /// Shows the active tab beside the next tab in its Space (or the previous one when it is last).
    /// With no other tab in the Space, a new tab is opened for the second pane. Toggling again ends it.
    fn toggle_split(&mut self) {
        if self.split.take().is_none() {
            let Some(active) = self.active else { return };
            let other = match self.neighbour_tab(active) {
                Some(id) => id,
                None => match self.open(NEW_TAB_URL, 1.0) {
                    Ok(id) => id,
                    Err(error) => {
                        eprintln!("Zyla Browser: could not open a split pane: {error:#}");
                        return;
                    }
                },
            };
            self.split = Some((active, other));
        }
        self.place_tabs();
        self.dirty = true;
        self.push_state();
    }

    /// The tab after `id` in its Space, or the one before it when `id` is the last.
    fn neighbour_tab(&self, id: u32) -> Option<u32> {
        let space = self.tabs.iter().find(|tab| tab.id == id)?.space;
        let ids: Vec<u32> = self.tabs.iter().filter(|tab| tab.space == space).map(|tab| tab.id).collect();
        let index = ids.iter().position(|candidate| *candidate == id)?;
        ids.get(index + 1).or_else(|| index.checked_sub(1).and_then(|previous| ids.get(previous))).copied()
    }

    /// Ctrl+1..8 select the tab at that place in the sidebar (favorites, pinned, then regular); Ctrl+9 the last.
    fn select_index(&mut self, number: usize) {
        let space = self.active_space;
        let mut ids: Vec<u32> = self.tabs.iter().filter(|t| t.space == space && t.pinned && t.favorite).map(|t| t.id).collect();
        ids.extend(self.tabs.iter().filter(|t| t.space == space && t.pinned && !t.favorite).map(|t| t.id));
        ids.extend(self.tabs.iter().filter(|t| t.space == space && !t.pinned).map(|t| t.id));
        let target = if number == 9 { ids.last().copied() } else { ids.get(number.wrapping_sub(1)).copied() };
        if let Some(id) = target {
            self.select(id);
        }
    }

    fn select_space(&mut self, id: u32) {
        let Some(last) = self.spaces.iter().find(|space| space.id == id).map(|space| space.last_active) else { return };
        self.active_space = id;
        let target = last
            .filter(|tab| self.tabs.iter().any(|candidate| candidate.id == *tab && candidate.space == id))
            .or_else(|| self.tabs.iter().find(|tab| tab.space == id).map(|tab| tab.id));
        match target {
            Some(tab) => self.select(tab),
            None => self.open_and_select(NEW_TAB_URL),
        }
    }

    fn new_space(&mut self) {
        let id = self.next_space_id;
        self.next_space_id += 1;
        let tint = TINTS[id as usize % TINTS.len()];
        self.spaces.push(Space { id, name: format!("Space {id}"), tint: tint.into(), last_active: None });
        self.active_space = id;
        self.open_and_select(NEW_TAB_URL);
    }

    fn delete_space(&mut self, id: u32) {
        let Some(index) = self.spaces.iter().position(|space| space.id == id) else { return };
        if self.spaces.len() <= 1 {
            return;
        }

        let fallback_index = if index + 1 < self.spaces.len() { index + 1 } else { index - 1 };
        let fallback_id = self.spaces[fallback_index].id;
        let was_active = self.active_space == id;
        self.spaces.remove(index);

        let mut tab_index = 0;
        while tab_index < self.tabs.len() {
            if self.tabs[tab_index].space == id {
                let tab = self.tabs.remove(tab_index);
                self.closed.push((tab.url.clone(), tab.zoom));
                if self.closed.len() > CLOSED_TAB_LIMIT {
                    self.closed.remove(0);
                }
                drop(tab);
            } else {
                tab_index += 1;
            }
        }

        if was_active {
            self.active = None;
            self.active_space = fallback_id;
            let target = self
                .spaces
                .iter()
                .find(|space| space.id == fallback_id)
                .and_then(|space| space.last_active)
                .filter(|tab| self.tabs.iter().any(|candidate| candidate.id == *tab && candidate.space == fallback_id))
                .or_else(|| self.tabs.iter().find(|tab| tab.space == fallback_id).map(|tab| tab.id));
            match target {
                Some(tab) => self.select(tab),
                None => self.open_and_select(NEW_TAB_URL),
            }
        } else {
            self.dirty = true;
            self.push_state();
        }
    }

    fn send_vault_state(&self) {
        match vault::load(&self.vault_path) {
            Ok(saved) => {
                let passwords: Vec<Value> = saved.passwords.iter().map(|item| json!({
                    "id": item.id, "host": item.host, "username": item.username,
                })).collect();
                let cards: Vec<Value> = saved.cards.iter().map(|item| json!({
                    "id": item.id, "name": item.name,
                    "lastFour": item.number.chars().rev().take(4).collect::<String>().chars().rev().collect::<String>(),
                    "expMonth": item.exp_month, "expYear": item.exp_year,
                })).collect();
                self.send_chrome(json!({ "type": "vault-state", "passwords": passwords, "cards": cards }));
            }
            Err(error) => self.vault_notice(format!("Could not open the vault: {error:#}")),
        }
    }

    fn save_password(&mut self, host: String, username: String, password: String) {
        let password = Zeroizing::new(password);
        let result = (|| -> Result<()> {
            let host = normalize_site_host(&host)?;
            let username = username.trim().to_string();
            if username.is_empty() || password.is_empty() || username.len() > 1024 || password.len() > 4096 {
                bail!("Enter a username and password (up to 4,096 characters)");
            }
            let mut saved = vault::load(&self.vault_path)?;
            if let Some(existing) = saved.passwords.iter_mut().find(|item| item.host == host && item.username == username) {
                existing.password = password.as_str().to_string();
            } else {
                let id = saved.passwords.iter().map(|item| item.id).max().unwrap_or(0) + 1;
                saved.passwords.push(vault::SavedPassword { id, host, username, password: password.as_str().to_string() });
            }
            vault::save(&self.vault_path, &saved)
        })();
        match result {
            Ok(()) => {
                self.vault_notice("Password saved in the encrypted local vault");
                self.send_vault_state();
            }
            Err(error) => self.vault_notice(format!("Could not save password: {error:#}")),
        }
    }

    fn delete_password(&mut self, id: u64) {
        let result = (|| -> Result<bool> {
            let mut saved = vault::load(&self.vault_path)?;
            let before = saved.passwords.len();
            saved.passwords.retain(|item| item.id != id);
            if before != saved.passwords.len() { vault::save(&self.vault_path, &saved)?; }
            Ok(before != saved.passwords.len())
        })();
        match result {
            Ok(true) => { self.send_vault_state(); self.vault_notice("Password removed"); }
            Ok(false) => self.vault_notice("That saved password no longer exists"),
            Err(error) => self.vault_notice(format!("Could not remove password: {error:#}")),
        }
    }

    fn fill_password(&self, id: u64) {
        let result = (|| -> Result<(String, String, String)> {
            let saved = vault::load(&self.vault_path)?;
            let item = saved.passwords.iter().find(|item| item.id == id).context("That saved password no longer exists")?;
            let tab = self.active_tab().context("Open the site where you want to use this password")?;
            let url = url::Url::parse(&tab.url).context("The active tab is not a web page")?;
            if url.scheme() != "https" || url.host_str() != Some(item.host.as_str()) {
                bail!("Open an HTTPS page on {} before filling this login", item.host);
            }
            Ok((item.username.clone(), item.password.clone(), item.host.clone()))
        })();
        match result {
            Ok((mut username, mut password, _host)) => {
                let username_json = serde_json::to_string(&username).unwrap_or_else(|_| "\"\"".into());
                let mut password_json = serde_json::to_string(&password).unwrap_or_else(|_| "\"\"".into());
                username.zeroize();
                password.zeroize();
                let mut script = format!(
                    "(()=>{{const u={username_json},p={password_json};const f=document.querySelector('input[type=password]:not([disabled]):not([readonly])');if(!f)return;const root=f.form||document;const fields=Array.from(root.querySelectorAll('input'));const before=fields.slice(0,fields.indexOf(f)).filter(e=>!e.disabled&&!e.readOnly);const n=before.slice().reverse().find(e=>e.autocomplete==='username'||e.type==='email'||/user|email|login/i.test(e.name+' '+e.id))||before.slice().reverse().find(e=>e.type==='text');const set=(e,v)=>{{if(!e)return;const d=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value');if(d&&d.set)d.set.call(e,v);else e.value=v;e.dispatchEvent(new Event('input',{{bubbles:true}}));e.dispatchEvent(new Event('change',{{bubbles:true}}));}};set(n,u);set(f,p);}})();"
                );
                password_json.zeroize();
                self.eval_active(&script);
                script.zeroize();
                self.vault_notice("Filled the matching HTTPS login");
            }
            Err(error) => self.vault_notice(format!("Could not fill password: {error:#}")),
        }
    }

    fn save_card(&mut self, name: String, number: String, exp_month: u8, exp_year: u16) {
        let number = Zeroizing::new(number);
        let result = (|| -> Result<()> {
            let name = name.trim().to_string();
            if name.is_empty() || name.len() > 100 { bail!("Enter the name printed on the card"); }
            if !(1..=12).contains(&exp_month) { bail!("Choose a valid expiration month"); }
            let (year, month) = current_year_month();
            if i64::from(exp_year) < year || (i64::from(exp_year) == year && i64::from(exp_month) < month) {
                bail!("That card has expired");
            }
            if number.chars().any(|ch| !(ch.is_ascii_digit() || ch == ' ' || ch == '-')) {
                bail!("Card number can contain only digits, spaces, and dashes");
            }
            let digits = Zeroizing::new(number.chars().filter(char::is_ascii_digit).collect::<String>());
            if !(12..=19).contains(&digits.len()) || !passes_luhn(&digits) { bail!("Enter a valid card number"); }

            let mut saved = vault::load(&self.vault_path)?;
            if let Some(existing) = saved.cards.iter_mut().find(|item| item.number == digits.as_str() && item.name == name) {
                existing.exp_month = exp_month;
                existing.exp_year = exp_year;
            } else {
                let id = saved.cards.iter().map(|item| item.id).max().unwrap_or(0) + 1;
                saved.cards.push(vault::SavedCard { id, name, number: digits.to_string(), exp_month, exp_year });
            }
            vault::save(&self.vault_path, &saved)
        })();
        match result {
            Ok(()) => { self.vault_notice("Card saved in the encrypted local vault"); self.send_vault_state(); }
            Err(error) => self.vault_notice(format!("Could not save card: {error:#}")),
        }
    }

    fn delete_card(&mut self, id: u64) {
        let result = (|| -> Result<bool> {
            let mut saved = vault::load(&self.vault_path)?;
            let before = saved.cards.len();
            saved.cards.retain(|item| item.id != id);
            if before != saved.cards.len() { vault::save(&self.vault_path, &saved)?; }
            Ok(before != saved.cards.len())
        })();
        match result {
            Ok(true) => { self.send_vault_state(); self.vault_notice("Card removed"); }
            Ok(false) => self.vault_notice("That saved card no longer exists"),
            Err(error) => self.vault_notice(format!("Could not remove card: {error:#}")),
        }
    }

    fn fill_card(&self, id: u64) {
        let result = (|| -> Result<(String, String, u8, u16)> {
            let saved = vault::load(&self.vault_path)?;
            let item = saved.cards.iter().find(|item| item.id == id).context("That saved card no longer exists")?;
            let tab = self.active_tab().context("Open a secure checkout page before filling a card")?;
            let url = url::Url::parse(&tab.url).context("The active tab is not a web page")?;
            if url.scheme() != "https" { bail!("Saved cards can only be filled on HTTPS pages"); }
            Ok((item.name.clone(), item.number.clone(), item.exp_month, item.exp_year))
        })();
        match result {
            Ok((mut name, mut number, month, year)) => {
                let mut name_json = serde_json::to_string(&name).unwrap_or_else(|_| "\"\"".into());
                let mut number_json = serde_json::to_string(&number).unwrap_or_else(|_| "\"\"".into());
                let month_json = serde_json::to_string(&format!("{month:02}")).unwrap_or_else(|_| "\"\"".into());
                let year_json = serde_json::to_string(&year.to_string()).unwrap_or_else(|_| "\"\"".into());
                let short_year = serde_json::to_string(&format!("{:02}", year % 100)).unwrap_or_else(|_| "\"\"".into());
                name.zeroize();
                number.zeroize();
                let mut script = format!(
                    "(()=>{{const n={name_json},c={number_json},m={month_json},y={year_json},ys={short_year};const set=(s,v)=>{{const e=document.querySelector(s);if(!e||e.disabled||e.readOnly)return;const d=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value');if(d&&d.set)d.set.call(e,v);else e.value=v;e.dispatchEvent(new Event('input',{{bubbles:true}}));e.dispatchEvent(new Event('change',{{bubbles:true}}));}};set('input[autocomplete=cc-name]',n);set('input[autocomplete=cc-number]',c);set('input[autocomplete=cc-exp-month]',m);set('input[autocomplete=cc-exp-year]',y);set('input[autocomplete=cc-exp]',m+'/'+ys);}})();"
                );
                name_json.zeroize();
                number_json.zeroize();
                self.eval_active(&script);
                script.zeroize();
                self.vault_notice("Filled saved card details. The security code is never stored.");
            }
            Err(error) => self.vault_notice(format!("Could not fill card: {error:#}")),
        }
    }

    fn pick_extension(&mut self) {
        if let Some(path) = rfd::FileDialog::new().set_title("Choose an unpacked browser extension").pick_folder() {
            match self.install_extension(&path) {
                Ok(name) => {
                    self.push_state();
                    self.vault_notice(format!("Added {name}. Restart Zyla to activate the extension."));
                }
                Err(error) => self.vault_notice(format!("Could not add extension: {error:#}")),
            }
        }
    }

    fn install_extension(&self, source: &Path) -> Result<String> {
        let manifest_path = source.join("manifest.json");
        let manifest: Value = serde_json::from_slice(&std::fs::read(&manifest_path)
            .with_context(|| format!("No manifest.json found in {}", source.display()))?)
            .context("Extension manifest.json is not valid JSON")?;
        match manifest.get("manifest_version").and_then(Value::as_u64) {
            Some(2 | 3) => {}
            _ => bail!("Only unpacked Manifest V2 or V3 extensions can be added"),
        }
        let name = manifest.get("name").and_then(Value::as_str).unwrap_or("Local extension").to_string();
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
        let mut attempt = 0u32;
        let destination = loop {
            let path = self.extensions_dir.join(format!("extension-{stamp}-{attempt}"));
            if !path.exists() { break path; }
            attempt += 1;
        };
        let staging_root = self.extensions_dir.parent().context("The extension path has no parent directory")?.join("extension-staging");
        std::fs::create_dir_all(&staging_root).context("Could not create extension staging directory")?;
        let staging = staging_root.join(destination.file_name().context("Invalid extension destination")?);
        copy_extension_tree(source, &staging)?;
        std::fs::rename(&staging, &destination).context("Could not install the local extension")?;
        Ok(name)
    }

    fn vault_notice(&self, text: impl Into<String>) {
        self.send_chrome(json!({ "type": "notice", "text": text.into() }));
    }

    fn move_tab(&mut self, id: u32, before: Option<u32>, section: &str) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else { return };
        let mut tab = self.tabs.remove(index);
        match section {
            "favorite" => {
                tab.pinned = true;
                tab.favorite = true;
            }
            "pinned" => {
                tab.pinned = true;
                tab.favorite = false;
            }
            _ => {
                tab.pinned = false;
                tab.favorite = false;
            }
        }
        let at = before
            .and_then(|target| self.tabs.iter().position(|candidate| candidate.id == target))
            .unwrap_or(self.tabs.len());
        self.tabs.insert(at, tab);
        self.dirty = true;
        self.push_state();
    }

    fn set_pinned(&mut self, id: u32, pinned: bool, favorite: bool) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            tab.pinned = pinned;
            tab.favorite = pinned && favorite;
        }
        self.dirty = true;
        self.push_state();
    }

    fn close(&mut self, id: u32) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else { return };
        if self.in_split(id) {
            self.split = None;
        }
        let tab = self.tabs.remove(index);
        let space = tab.space;
        self.closed.push((tab.url.clone(), tab.zoom));
        if self.closed.len() > CLOSED_TAB_LIMIT {
            self.closed.remove(0);
        }
        // Dropping `tab` destroys its WebView and frees the page.
        drop(tab);
        for entry in &mut self.spaces {
            if entry.last_active == Some(id) {
                entry.last_active = None;
            }
        }
        if self.active == Some(id) {
            self.active = None;
            // The tab that took the closed one's place, within the same Space, becomes active.
            let next = self
                .tabs
                .iter()
                .enumerate()
                .filter(|(_, tab)| tab.space == space)
                .min_by_key(|(i, _)| i.abs_diff(index))
                .map(|(_, tab)| tab.id);
            match next {
                Some(next) => self.select(next),
                None => {
                    self.active_space = space;
                    self.open_and_select(NEW_TAB_URL);
                }
            }
        }
        // Closing the other pane leaves the active tab with the whole area again.
        self.place_tabs();
        self.dirty = true;
        self.push_state();
    }

    fn step_tab(&mut self, direction: isize) {
        let Some(active) = self.active else { return };
        let ids: Vec<u32> = self.tabs.iter().filter(|tab| tab.space == self.active_space).map(|tab| tab.id).collect();
        let Some(index) = ids.iter().position(|id| *id == active) else { return };
        let count = ids.len() as isize;
        let next = (index as isize + direction).rem_euclid(count) as usize;
        self.select(ids[next]);
    }

    fn zoom_active(&mut self, step: f64) {
        let Some(tab) = self.active_tab_mut() else { return };
        tab.zoom = if step == 0.0 { 1.0 } else { (tab.zoom + step).clamp(0.25, 5.0) };
        let _ = tab.webview.zoom(tab.zoom);
        self.dirty = true;
        self.push_state();
    }

    fn eval_active(&self, script: &str) {
        if let Some(tab) = self.active_tab() {
            let _ = tab.webview.evaluate_script(script);
        }
    }

    fn focus_active(&self) {
        if let Some(tab) = self.active_tab() {
            let _ = tab.webview.focus();
        }
    }

    fn active_tab(&self) -> Option<&Tab> {
        let active = self.active?;
        self.tabs.iter().find(|tab| tab.id == active)
    }

    fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        let active = self.active?;
        self.tabs.iter_mut().find(|tab| tab.id == active)
    }

    fn update_title(&self) {
        let title = match self.active_tab() {
            Some(tab) if !tab.title.is_empty() => format!("{} – Zyla Browser", tab.title),
            _ => "Zyla Browser".to_string(),
        };
        self.window.set_title(&title);
    }

    fn logical_size(&self) -> (f64, f64) {
        let size = self.window.inner_size().to_logical::<f64>(self.window.scale_factor());
        (size.width, size.height)
    }

    fn sidebar_offset(&self) -> f64 {
        // With tabs on top the sidebar is not shown at all.
        if self.sidebar_visible && !self.tabs_on_top { self.sidebar_width } else { 0.0 }
    }

    /// Height of everything above the tab pages: the toolbar, bookmark bar and optional tab strip.
    fn chrome_height(&self) -> f64 {
        let tab_strip = if self.tabs_on_top { TAB_STRIP_HEIGHT } else { 0.0 };
        tab_strip + TOP_BAR_HEIGHT + BOOKMARK_BAR_HEIGHT
    }

    /// The area a tab page fills: right of the sidebar, below the toolbar (and the tab strip).
    fn content_area(&self) -> (f64, f64, f64, f64) {
        let (width, height) = self.logical_size();
        let offset = self.sidebar_offset();
        let x = if self.sidebar_side == "right" { 0.0 } else { offset };
        let top = self.chrome_height();
        (x, top, (width - offset).max(0.0), (height - top).max(0.0))
    }

    fn content_rect(&self) -> Rect {
        let (x, y, width, height) = self.content_area();
        rect(x, y, width, height)
    }

    /// Each visible tab page with its area: the whole content area, or the two halves of a split.
    fn pane_areas(&self) -> Vec<(u32, (f64, f64, f64, f64))> {
        let (x, y, width, height) = self.content_area();
        match (self.split, self.active) {
            (Some((left, right)), _) => {
                let half = ((width - SPLIT_GAP) / 2.0).max(0.0);
                vec![(left, (x, y, half, height)), (right, (x + half + SPLIT_GAP, y, half, height))]
            }
            (None, Some(active)) => vec![(active, (x, y, width, height))],
            (None, None) => Vec::new(),
        }
    }

    /// Puts each tab page in its place: the visible pane areas for the shown tabs, every other tab hidden.
    fn place_tabs(&self) {
        if !self.restore_queue.is_empty() || self.startup_covering {
            for tab in &self.tabs {
                let _ = tab.webview.set_visible(false);
            }
            return;
        }
        let (x, y, width, height) = self.content_area();
        let panes = self.pane_areas();
        for tab in &self.tabs {
            match panes.iter().find(|(id, _)| *id == tab.id) {
                Some((_, (px, py, pw, ph))) => {
                    let _ = tab.webview.set_bounds(rect(*px, *py, *pw, *ph));
                    let _ = tab.webview.set_visible(true);
                }
                None => {
                    let _ = tab.webview.set_bounds(rect(x, y, width, height));
                    let _ = tab.webview.set_visible(false);
                }
            }
        }
    }

    fn layout(&self) {
        let (width, height) = self.logical_size();
        let _ = self.chrome.set_bounds(rect(0.0, 0.0, width, height));
        self.place_tabs();
    }

    fn send_chrome(&self, message: Value) {
        if !self.chrome_ready {
            return;
        }
        let script = format!("window.tsgpuiBrowser && window.tsgpuiBrowser.receive({message})");
        let _ = self.chrome.evaluate_script(&script);
    }

    /// Sends the full browser state to the chrome page. Small enough to send on every change.
    fn push_state(&self) {
        let tabs: Vec<Value> = self
            .tabs
            .iter()
            .map(|tab| {
                json!({
                    "id": tab.id,
                    "title": tab.title,
                    "url": tab.url,
                    "favicon": tab.favicon,
                    "loading": tab.loading,
                    "zoom": tab.zoom,
                    "loadMs": tab.load_ms,
                    "space": tab.space,
                    "pinned": tab.pinned,
                    "favorite": tab.favorite,
                })
            })
            .collect();
        let spaces: Vec<Value> = self
            .spaces
            .iter()
            .map(|space| json!({ "id": space.id, "name": space.name, "tint": space.tint }))
            .collect();
        self.send_chrome(json!({
            "type": "state",
            "active": self.active,
            "activeSpace": self.active_space,
            "split": self.split.map(|(left, right)| [left, right]),
            "spaces": spaces,
            "tabs": tabs,
            "sidebar": { "visible": self.sidebar_visible, "width": self.sidebar_width, "side": self.sidebar_side },
            "tabsOnTop": self.tabs_on_top,
            "extensionsCount": extension_count(&self.extensions_dir),
            "restoringTabs": self.restore_queue.len(),
        }));
    }
}

fn normalize_site_host(input: &str) -> Result<String> {
    let input = input.trim();
    if input.is_empty() || input.len() > 2048 { bail!("Enter a website host"); }
    let with_scheme = if input.contains("://") { input.to_string() } else { format!("https://{input}") };
    let parsed = url::Url::parse(&with_scheme).context("Enter a valid website host")?;
    if parsed.scheme() != "https" || !parsed.username().is_empty() || parsed.password().is_some() {
        bail!("Saved logins must use an HTTPS website host");
    }
    parsed.host_str().map(str::to_ascii_lowercase).context("Enter a valid website host")
}

fn current_year_month() -> (i64, i64) {
    let days = UNIX_EPOCH.elapsed().unwrap_or_default().as_secs() as i64 / 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 { year += 1; }
    (year, month)
}

fn passes_luhn(digits: &str) -> bool {
    let parity = digits.len() % 2;
    let sum: u32 = digits
        .chars()
        .enumerate()
        .map(|(index, ch)| {
            let mut digit = ch.to_digit(10).unwrap_or(0);
            if index % 2 == parity {
                digit *= 2;
                if digit > 9 { digit -= 9; }
            }
            digit
        })
        .sum();
    sum % 10 == 0
}

fn copy_extension_tree(source: &Path, destination: &Path) -> Result<()> {
    std::fs::create_dir_all(destination).context("Could not create extension install directory")?;
    for entry in std::fs::read_dir(source).with_context(|| format!("Could not read {}", source.display()))? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            copy_extension_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), target).context("Could not copy an extension file")?;
        }
        // Symlinks are skipped so an extension cannot make the installer copy files from outside its folder.
    }
    Ok(())
}

fn extension_count(directory: &Path) -> usize {
    std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .filter(|entry| entry.path().join("manifest.json").is_file())
        .count()
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        position: LogicalPosition::new(x, y).into(),
        size: WryLogicalSize::new(width, height).into(),
    }
}

fn asset(chrome: &RwLock<String>, path: &str) -> Response<Cow<'static, [u8]>> {
    match path {
        "/" | "/index.html" => match chrome.read() {
            Ok(html) => respond(200, "text/html; charset=utf-8", html.as_bytes().to_vec()),
            Err(_) => respond(500, "text/plain; charset=utf-8", b"Chrome page could not be read".to_vec()),
        },
        "/newtab" => respond(200, "text/html; charset=utf-8", NEW_TAB_HTML.as_bytes().to_vec()),
        "/history" => respond(200, "text/html; charset=utf-8", HISTORY_HTML.as_bytes().to_vec()),
        "/settings" => respond(200, "text/html; charset=utf-8", SETTINGS_HTML.as_bytes().to_vec()),
        "/font/public-sans.woff2" => respond(200, "font/woff2", FONT.to_vec()),
        "/assets/logo.png" | "/logo.png" => respond(200, "image/png", LOGO.to_vec()),
        _ => respond(404, "text/plain; charset=utf-8", b"Not found".to_vec()),
    }
}

fn respond(status: u16, mime: &str, body: Vec<u8>) -> Response<Cow<'static, [u8]>> {
    match Response::builder().status(status).header(CONTENT_TYPE, mime).body(body) {
        Ok(response) => response.map(Into::into),
        Err(_) => Response::new(Cow::Borrowed(&[][..])),
    }
}

/// Sign-in pages that must open as a real popup. Sign in with Apple returns its result to the page that
/// opened it through `window.opener`, so a popup turned into a tab cannot finish the sign-in.
const SIGN_IN_POPUP_HOSTS: &[&str] = &["appleid.apple.com"];

fn is_sign_in_popup(next: &str) -> bool {
    url::Url::parse(next)
        .ok()
        .filter(|url| url.scheme() == "https")
        .and_then(|url| url.host_str().map(|host| SIGN_IN_POPUP_HOSTS.contains(&host)))
        .unwrap_or(false)
}

fn downloads_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|home| PathBuf::from(home).join("Downloads"))
        .unwrap_or_else(|| PathBuf::from("Downloads"))
}

/// Picks a file name in the downloads folder. Existing files are never overwritten.
/// `suggested` is the name WebView2 chose from the response (its `Content-Disposition` or URL), which keeps
/// the real extension, so it wins over the URL. Dynamic URLs such as `/download?id=5` have no usable name.
fn download_target(dir: &Path, suggested: &Path, url: &str) -> PathBuf {
    let from_suggested = suggested.file_name().and_then(|name| name.to_str()).unwrap_or("");
    let name = match clean_file_name(from_suggested) {
        Some(name) => name,
        None => {
            let last = url::Url::parse(url)
                .ok()
                .and_then(|url| url.path_segments().and_then(|mut segments| segments.next_back().map(str::to_owned)))
                .unwrap_or_default();
            // URL segments are percent-encoded (`my%20app.exe`), so decode before cleaning.
            let decoded = url::form_urlencoded::parse(last.as_bytes()).next().map(|(key, _)| key.into_owned()).unwrap_or_default();
            clean_file_name(&decoded).unwrap_or_else(|| "download".into())
        }
    };
    let target = dir.join(&name);
    if !target.exists() {
        return target;
    }
    let millis = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    dir.join(format!("{millis}-{name}"))
}

/// Keeps letters, digits, `.`, `_`, `-` and turns anything else (including `\`, `:`, spaces) into `_`, so a
/// name can never escape the downloads folder. Returns `None` when nothing usable is left.
fn clean_file_name(raw: &str) -> Option<String> {
    let name: String = raw.trim().chars().take(200).map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' }).collect();
    (!name.is_empty() && name != "." && name != "..").then_some(name)
}

#[cfg(test)]
mod download_tests {
    use super::*;

    #[test]
    fn keeps_the_suggested_name_and_extension() {
        let dir = Path::new("C:/Downloads-test-missing");
        let suggested = Path::new("C:/somewhere/setup-2.1.exe");
        assert_eq!(download_target(dir, suggested, "https://example.com/get?id=5"), dir.join("setup-2.1.exe"));
    }

    #[test]
    fn falls_back_to_the_decoded_url_segment() {
        let dir = Path::new("C:/Downloads-test-missing");
        let empty = Path::new("");
        assert_eq!(download_target(dir, empty, "https://example.com/files/my%20app.zip?x=1"), dir.join("my_app.zip"));
        assert_eq!(download_target(dir, empty, "https://example.com/"), dir.join("download"));
    }

    #[test]
    fn never_escapes_the_folder() {
        assert_eq!(clean_file_name("..\\..\\evil.exe").as_deref(), Some(".._.._evil.exe"));
        assert_eq!(clean_file_name(".."), None);
    }
}
