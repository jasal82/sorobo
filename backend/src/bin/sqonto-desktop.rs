// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

//! `sqonto-desktop` – Sqonto as a desktop application: runs the server inside the process and
//! shows the interface in a native window (WebView2 on Windows, WebKit on macOS/Linux) – without
//! console window and without browser.
//!
//! On Windows the window has no native frame: the app's header is the title bar (drawn by the
//! frontend, see `lib/desktop.tsx`), which moves, resizes and controls the window through IPC.
//!
//! Development: `SQONTO_DESKTOP_URL=http://localhost:5173` shows the Vite dev server (hot reload)
//! instead of the embedded frontend; `SQONTO_DEVTOOLS=1` enables the developer tools.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;

use sqonto::Launch;
use serde::{Deserialize, Serialize};
use tao::dpi::{LogicalSize, PhysicalPosition};
#[cfg(target_os = "macos")]
use tao::event::StartCause;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget};
use tao::window::{Icon, ResizeDirection, Window, WindowBuilder, WindowId};
use wry::{NewWindowResponse, WebContext, WebView, WebViewBuilder};

const WEBVIEW2_DOWNLOAD: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";
/// Asks the page how many editors have unsaved changes (see `useLeaveWarning` in the frontend).
const UNSAVED_JS: &str = "(window.__sqontoUnsaved ? window.__sqontoUnsaved() : 0)";

#[derive(Debug)]
enum UserEvent {
    /// Another start of the program wants this window in front.
    Focus,
    /// `window.open` – app pages (PDF preview) get an own window, external pages the browser.
    OpenWindow(String),
    /// Answer of the page to `UNSAVED_JS` when the window is closed.
    CloseCheck(String),
    Downloaded(PathBuf),
    /// An update was installed: end, the new version starts itself.
    Quit,
    /// Cmd+Q: closes the main window like its close button does (unsaved changes are asked for).
    CloseMain,
    /// Title bar of the page: minimize, maximize, close, drag, resize-<edge>, state.
    WindowCommand(String),
}

/// The page draws the title bar itself (Windows; elsewhere the native frame stays).
const CUSTOM_TITLEBAR: bool = cfg!(windows);

#[derive(Serialize, Deserialize)]
struct WindowState {
    width: f64,
    height: f64,
    x: i32,
    y: i32,
    maximized: bool,
}

fn main() {
    let dir = sqonto::data_dir();
    init_logging(&dir);
    let runtime = tokio::runtime::Runtime::new().unwrap_or_else(|e| fatal(&format!("Sqonto konnte nicht gestartet werden:\n\n{e}")));
    let app_url = match runtime.block_on(sqonto::launch(sqonto::default_port(), &dir)) {
        Ok(Launch::Started(server)) => {
            let url = server.url.clone();
            runtime.spawn(async move {
                if let Err(e) = server.run().await {
                    tracing::error!("Server beendet: {e:#}");
                }
            });
            url
        }
        Ok(Launch::AlreadyRunning { port, url }) => {
            if sqonto::request_focus(port) {
                return;
            }
            // A headless server with the same data is running – show it in a window.
            url
        }
        Err(e) => fatal(&format!("Sqonto konnte nicht gestartet werden:\n\n{e:#}")),
    };
    let start_url = std::env::var("SQONTO_DESKTOP_URL").unwrap_or_else(|_| app_url.clone());
    run(&dir, app_url, start_url);
}

