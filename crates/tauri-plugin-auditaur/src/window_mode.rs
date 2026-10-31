//! Keeps a drilled app's windows from taking the foreground.
//!
//! `auditaur drill test` sets `AUDITAUR_DRILL_WINDOW=background` by default. Showing a new window
//! normally activates it, taking focus from whatever the user is doing. While the app starts
//! (until its first drive request), a window that takes the foreground hands it straight back to
//! the window that had it when the app started, and sits just behind it. Once per window; after
//! that the app is left alone, so a user's click or the app's own `set_focus` still work.
//! Windows only: elsewhere the plugin doesn't advertise [`WINDOW_BACKGROUND_CAPABILITY`] and the
//! CLI reports background mode as unsupported.
//!
//! The CLI also sends [`WINDOW_ACTION`] drive requests, answered here without reaching the JS
//! bridge: `status` reports the window events so far (startup hand-backs and gate raises and
//! hand-backs), `front` brings the window forward for a human gate, and `back` hands the
//! foreground back afterwards. Both run on the UI thread, which owns the foreground.
//!
//! [`WINDOW_BACKGROUND_CAPABILITY`]: auditaur_core::drive_bridge::WINDOW_BACKGROUND_CAPABILITY

use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Mutex, OnceLock,
};
use std::time::Duration;

use auditaur_core::drive_bridge::{
    DriveBridgeRequest, DriveBridgeResponse, WINDOW_ACTION, WINDOW_MODE_ENV,
};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, Runtime, WebviewWindow, Window};

#[derive(Default)]
struct ModeState {
    background: bool,
    /// The foreground window (raw HWND) when the app started.
    prior: Option<isize>,
    /// Windows already handed back, by label.
    handled: HashSet<String>,
    /// What happened to windows: startup hand-backs and gate raises and hand-backs.
    events: Vec<Value>,
    /// The foreground window before the last `front`.
    before_front: Option<isize>,
}

static STATE: Mutex<Option<ModeState>> = Mutex::new(None);
/// Cleared by the first drive request; window activations after that are left alone.
static STARTING: AtomicBool = AtomicBool::new(true);

type Handler = Box<dyn Fn(&str, Option<&str>) -> Result<Value, String> + Send + Sync>;
/// Set at plugin setup; the poll command reaches the app through this (see `focus`).
static HANDLER: OnceLock<Handler> = OnceLock::new();

fn with_state<T>(f: impl FnOnce(&mut ModeState) -> T) -> T {
    let mut guard = STATE.lock().unwrap_or_else(|poison| poison.into_inner());
    f(guard.get_or_insert_with(ModeState::default))
}

/// Whether this platform can keep a window in the background.
pub(crate) fn supported() -> bool {
    cfg!(windows)
}

/// Read the requested mode and remember the user's foreground window, before any window exists.
pub(crate) fn init<R: Runtime>(app: &AppHandle<R>) {
    let background = std::env::var(WINDOW_MODE_ENV).is_ok_and(|mode| mode == "background");
    let prior = if background {
        platform::foreground().filter(|&hwnd| platform::foreign(hwnd))
    } else {
        None
    };
    with_state(|state| {
        state.background = background;
        state.prior = prior;
    });
    if HANDLER.get().is_none() {
        let app = app.clone();
        let _ = HANDLER.set(Box::new(move |command, label| {
            handle_command(&app, command, label)
        }));
    }
}

/// The drill has started driving the app: stop handing the foreground back.
pub(crate) fn started() {
    STARTING.store(false, Ordering::SeqCst);
}

/// A window was created or activated; hand the foreground back if it took it at startup.
pub(crate) fn window_activated<R: Runtime>(window: &Window<R>, trigger: &str) {
    if !STARTING.load(Ordering::SeqCst) {
        return;
    }
    let Some(hwnd) = platform::window_hwnd(window) else {
        return;
    };
    if platform::foreground() != Some(hwnd) {
        return;
    }
    let label = window.label().to_owned();
    // Decide under the lock, act without it: giving the foreground away sends this thread
    // activation messages that can come back here for another window.
    let Some(prior) = with_state(|state| claim(state, &label)) else {
        return;
    };
    let mut outcome = platform::hand_back(hwnd, prior);
    outcome["window"] = json!(label);
    outcome["trigger"] = json!(trigger);
    if outcome["restored"] == true {
        ::tracing::debug!(window = %label, "Auditaur kept the drilled window in the background");
    } else {
        ::tracing::warn!(window = %label, outcome = %outcome, "Auditaur could not hand the foreground back from the drilled window");
    }
    record(outcome);
}

/// Keep what happened to a window for `status`, stamped with when it happened.
fn record(mut event: Value) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64);
    event["unixMs"] = json!(now);
    with_state(|state| state.events.push(event));
}

/// How long `front` and `back` wait for the app's UI thread.
const MAIN_THREAD_TIMEOUT: Duration = Duration::from_secs(5);

