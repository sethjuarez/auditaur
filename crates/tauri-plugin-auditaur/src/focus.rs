//! Page focus for driven windows.
//!
//! While the app window isn't the OS foreground window, Chromium reports
//! `document.hasFocus() === false` and `blur()`/`focus()` fire no events, so a field that saves on
//! blur never saves under a drill. Like Playwright, turn on DevTools focus emulation for each
//! window the first time it's driven: the page then behaves as focused whatever window the user is
//! in. It lasts across reloads. WebView2 only; WebKit has no equivalent.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use tauri::{AppHandle, Manager, Runtime};

static EMULATED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

type Emulator = Box<dyn Fn(Option<&str>) + Send + Sync>;

/// Set when the drive bridge registers. The poll command can't take an `AppHandle` without
/// changing its public signature, so it reaches the app through this.
static EMULATOR: OnceLock<Emulator> = OnceLock::new();

/// Remember `app` so a later drive request can turn on focus emulation.
pub(crate) fn arm<R: Runtime>(app: &AppHandle<R>) {
    if EMULATOR.get().is_none() {
        let app = app.clone();
        let _ = EMULATOR.set(Box::new(move |label| emulate(&app, label)));
    }
}

/// Turn on focus emulation for the window `label` names (every window when `None`), once each.
/// Best effort: a failure is logged and never fails the drive request.
pub(crate) fn emulate_for_drive(label: Option<&str>) {
    if let Some(emulate) = EMULATOR.get() {
        emulate(label);
    }
}

fn emulate<R: Runtime>(app: &AppHandle<R>, label: Option<&str>) {
    let windows: Vec<_> = match label {
        Some(label) => app.get_webview_window(label).into_iter().collect(),
        None => app.webview_windows().into_values().collect(),
    };
    for window in windows {
        if !first_time(window.label()) {
            continue;
        }
        if let Err(error) = platform::emulate(&window) {
            ::tracing::warn!(
                window = window.label(),
                %error,
                "Auditaur could not turn on focus emulation; blur and focus events may not fire while the window is in the background"
            );
        }
    }
}

fn first_time(label: &str) -> bool {
    EMULATED
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .get_or_insert_with(HashSet::new)
        .insert(label.to_owned())
}

/// A window reopened under the same label is a new WebView, so it needs emulating again.
pub(crate) fn forget(label: &str) {
    if let Some(labels) = EMULATED
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .as_mut()
    {
        labels.remove(label);
    }
}

#[cfg(windows)]
mod platform {
    use std::sync::mpsc;
    use std::time::Duration;

    use tauri::{Runtime, WebviewWindow};
    use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
    use windows::core::HSTRING;

    pub fn emulate<R: Runtime>(window: &WebviewWindow<R>) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        window
            .with_webview(move |webview| {
                let _ = tx.send(call(webview));
            })
            .map_err(|error| format!("could not reach the WebView: {error}"))?;
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|_| "WebView2 did not answer within 5 s".to_owned())?
    }

    fn call(webview: tauri::webview::PlatformWebview) -> Result<(), String> {
        // SAFETY: COM calls on the WebView's own thread, where `with_webview` runs.
        let core = unsafe { webview.controller().CoreWebView2() }
            .map_err(|error| format!("could not access CoreWebView2: {error}"))?;
        CallDevToolsProtocolMethodCompletedHandler::wait_for_async_operation(
            Box::new(move |handler| unsafe {
                core.CallDevToolsProtocolMethod(
                    &HSTRING::from("Emulation.setFocusEmulationEnabled"),
                    &HSTRING::from(r#"{"enabled":true}"#),
                    &handler,
                )
                .map_err(webview2_com::Error::WindowsError)
            }),
            Box::new(|error_code, _result| error_code),
        )
        .map_err(|error| format!("Emulation.setFocusEmulationEnabled failed: {error}"))
    }
}

#[cfg(not(windows))]
mod platform {
    use tauri::{Runtime, WebviewWindow};

    pub fn emulate<R: Runtime>(_window: &WebviewWindow<R>) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{first_time, forget};

    #[test]
    fn each_window_is_emulated_once() {
        assert!(first_time("focus-test-window"));
        assert!(!first_time("focus-test-window"));
        assert!(first_time("focus-test-other"));
    }

    #[test]
    fn a_destroyed_window_is_emulated_again() {
        assert!(first_time("focus-test-reopened"));
        forget("focus-test-reopened");
        assert!(first_time("focus-test-reopened"));
    }
}