fn run(dir: &Path, app_url: String, start_url: String) -> ! {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    #[cfg(target_os = "macos")]
    set_dock_icon();
    let proxy = event_loop.create_proxy();
    #[cfg(target_os = "macos")]
    let _menu = macos_menu(proxy.clone());
    let focus_proxy = Mutex::new(proxy.clone());
    sqonto::set_focus_handler(move || {
        if let Ok(p) = focus_proxy.lock() {
            let _ = p.send_event(UserEvent::Focus);
        }
    });
    let quit_proxy = Mutex::new(proxy.clone());
    sqonto::set_quit_handler(move || {
        if let Ok(p) = quit_proxy.lock() {
            let _ = p.send_event(UserEvent::Quit);
        }
    });

    let state_file = dir.join("window.json");
    let window = main_window(&event_loop, &state_file).unwrap_or_else(|e| fatal(&format!("Das Fenster konnte nicht erstellt werden:\n\n{e}")));
    // Own profile folder in the data directory (WebView2 would otherwise create one next to the exe).
    let mut web_context = WebContext::new(Some(dir.join("webview")));
    let downloads: Rc<RefCell<HashSet<PathBuf>>> = Rc::default();
    let last_dir: Rc<RefCell<Option<PathBuf>>> = Rc::default();
    let allowed = vec![origin(&app_url), origin(&start_url), "http://127.0.0.1:".to_string() + port_of(&app_url)];
    let webview = {
        let proxy_nw = proxy.clone();
        let proxy_dl = proxy.clone();
        let allowed_nav = allowed.clone();
        let downloads_ipc = downloads.clone();
        let proxy_ipc = proxy.clone();
        let init = if CUSTOM_TITLEBAR { "window.__SQONTO_DESKTOP__ = true; window.__SQONTO_TITLEBAR__ = true;" } else { "window.__SQONTO_DESKTOP__ = true;" };
        let builder = WebViewBuilder::new_with_web_context(&mut web_context)
            .with_url(&start_url)
            .with_devtools(devtools())
            .with_initialization_script(init)
            .with_navigation_handler(move |url| keep_inside(&url, &allowed_nav))
            .with_new_window_req_handler(move |url, _| {
                let _ = proxy_nw.send_event(UserEvent::OpenWindow(url));
                NewWindowResponse::Deny
            })
            .with_download_started_handler(save_dialog(last_dir.clone()))
            .with_download_completed_handler(move |_, path, ok| {
                if ok && let Some(path) = path {
                    let _ = proxy_dl.send_event(UserEvent::Downloaded(path));
                }
            })
            .with_ipc_handler(move |req| match req.body().strip_prefix("window:") {
                Some(command) => {
                    let _ = proxy_ipc.send_event(UserEvent::WindowCommand(command.to_string()));
                }
                None => handle_ipc(req.body(), &downloads_ipc.borrow()),
            });
        builder.build(&window).unwrap_or_else(|e| webview_missing(&e))
    };

    let mut popups: HashMap<WindowId, (Window, WebView)> = HashMap::new();
    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            // Once the app has finished launching: macOS may reset the icon set before that.
            #[cfg(target_os = "macos")]
            Event::NewEvents(StartCause::Init) => set_dock_icon(),
            Event::WindowEvent { window_id, event: WindowEvent::CloseRequested, .. } => {
                if window_id != window.id() {
                    popups.remove(&window_id);
                    return;
                }
                request_close(&window, &webview, &proxy, &state_file, control_flow);
            }
            Event::UserEvent(UserEvent::CloseMain) => request_close(&window, &webview, &proxy, &state_file, control_flow),
            Event::WindowEvent { window_id, event: WindowEvent::Resized(_) | WindowEvent::Focused(_), .. } if CUSTOM_TITLEBAR && window_id == window.id() => {
                send_window_state(&window, &webview);
            }
            Event::UserEvent(UserEvent::WindowCommand(command)) => match command.as_str() {
                "minimize" => window.set_minimized(true),
                "maximize" => window.set_maximized(!window.is_maximized()),
                "drag" => {
                    let _ = window.drag_window();
                }
                "close" => request_close(&window, &webview, &proxy, &state_file, control_flow),
                "state" => send_window_state(&window, &webview),
                other => {
                    if let Some(edge) = other.strip_prefix("resize-").and_then(resize_direction)
                        && !window.is_maximized()
                    {
                        let _ = window.drag_resize_window(edge);
                    }
                }
            },
            Event::UserEvent(UserEvent::CloseCheck(answer)) => {
                let unsaved = answer.trim_matches('"').parse::<f64>().unwrap_or(0.0) > 0.0;
                if !unsaved
                    || ask(
                        &window,
                        "Ungespeicherte Änderungen",
                        "Es gibt Änderungen, die noch nicht gespeichert wurden. Sqonto trotzdem beenden?",
                    )
                {
                    save_state(&window, &state_file);
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::UserEvent(UserEvent::Quit) => {
                save_state(&window, &state_file);
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(UserEvent::Focus) => {
                window.set_visible(true);
                window.set_minimized(false);
                window.set_focus();
            }
            Event::UserEvent(UserEvent::OpenWindow(url)) => {
                if allowed.iter().any(|o| url.starts_with(o.as_str())) {
                    match popup(target, &mut web_context, &url, &allowed, last_dir.clone()) {
                        Ok(p) => {
                            popups.insert(p.0.id(), p);
                        }
                        Err(e) => tracing::error!("Fenster für {url} nicht geöffnet: {e}"),
                    }
                } else {
                    let _ = open::that(&url);
                }
            }
            Event::UserEvent(UserEvent::Downloaded(path)) => {
                downloads.borrow_mut().insert(path.clone());
                let arg = serde_json::to_string(&path.display().to_string()).unwrap_or_default();
                let _ = webview.evaluate_script(&format!("window.__sqontoDownloaded && window.__sqontoDownloaded({arg})"));
            }
            _ => {}
        }
    })
}