/// Run `f` on the app's UI thread and wait for it. Windows lets only the thread that owns the
/// foreground give it away, so `SetForegroundWindow` from the poll command's worker is refused.
/// Never call this from the UI thread itself: it would wait on itself until the timeout.
fn on_main_thread<R: Runtime, T: Send + 'static>(
    app: &AppHandle<R>,
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .map_err(|error| format!("could not reach the app's UI thread: {error}"))?;
    rx.recv_timeout(MAIN_THREAD_TIMEOUT).map_err(|_| {
        format!(
            "the app's UI thread did not answer within {}s",
            MAIN_THREAD_TIMEOUT.as_secs()
        )
    })
}

/// `Some(prior)` the first time `label` needs handing back in background mode.
fn claim(state: &mut ModeState, label: &str) -> Option<Option<isize>> {
    (state.background && state.handled.insert(label.to_owned())).then_some(state.prior)
}

/// Answer a [`WINDOW_ACTION`] request natively; `None` for any other action.
pub(crate) fn handle(request: &DriveBridgeRequest) -> Option<DriveBridgeResponse> {
    if request.action != WINDOW_ACTION {
        return None;
    }
    let command = request.value.as_deref().unwrap_or("status");
    let result = match HANDLER.get() {
        Some(handler) => handler(command, request.window_label.as_deref()),
        None => Err("the Auditaur plugin is not set up".to_owned()),
    };
    let (ok, payload, error) = match result {
        Ok(payload) => (true, payload, None),
        Err(error) => (false, Value::Null, Some(error)),
    };
    Some(DriveBridgeResponse {
        schema_version: request.schema_version,
        protocol_version: request.protocol_version,
        request_id: request.request_id.clone(),
        action: request.action.clone(),
        selector: request.selector.clone(),
        visible_only: request.visible_only,
        ok,
        payload,
        error,
        completed_at_unix_nanos: crate::now_unix_nanos(),
    })
}

fn handle_command<R: Runtime>(
    app: &AppHandle<R>,
    command: &str,
    label: Option<&str>,
) -> Result<Value, String> {
    match command {
        "status" => Ok(with_state(|state| {
            json!({
                "mode": if state.background { "background" } else { "foreground" },
                "supported": supported(),
                "events": state.events,
            })
        })),
        "front" => {
            let window = resolve(app, label)?;
            let label = window.label().to_owned();
            // Stored on the UI thread, so a late run (after a timeout) can still be handed back:
            // `back` queues behind it.
            let result = on_main_thread(app, move || {
                let before = platform::foreground().filter(|&hwnd| platform::foreign(hwnd));
                with_state(|state| state.before_front = before);
                let _ = window.unminimize();
                let _ = window.show();
                window
                    .set_focus()
                    .map_err(|error| format!("could not bring the window to the front: {error}"))
            })
            .and_then(|focused| focused);
            record(json!({
                "window": label,
                "trigger": "gate-front",
                "raised": result.is_ok(),
                "error": result.as_ref().err(),
            }));
            result.map(|()| json!({ "window": label }))
        }
        "back" => {
            let window = resolve(app, label)?;
            let label = window.label().to_owned();
            let result = on_main_thread(app, move || {
                let before = with_state(|state| state.before_front.take());
                let Some(hwnd) = platform::webview_hwnd(&window) else {
                    return json!({ "skipped": "not supported on this platform" });
                };
                if platform::foreground() != Some(hwnd) {
                    return json!({
                        "skipped": "focus already moved away from the app, such as to the Drills canvas"
                    });
                }
                platform::hand_back(hwnd, before)
            });
            let mut event = match &result {
                Ok(outcome) => outcome.clone(),
                Err(error) => json!({ "restored": false, "error": error }),
            };
            event["window"] = json!(label);
            event["trigger"] = json!("gate-back");
            record(event.clone());
            // The event, so a failure reaches the CLI through its `error`.
            result.map(|_| event)
        }
        other => Err(format!(
            "unknown `{WINDOW_ACTION}` command `{other}`; expected status, front, or back"
        )),
    }
}

/// The named window, else the only one, else `main`, else the first by label.
fn resolve<R: Runtime>(
    app: &AppHandle<R>,
    label: Option<&str>,
) -> Result<WebviewWindow<R>, String> {
    if let Some(label) = label {
        return app
            .get_webview_window(label)
            .ok_or_else(|| format!("no Tauri WebView window matched `{label}`"));
    }
    let mut windows: Vec<_> = app.webview_windows().into_iter().collect();
    windows.sort_by(|(a, _), (b, _)| (a != "main").cmp(&(b != "main")).then(a.cmp(b)));
    windows
        .into_iter()
        .next()
        .map(|(_, window)| window)
        .ok_or_else(|| "the app has no WebView windows".to_owned())
}

