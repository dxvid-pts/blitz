#![cfg_attr(docsrs, feature(doc_cfg))]

//! Event loop, windowing and system integration.
//!
//! ## Feature flags
//!  - `default`: Enables the features listed below.
//!  - `accessibility`: Enables [`accesskit`] accessibility support.
//!  - `hot-reload`: Enables hot-reloading of Dioxus RSX.
//!  - `tracing`: Enables tracing support.

mod application;
mod convert_events;
mod event;
mod net;
mod window;

#[cfg(feature = "accessibility")]
mod accessibility;

pub use crate::application::BlitzApplication;
pub use crate::event::{BlitzShellEvent, BlitzShellProxy};
pub use crate::window::{View, WindowConfig};

#[cfg(feature = "data-uri")]
pub use crate::net::DataUriNetProvider;

#[cfg(all(
    feature = "file_dialog",
    any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )
))]
use blitz_traits::shell::FileDialogFilter;
use blitz_traits::shell::NativeSelectMenuRequest;
use blitz_traits::shell::ShellProvider;
use std::sync::Arc;
use winit::cursor::{Cursor, CursorIcon};
use winit::dpi::{LogicalPosition, LogicalSize};
pub use winit::event_loop::{ControlFlow, EventLoop, EventLoopProxy};
pub use winit::window::Window;
use winit::window::{ImeCapabilities, ImeEnableRequest, ImeRequest, ImeRequestData};

use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};

#[derive(Default)]
pub struct Config {
    pub stylesheets: Vec<String>,
    pub base_url: Option<String>,
}

/// Build an event loop for the application
pub fn create_default_event_loop() -> EventLoop {
    let mut ev_builder = EventLoop::builder();
    #[cfg(target_os = "android")]
    {
        use winit::platform::android::EventLoopBuilderExtAndroid;
        ev_builder.with_android_app(current_android_app());
    }

    let event_loop = ev_builder.build().unwrap();
    event_loop.set_control_flow(ControlFlow::Wait);

    event_loop
}

#[cfg(target_os = "android")]
static ANDROID_APP: std::sync::OnceLock<android_activity::AndroidApp> = std::sync::OnceLock::new();

#[cfg(target_os = "android")]
#[cfg_attr(docsrs, doc(cfg(target_os = "android")))]
/// Set the current [`AndroidApp`](android_activity::AndroidApp).
pub fn set_android_app(app: android_activity::AndroidApp) {
    ANDROID_APP.set(app).unwrap()
}

#[cfg(target_os = "android")]
#[cfg_attr(docsrs, doc(cfg(target_os = "android")))]
/// Get the current [`AndroidApp`](android_activity::AndroidApp).
/// This will panic if the android activity has not been setup with [`set_android_app`].
pub fn current_android_app() -> android_activity::AndroidApp {
    ANDROID_APP.get().unwrap().clone()
}

pub struct BlitzShellProvider {
    window: Arc<dyn Window>,
    proxy: BlitzShellProxy,
}
impl BlitzShellProvider {
    pub fn new(window: Arc<dyn Window>, proxy: BlitzShellProxy) -> Self {
        Self { window, proxy }
    }
}

impl ShellProvider for BlitzShellProvider {
    fn request_redraw(&self) {
        self.window.request_redraw();
    }
    fn set_cursor(&self, icon: CursorIcon) {
        self.window.set_cursor(Cursor::Icon(icon));
    }
    fn set_window_title(&self, title: String) {
        self.window.set_title(&title);
    }
    fn set_ime_enabled(&self, is_enabled: bool) {
        if is_enabled {
            let _ = self.window.request_ime_update(ImeRequest::Enable(
                ImeEnableRequest::new(ImeCapabilities::new(), ImeRequestData::default()).unwrap(),
            ));
        } else {
            let _ = self.window.request_ime_update(ImeRequest::Disable);
        }
    }
    fn set_ime_cursor_area(&self, x: f32, y: f32, width: f32, height: f32) {
        let _ = self.window.request_ime_update(ImeRequest::Update(
            ImeRequestData::default().with_cursor_area(
                LogicalPosition::new(x, y).into(),
                LogicalSize::new(width, height).into(),
            ),
        ));
    }