/// Asks the page for unsaved changes (answered by `CloseCheck`); without a page the app ends at once.
fn request_close(window: &Window, webview: &WebView, proxy: &EventLoopProxy<UserEvent>, state_file: &Path, control_flow: &mut ControlFlow) {
    let p = proxy.clone();
    if webview
        .evaluate_script_with_callback(UNSAVED_JS, move |r| {
            let _ = p.send_event(UserEvent::CloseCheck(r));
        })
        .is_err()
    {
        save_state(window, state_file);
        *control_flow = ControlFlow::Exit;
    }
}

/// The WebView only handles Cmd+C/V/X/A/Z when the menu bar has the matching items.
#[cfg(target_os = "macos")]
fn macos_menu(proxy: EventLoopProxy<UserEvent>) -> muda::Menu {
    use muda::{accelerator::{Accelerator, Code, Modifiers}, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
    let menu = Menu::new();
    // Not the predefined item: it would end the process without the unsaved-changes check.
    let quit = MenuItem::new("Sqonto beenden", true, Some(Accelerator::new(Some(Modifiers::META), Code::KeyQ)));
    let quit_id = quit.id().clone();
    let proxy = Mutex::new(proxy);
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == quit_id && let Ok(p) = proxy.lock() {
            let _ = p.send_event(UserEvent::CloseMain);
        }
    }));
    let app = Submenu::new("Sqonto", true);
    let edit = Submenu::new("Bearbeiten", true);
    let window = Submenu::new("Fenster", true);
    let _ = menu.append_items(&[&app, &edit, &window]);
    let _ = app.append_items(&[
        &PredefinedMenuItem::about(None, None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::services(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::hide(None),
        &PredefinedMenuItem::hide_others(None),
        &PredefinedMenuItem::show_all(None),
        &PredefinedMenuItem::separator(),
        &quit,
    ]);
    let _ = edit.append_items(&[
        &PredefinedMenuItem::undo(None),
        &PredefinedMenuItem::redo(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::cut(None),
        &PredefinedMenuItem::copy(None),
        &PredefinedMenuItem::paste(None),
        &PredefinedMenuItem::select_all(None),
    ]);
    let _ = window.append_items(&[&PredefinedMenuItem::minimize(None), &PredefinedMenuItem::maximize(None), &PredefinedMenuItem::close_window(None)]);
    window.set_as_windows_menu_for_nsapp();
    menu.init_for_nsapp();
    menu
}

/// The executable has no bundle, so macOS would show a generic icon in the Dock.
#[cfg(target_os = "macos")]
fn set_dock_icon() {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;
    let Some(mtm) = MainThreadMarker::new() else { return };
    let data = NSData::with_bytes(include_bytes!("../../assets/icons/sqonto-macos-512.png"));
    if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
        // SAFETY: called on the main thread with a valid image.
        unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
    }
}

fn main_window(event_loop: &EventLoop<UserEvent>, state_file: &Path) -> Result<Window, tao::error::OsError> {
    let saved: Option<WindowState> = std::fs::read(state_file).ok().and_then(|b| serde_json::from_slice(&b).ok());
    let mut builder = WindowBuilder::new().with_title("Sqonto").with_min_inner_size(LogicalSize::new(900.0, 600.0)).with_window_icon(app_icon());
    #[cfg(windows)]
    {
        // Without frame, but with the shadow, the rounded corners (Windows 11) and the invisible
        // resize borders of a normal window; the top edge is resized through the page.
        use tao::platform::windows::WindowBuilderExtWindows;
        builder = builder.with_decorations(false).with_undecorated_shadow(true);
    }
    match saved {
        Some(s) => {
            builder = builder.with_inner_size(LogicalSize::new(s.width.max(900.0), s.height.max(600.0))).with_maximized(s.maximized);
            // Only restore the position if it is still on one of the screens.
            let visible = event_loop.available_monitors().any(|m| {
                let (p, size) = (m.position(), m.size());
                s.x + 80 >= p.x && s.y >= p.y && s.x + 80 <= p.x + size.width as i32 && s.y + 40 <= p.y + size.height as i32
            });
            if visible {
                builder = builder.with_position(PhysicalPosition::new(s.x, s.y));
            }
        }
        None => builder = builder.with_inner_size(LogicalSize::new(1440.0, 920.0)),
    }
    builder.build(event_loop)
}

/// Window for app pages opened via `window.open`, e.g. a PDF for printing.
fn popup(
    target: &EventLoopWindowTarget<UserEvent>,
    web_context: &mut WebContext,
    url: &str,
    allowed: &[String],
    last_dir: Rc<RefCell<Option<PathBuf>>>,
) -> Result<(Window, WebView), Box<dyn std::error::Error>> {
    let window = WindowBuilder::new()
        .with_title(if url.contains("/pdf") { "Sqonto – PDF" } else { "Sqonto" })
        .with_inner_size(LogicalSize::new(980.0, 1100.0))
        .with_window_icon(app_icon())
        .build(target)?;
    let allowed = allowed.to_vec();
    let webview = WebViewBuilder::new_with_web_context(web_context)
        .with_url(url)
        .with_devtools(devtools())
        .with_navigation_handler(move |u| keep_inside(&u, &allowed))
        .with_new_window_req_handler(|u, _| {
            let _ = open::that(&u);
            NewWindowResponse::Deny
        })
        .with_download_started_handler(save_dialog(last_dir))
        .build(&window)?;
    Ok((window, webview))
}

/// Tells the page whether the window is maximized (restore button, no resize edge) and active.
fn send_window_state(window: &Window, webview: &WebView) {
    let script = format!(
        "window.__sqontoWindowState && window.__sqontoWindowState({{ maximized: {}, focused: {} }})",
        window.is_maximized(),
        window.is_focused()
    );
    let _ = webview.evaluate_script(&script);
}

fn resize_direction(edge: &str) -> Option<ResizeDirection> {
    Some(match edge {
        "n" => ResizeDirection::North,
        "ne" => ResizeDirection::NorthEast,
        "nw" => ResizeDirection::NorthWest,
        "e" => ResizeDirection::East,
        "w" => ResizeDirection::West,
        "s" => ResizeDirection::South,
        "se" => ResizeDirection::SouthEast,
        "sw" => ResizeDirection::SouthWest,
        _ => return None,
    })
}

/// Navigation inside the app stays in the window; everything else (mailto:, websites) is handed
/// to the default application.
fn keep_inside(url: &str, allowed: &[String]) -> bool {
    if ["about:", "blob:", "data:"].iter().any(|p| url.starts_with(p)) || allowed.iter().any(|o| url.starts_with(o.as_str())) {
        return true;
    }
    if let Err(e) = open::that(url) {
        tracing::warn!("{url} konnte nicht geöffnet werden: {e}");
    }
    false
}

/// Downloads ask for the target with the native "Speichern unter" dialog.
fn save_dialog(last_dir: Rc<RefCell<Option<PathBuf>>>) -> impl FnMut(String, &mut PathBuf) -> bool + 'static {
    move |_url, path| {
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Download".into());
        let mut dialog = rfd::FileDialog::new().set_title("Speichern unter").set_file_name(&name);
        if let Some(dir) = last_dir.borrow().clone().or_else(|| path.parent().map(Path::to_path_buf)) {
            dialog = dialog.set_directory(dir);
        }
        if let Some(ext) = Path::new(&name).extension().and_then(|e| e.to_str()) {
            dialog = dialog.add_filter(ext.to_uppercase(), &[ext]);
        }
        match dialog.save_file() {
            Some(target) => {
                *last_dir.borrow_mut() = target.parent().map(Path::to_path_buf);
                *path = target;
                true
            }
            None => false,
        }
    }
}

