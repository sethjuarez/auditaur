use auditaur_collector::receiver::OTelBatch;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
use serde::Serialize;
use std::io::Cursor;
use tauri::{AppHandle, Manager, Runtime, State, WebviewWindow};
use xcap::{
    image::{DynamicImage, ImageFormat, RgbaImage},
    Monitor as CaptureMonitor, Window as CaptureWindow,
};

use crate::{error::AuditaurError, state::AuditaurState};

pub const EXPORT_OTEL_BATCH_COMMAND: &str = "export_otel_batch";
pub const REGISTER_DRIVE_BRIDGE_COMMAND: &str = "register_drive_bridge";
pub const POLL_DRIVE_BRIDGE_REQUEST_COMMAND: &str = "poll_drive_bridge_request";
pub const COMPLETE_DRIVE_BRIDGE_REQUEST_COMMAND: &str = "complete_drive_bridge_request";
pub const CAPTURE_DRIVE_BRIDGE_SCREENSHOT_COMMAND: &str = "capture_drive_bridge_screenshot";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveBridgeScreenshot {
    format: &'static str,
    png_base64: String,
    width: u32,
    height: u32,
    screenshot_backend: &'static str,
    window_label: String,
    window_title: Option<String>,
    native_window_id: Option<u32>,
    native_window_title: Option<String>,
    native_app_name: Option<String>,
}

#[tauri::command]
pub async fn export_otel_batch(
    state: State<'_, AuditaurState>,
    batch: OTelBatch,
) -> Result<(), AuditaurError> {
    state.export_batch(batch)
}

#[tauri::command]
pub async fn register_drive_bridge<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AuditaurState>,
    window_label: Option<String>,
) -> Result<(), AuditaurError> {
    state.register_drive_bridge(window_label)?;
    if let Some((bridge_dir, alive)) = state.start_bridge_notifier_if_needed() {
        crate::start_drive_bridge_request_notifier(&app, bridge_dir, alive);
    }
    Ok(())
}

#[tauri::command]
pub async fn poll_drive_bridge_request(
    state: State<'_, AuditaurState>,
    window_label: Option<String>,
) -> Result<Option<auditaur_core::drive_bridge::DriveBridgeRequest>, AuditaurError> {
    state.poll_drive_bridge_request(window_label)
}

#[tauri::command]
pub async fn complete_drive_bridge_request(
    state: State<'_, AuditaurState>,
    response: auditaur_core::drive_bridge::DriveBridgeResponse,
) -> Result<(), AuditaurError> {
    state.complete_drive_bridge_request(response)
}

#[tauri::command]
pub async fn capture_drive_bridge_screenshot<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AuditaurState>,
    window_label: Option<String>,
) -> Result<DriveBridgeScreenshot, AuditaurError> {
    state.ensure_drive_bridge_available()?;
    tauri::async_runtime::spawn_blocking(move || capture_window_screenshot(&app, window_label))
        .await
        .map_err(|error| AuditaurError::new(format!("native screenshot task failed: {error}")))?
}

fn capture_window_screenshot<R: Runtime>(
    app: &AppHandle<R>,
    window_label: Option<String>,
) -> Result<DriveBridgeScreenshot, AuditaurError> {
    let window = resolve_tauri_window(app, window_label.as_deref())?;
    let window_label = window.label().to_string();
    let window_titles = candidate_window_titles(app, &window, &window_label);
    let window_title = window_titles.first().cloned();
    let capture = match select_capture_window(&window_titles) {
        Ok(capture_window) => {
            let native_window_id = capture_window.id().ok();
            let native_window_title = capture_window.title().ok();
            let native_app_name = capture_window.app_name().ok();
            let image = capture_window.capture_image().map_err(|error| {
                AuditaurError::new(format!("native screenshot capture failed: {error}"))
            })?;
            NativeCapture {
                image,
                native_window_id,
                native_window_title,
                native_app_name,
            }
        }
        Err(window_error) => {
            let image = capture_tauri_window_region(&window).map_err(|region_error| {
                AuditaurError::new(format!(
                    "{window_error}; monitor-region fallback failed: {region_error}"
                ))
            })?;
            NativeCapture {
                image,
                native_window_id: None,
                native_window_title: window_title.clone(),
                native_app_name: configured_product_name(app),
            }
        }
    };
    let image = capture.image;
    let width = image.width();
    let height = image.height();
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, ImageFormat::Png)
        .map_err(|error| {
            AuditaurError::new(format!("native screenshot PNG encoding failed: {error}"))
        })?;

    Ok(DriveBridgeScreenshot {
        format: "png",
        png_base64: BASE64_STANDARD.encode(bytes.into_inner()),
        width,
        height,
        screenshot_backend: "tauri_native_window_xcap",
        window_label,
        window_title,
        native_window_id: capture.native_window_id,
        native_window_title: capture.native_window_title,
        native_app_name: capture.native_app_name,
    })
}

struct NativeCapture {
    image: RgbaImage,
    native_window_id: Option<u32>,
    native_window_title: Option<String>,
    native_app_name: Option<String>,
}