#[cfg(windows)]
mod platform {
    use serde_json::{json, Value};
    use tauri::{Runtime, WebviewWindow, Window};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, IsWindow, SetForegroundWindow, SetWindowPos,
        HWND_BOTTOM, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };

    fn handle(raw: isize) -> HWND {
        HWND(raw as *mut _)
    }

    pub fn foreground() -> Option<isize> {
        // SAFETY: no arguments; returns null when no window is in the foreground.
        let hwnd = unsafe { GetForegroundWindow() };
        (!hwnd.is_invalid()).then_some(hwnd.0 as isize)
    }

    /// A live window of another process.
    pub fn foreign(raw: isize) -> bool {
        let hwnd = handle(raw);
        // SAFETY: both calls tolerate any handle value, including stale ones.
        if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            return false;
        }
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        pid != 0 && pid != std::process::id()
    }

    pub fn window_hwnd<R: Runtime>(window: &Window<R>) -> Option<isize> {
        window.hwnd().ok().map(|hwnd| hwnd.0 as isize)
    }

    pub fn webview_hwnd<R: Runtime>(window: &WebviewWindow<R>) -> Option<isize> {
        window.hwnd().ok().map(|hwnd| hwnd.0 as isize)
    }

    /// Give the foreground from `app` back to `prior` and put `app` just behind it, or at the
    /// bottom of the z-order when `prior` is gone or Windows refuses.
    pub fn hand_back(app: isize, prior: Option<isize>) -> Value {
        let prior = prior.filter(|&hwnd| foreign(hwnd));
        // SAFETY: `prior` is a live window; this process holds the foreground, so may give it away.
        let restored = prior.is_some_and(|hwnd| {
            unsafe { SetForegroundWindow(handle(hwnd)) }.as_bool() && foreground() == Some(hwnd)
        });
        let after = match prior {
            Some(hwnd) if restored => handle(hwnd),
            _ => HWND_BOTTOM,
        };
        // SAFETY: `app` is this process's window; flags keep its size, position, and activation.
        let placed = unsafe {
            SetWindowPos(
                handle(app),
                Some(after),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        }
        .is_ok();
        let error = match (prior, restored) {
            (_, true) => None,
            (None, _) => Some("the window that had the foreground before is gone"),
            (Some(_), false) => Some("Windows refused to give the foreground back"),
        };
        json!({
            "restored": restored,
            "behind": if restored { "previous foreground window" } else { "all windows" },
            "placed": placed,
            "error": error,
        })
    }
}

#[cfg(not(windows))]
mod platform {
    use serde_json::{json, Value};
    use tauri::{Runtime, WebviewWindow, Window};

    pub fn foreground() -> Option<isize> {
        None
    }

    pub fn foreign(_raw: isize) -> bool {
        false
    }

    pub fn window_hwnd<R: Runtime>(_window: &Window<R>) -> Option<isize> {
        None
    }

    pub fn webview_hwnd<R: Runtime>(_window: &WebviewWindow<R>) -> Option<isize> {
        None
    }

    pub fn hand_back(_app: isize, _prior: Option<isize>) -> Value {
        json!({ "restored": false, "error": "not supported on this platform" })
    }
}

#[cfg(test)]
mod tests {
    use super::{claim, handle, record, with_state, ModeState};
    use auditaur_core::drive_bridge::{DriveBridgeRequest, WINDOW_ACTION};
    use serde_json::json;

    #[test]
    fn each_window_is_handed_back_once_and_only_in_background_mode() {
        let mut state = ModeState {
            background: true,
            prior: Some(42),
            ..ModeState::default()
        };
        assert_eq!(claim(&mut state, "main"), Some(Some(42)));
        assert_eq!(claim(&mut state, "main"), None);
        assert_eq!(claim(&mut state, "other"), Some(Some(42)));

        let mut foreground = ModeState::default();
        assert_eq!(claim(&mut foreground, "main"), None);
    }

    fn request(action: &str, value: Option<&str>) -> DriveBridgeRequest {
        DriveBridgeRequest {
            schema_version: 1,
            protocol_version: 1,
            request_id: "r1".into(),
            action: action.into(),
            selector: None,
            value: value.map(str::to_owned),
            values: Vec::new(),
            visible_only: false,
            window_label: None,
            test_id: None,
            step_id: None,
            created_at_unix_nanos: 0,
        }
    }

    #[test]
    fn only_window_requests_are_answered_natively() {
        assert!(handle(&request("click", None)).is_none());
        let response = handle(&request(WINDOW_ACTION, Some("status"))).unwrap();
        assert_eq!(response.request_id, "r1");
        assert_eq!(response.action, WINDOW_ACTION);
    }

    #[test]
    fn recorded_events_are_timestamped() {
        record(json!({ "window": "recorded", "trigger": "gate-back", "restored": true }));
        let events = with_state(|state| state.events.clone());
        let event = events
            .iter()
            .find(|event| event["window"] == "recorded")
            .expect("the event is kept");
        assert!(event["unixMs"].as_u64().is_some_and(|ms| ms > 0));
    }
}