/// Messages from the page (`window.ipc.postMessage`). Only files saved in this session may be
/// opened or shown.
fn handle_ipc(message: &str, downloads: &HashSet<PathBuf>) {
    let Some((command, path)) = message.split_once(':') else { return };
    let path = PathBuf::from(path);
    if !downloads.contains(&path) {
        return;
    }
    match command {
        "open" => {
            let _ = open::that(&path);
        }
        "reveal" => reveal(&path),
        _ => {}
    }
}

fn reveal(path: &Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
    }
    #[cfg(not(windows))]
    if let Some(dir) = path.parent() {
        let _ = open::that(dir);
    }
}

fn save_state(window: &Window, file: &Path) {
    let maximized = window.is_maximized();
    let scale = window.scale_factor();
    let size = window.inner_size().to_logical::<f64>(scale);
    let pos = window.outer_position().unwrap_or_default();
    let mut state = WindowState { width: size.width, height: size.height, x: pos.x, y: pos.y, maximized };
    // Keep the normal size and position of a maximised window from the last start.
    if maximized && let Some(prev) = std::fs::read(file).ok().and_then(|b| serde_json::from_slice::<WindowState>(&b).ok()) {
        state = WindowState { maximized: true, ..prev };
    }
    if let Ok(json) = serde_json::to_vec_pretty(&state) {
        let _ = std::fs::write(file, json);
    }
}