fn capture_tauri_window_region<R: Runtime>(
    window: &WebviewWindow<R>,
) -> Result<RgbaImage, AuditaurError> {
    let position = window.outer_position().map_err(|error| {
        AuditaurError::new(format!("could not read Tauri window position: {error}"))
    })?;
    let size = window.outer_size().map_err(|error| {
        AuditaurError::new(format!("could not read Tauri window size: {error}"))
    })?;
    let center_x = position.x.saturating_add((size.width / 2) as i32);
    let center_y = position.y.saturating_add((size.height / 2) as i32);
    let monitor = CaptureMonitor::from_point(center_x, center_y).map_err(|error| {
        AuditaurError::new(format!("could not resolve window monitor: {error}"))
    })?;
    let monitor_x = monitor
        .x()
        .map_err(|error| AuditaurError::new(format!("could not read monitor x: {error}")))?;
    let monitor_y = monitor
        .y()
        .map_err(|error| AuditaurError::new(format!("could not read monitor y: {error}")))?;
    let monitor_width = monitor
        .width()
        .map_err(|error| AuditaurError::new(format!("could not read monitor width: {error}")))?;
    let monitor_height = monitor
        .height()
        .map_err(|error| AuditaurError::new(format!("could not read monitor height: {error}")))?;
    let x = position.x.saturating_sub(monitor_x).max(0) as u32;
    let y = position.y.saturating_sub(monitor_y).max(0) as u32;
    let width = size.width.min(monitor_width.saturating_sub(x));
    let height = size.height.min(monitor_height.saturating_sub(y));
    if width == 0 || height == 0 {
        return Err(AuditaurError::new(format!(
            "window bounds ({}, {}, {}x{}) do not intersect monitor bounds ({}, {}, {}x{})",
            position.x,
            position.y,
            size.width,
            size.height,
            monitor_x,
            monitor_y,
            monitor_width,
            monitor_height
        )));
    }
    monitor
        .capture_region(x, y, width, height)
        .map_err(|error| {
            AuditaurError::new(format!("native monitor region capture failed: {error}"))
        })
}

fn candidate_window_titles<R: Runtime>(
    app: &AppHandle<R>,
    window: &WebviewWindow<R>,
    window_label: &str,
) -> Vec<String> {
    let mut titles = Vec::new();
    push_title(&mut titles, window.title().ok());
    push_title(&mut titles, configured_window_title(app, window_label));
    push_title(&mut titles, configured_product_name(app));
    titles
}

fn push_title(titles: &mut Vec<String>, title: Option<String>) {
    let Some(title) = title else {
        return;
    };
    let title = title.trim();
    if title.is_empty() || titles.iter().any(|existing| existing == title) {
        return;
    }
    titles.push(title.to_string());
}

fn configured_window_title<R: Runtime>(app: &AppHandle<R>, window_label: &str) -> Option<String> {
    app.config()
        .app
        .windows
        .iter()
        .find(|window| window.label == window_label)
        .map(|window| window.title.clone())
        .filter(|title| !title.trim().is_empty())
}

fn configured_product_name<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    app.config()
        .product_name
        .clone()
        .filter(|title| !title.trim().is_empty())
}

fn resolve_tauri_window<R: Runtime>(
    app: &AppHandle<R>,
    window_label: Option<&str>,
) -> Result<WebviewWindow<R>, AuditaurError> {
    if let Some(window_label) = window_label {
        return app.get_webview_window(window_label).ok_or_else(|| {
            AuditaurError::new(format!("no Tauri WebView window matched `{window_label}`"))
        });
    }

    let mut windows: Vec<_> = app.webview_windows().into_values().collect();
    match windows.len() {
        0 => Err(AuditaurError::new(
            "native screenshot capture failed: no Tauri WebView windows are available",
        )),
        1 => Ok(windows.remove(0)),
        _ => Err(AuditaurError::new(
            "native screenshot capture requires a windowLabel when multiple Tauri WebView windows exist",
        )),
    }
}

fn select_capture_window(titles: &[String]) -> Result<CaptureWindow, AuditaurError> {
    let pid = std::process::id();
    let windows = CaptureWindow::all().map_err(|error| {
        AuditaurError::new(format!("native window enumeration failed: {error}"))
    })?;
    let mut same_process: Vec<_> = windows
        .iter()
        .filter(|window| window.pid().ok() == Some(pid))
        .cloned()
        .collect();

    for title in titles {
        let mut titled: Vec<_> = same_process
            .iter()
            .filter(|window| window.title().ok().as_deref() == Some(title))
            .cloned()
            .collect();
        if let Some(focused) = take_focused(&mut titled) {
            return Ok(focused);
        }
        if let Some(window) = titled.into_iter().next() {
            return Ok(window);
        }
    }

    if let Some(focused) = take_focused(&mut same_process) {
        return Ok(focused);
    }
    match same_process.len() {
        0 => Err(AuditaurError::new(
            "native screenshot capture failed: no native window matched the current app process",
        )),
        1 => Ok(same_process.remove(0)),
        count => Err(AuditaurError::new(format!(
            "native screenshot capture found {count} native windows for this process; set driveBridge.windowLabel and a unique Tauri window title"
        ))),
    }
}

fn take_focused(windows: &mut Vec<CaptureWindow>) -> Option<CaptureWindow> {
    let index = windows
        .iter()
        .position(|window| window.is_focused().ok() == Some(true))?;
    Some(windows.remove(index))
}