    fn request_native_select_menu(&self, req: NativeSelectMenuRequest) -> bool {
        #[cfg(target_os = "macos")]
        {
            use dispatch2::DispatchQueue;

            if req.items.is_empty() {
                return false;
            }

            let ns_view = match self.window.window_handle().ok().map(|h| h.as_raw()) {
                Some(RawWindowHandle::AppKit(handle)) => handle.ns_view.as_ptr() as usize,
                _ => return false,
            };

            let proxy = self.proxy.clone();
            let window_id = self.window.id();
            let view_height = self.window.surface_size().height as f32;

            DispatchQueue::main().exec_async(move || {
                let ns_view = ns_view as *const std::ffi::c_void;
                // winit's `WinitView` is flipped (origin is upper-left). Muda assumes an unflipped
                // view and flips Y internally. Pre-flip here to cancel out muda's inversion.
                let mut req = req;
                if let Some((x, y)) = req.position {
                    req.position = Some((x, view_height - y));
                }

                if let Some(index) = show_native_select_menu_macos(ns_view, &req) {
                    proxy.send_event(BlitzShellEvent::NativeSelect {
                        window_id,
                        select_id: req.select_id,
                        index,
                    });
                }
            });

            true
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = req;
            false
        }
    }

    #[cfg(all(
        feature = "clipboard",
        any(
            target_os = "windows",
            target_os = "macos",
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        )
    ))]
    fn get_clipboard_text(&self) -> Result<String, blitz_traits::shell::ClipboardError> {
        let mut cb = arboard::Clipboard::new().unwrap();
        cb.get_text()
            .map_err(|_| blitz_traits::shell::ClipboardError)
    }

    #[cfg(all(
        feature = "clipboard",
        any(
            target_os = "windows",
            target_os = "macos",
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        )
    ))]
    fn set_clipboard_text(&self, text: String) -> Result<(), blitz_traits::shell::ClipboardError> {
        let mut cb = arboard::Clipboard::new().unwrap();
        cb.set_text(text.to_owned())
            .map_err(|_| blitz_traits::shell::ClipboardError)
    }

    #[cfg(all(
        feature = "file_dialog",
        any(
            target_os = "windows",
            target_os = "macos",
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        )
    ))]
    fn open_file_dialog(
        &self,
        multiple: bool,
        filter: Option<FileDialogFilter>,
    ) -> Vec<std::path::PathBuf> {
        let mut dialog = rfd::FileDialog::new();
        if let Some(FileDialogFilter { name, extensions }) = filter {
            dialog = dialog.add_filter(&name, &extensions);
        }
        let files = if multiple {
            dialog.pick_files()
        } else {
            dialog.pick_file().map(|file| vec![file])
        };
        files.unwrap_or_default()
    }
}

#[cfg(target_os = "macos")]
fn escape_muda_menu_label(label: &str) -> String {
    // Muda uses '&' to mark mnemonics. Escape it so '&' renders as-is.
    label.replace('&', "&&")
}

#[cfg(target_os = "macos")]
fn show_native_select_menu_macos(
    ns_view: *const std::ffi::c_void,
    req: &NativeSelectMenuRequest,
) -> Option<usize> {
    use muda::ContextMenu as _;

    // The receiver is global; drain it so we don't accidentally consume an old activation.
    while muda::MenuEvent::receiver().try_recv().is_ok() {}

    let menu_id_prefix = format!("blitz-select:{}:", req.select_id);
    let menu = muda::Submenu::new("Select", true);
    let mut items = Vec::with_capacity(req.items.len());

    for (index, item) in req.items.iter().enumerate() {
        let id = muda::MenuId::new(format!("{menu_id_prefix}{index}"));
        let label = escape_muda_menu_label(&item.label);
        let enabled = !item.disabled;
        let checked = req.selected_index == Some(index);

        let menu_item = muda::CheckMenuItem::with_id(id, label, enabled, checked, None);
        if menu.append(&menu_item).is_err() {
            return None;
        }
        items.push(menu_item);
    }

    let position = req
        .position
        .map(|(x, y)| muda::dpi::PhysicalPosition::new(x as f64, y as f64).into());

    // This call blocks while the native menu is open.
    unsafe { menu.show_context_menu_for_nsview(ns_view, position) };

    while let Ok(event) = muda::MenuEvent::receiver().try_recv() {
        let id = event.id.as_ref();
        if let Some(index) = id
            .strip_prefix(&menu_id_prefix)
            .and_then(|rest| rest.parse::<usize>().ok())
        {
            return Some(index);
        }
    }

    None
}