fn origin(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let host = rest.split('/').next().unwrap_or(rest);
    let scheme = url.split_once("://").map(|(s, _)| s).unwrap_or("http");
    format!("{scheme}://{host}")
}

fn port_of(url: &str) -> &str {
    url.rsplit(':').next().unwrap_or("").trim_end_matches('/')
}

fn devtools() -> bool {
    cfg!(debug_assertions) || std::env::var("SQONTO_DEVTOOLS").is_ok()
}

fn app_icon() -> Option<Icon> {
    #[cfg(windows)]
    {
        use tao::platform::windows::IconExtWindows;
        Icon::from_resource(1, None).ok()
    }
    #[cfg(not(windows))]
    None
}

/// Log file in the data directory – there is no console.
fn init_logging(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    match std::fs::File::create(dir.join("sqonto.log")) {
        Ok(file) => tracing_subscriber::fmt().with_target(false).with_ansi(false).with_writer(Mutex::new(file)).init(),
        Err(_) => tracing_subscriber::fmt().with_target(false).init(),
    }
}

fn ask(window: &Window, title: &str, text: &str) -> bool {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title(title)
        .set_description(text)
        .set_buttons(rfd::MessageButtons::YesNo)
        .set_parent(window)
        .show()
        == rfd::MessageDialogResult::Yes
}

fn fatal(message: &str) -> ! {
    tracing::error!("{message}");
    rfd::MessageDialog::new().set_level(rfd::MessageLevel::Error).set_title("Sqonto").set_description(message).show();
    std::process::exit(1)
}

fn webview_missing(e: &wry::Error) -> ! {
    tracing::error!("WebView: {e}");
    let install = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("Sqonto")
        .set_description(format!(
            "Die Anzeige konnte nicht gestartet werden ({e}).\n\nSqonto benötigt die „Microsoft Edge WebView2 Runtime“, die unter Windows 10/11 normalerweise vorinstalliert ist. Jetzt herunterladen?"
        ))
        .set_buttons(rfd::MessageButtons::YesNo)
        .show()
        == rfd::MessageDialogResult::Yes;
    if install {
        let _ = open::that(WEBVIEW2_DOWNLOAD);
    }
    std::process::exit(1)
}

