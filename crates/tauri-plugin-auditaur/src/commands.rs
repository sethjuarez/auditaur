use auditaur_collector::receiver::OTelBatch;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::sync::mpsc;
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime, State, WebviewWindow};
use xcap::{
    image::{self, DynamicImage, GenericImageView, ImageFormat, RgbaImage},
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
    screenshot_scope: &'static str,
    window_label: String,
    window_title: Option<String>,
    selector_rect: Option<ScreenshotTargetRect>,
    webview_screenshot_error: Option<String>,
    native_window_id: Option<u32>,
    native_window_title: Option<String>,
    native_app_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenshotTargetRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    viewport_width: f64,
    viewport_height: f64,
    device_pixel_ratio: f64,
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
    target_rect: Option<ScreenshotTargetRect>,
) -> Result<DriveBridgeScreenshot, AuditaurError> {
    state.ensure_drive_bridge_available()?;
    tauri::async_runtime::spawn_blocking(move || {
        capture_screenshot(&app, window_label, target_rect)
    })
    .await
    .map_err(|error| AuditaurError::new(format!("native screenshot task failed: {error}")))?
}

fn capture_screenshot<R: Runtime>(
    app: &AppHandle<R>,
    window_label: Option<String>,
    target_rect: Option<ScreenshotTargetRect>,
) -> Result<DriveBridgeScreenshot, AuditaurError> {
    let window = resolve_tauri_window(app, window_label.as_deref())?;
    let window_label = window.label().to_string();
    let window_titles = candidate_window_titles(app, &window, &window_label);
    let window_title = window_titles.first().cloned();
    match capture_webview_screenshot(&window, target_rect.clone()) {
        Ok(capture) => {
            return Ok(DriveBridgeScreenshot {
                format: "png",
                png_base64: BASE64_STANDARD.encode(capture.png),
                width: capture.width,
                height: capture.height,
                screenshot_backend: "tauri_native_webview_snapshot",
                screenshot_scope: capture.scope,
                window_label,
                window_title,
                selector_rect: target_rect,
                webview_screenshot_error: None,
                native_window_id: None,
                native_window_title: None,
                native_app_name: None,
            });
        }
        Err(webview_error) => {
            let mut fallback =
                capture_window_screenshot(app, &window, window_label, window_title, target_rect)?;
            fallback.webview_screenshot_error = Some(webview_error.to_string());
            Ok(fallback)
        }
    }
}

fn capture_window_screenshot<R: Runtime>(
    app: &AppHandle<R>,
    window: &WebviewWindow<R>,
    window_label: String,
    window_title: Option<String>,
    target_rect: Option<ScreenshotTargetRect>,
) -> Result<DriveBridgeScreenshot, AuditaurError> {
    let window_titles = candidate_window_titles(app, window, &window_label);
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
        screenshot_scope: "window",
        window_label,
        window_title,
        selector_rect: target_rect,
        webview_screenshot_error: None,
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

struct WebviewCapture {
    png: Vec<u8>,
    width: u32,
    height: u32,
    scope: &'static str,
}

fn capture_webview_screenshot<R: Runtime>(
    window: &WebviewWindow<R>,
    target_rect: Option<ScreenshotTargetRect>,
) -> Result<WebviewCapture, AuditaurError> {
    let png = capture_webview_png(window)?;
    prepare_webview_capture(png, target_rect)
}

fn prepare_webview_capture(
    png: Vec<u8>,
    target_rect: Option<ScreenshotTargetRect>,
) -> Result<WebviewCapture, AuditaurError> {
    let image = image::load_from_memory(&png).map_err(|error| {
        AuditaurError::new(format!("webview screenshot image decode failed: {error}"))
    })?;
    let (image_width, image_height) = image.dimensions();
    let Some(rect) = target_rect else {
        let mut bytes = Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, ImageFormat::Png)
            .map_err(|error| AuditaurError::new(format!("webview PNG encoding failed: {error}")))?;
        return Ok(WebviewCapture {
            png: bytes.into_inner(),
            width: image_width,
            height: image_height,
            scope: "webview",
        });
    };

    let crop = scaled_crop_rect(&rect, image_width, image_height)?;
    let cropped = image.crop_imm(crop.x, crop.y, crop.width, crop.height);
    let mut bytes = Cursor::new(Vec::new());
    cropped
        .write_to(&mut bytes, ImageFormat::Png)
        .map_err(|error| {
            AuditaurError::new(format!("webview selector PNG encoding failed: {error}"))
        })?;
    Ok(WebviewCapture {
        png: bytes.into_inner(),
        width: crop.width,
        height: crop.height,
        scope: "selector",
    })
}

struct CropRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

fn scaled_crop_rect(
    rect: &ScreenshotTargetRect,
    image_width: u32,
    image_height: u32,
) -> Result<CropRect, AuditaurError> {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return Err(AuditaurError::new(
            "selector screenshot target has empty bounds",
        ));
    }
    let viewport_width = rect.viewport_width.max(1.0);
    let viewport_height = rect.viewport_height.max(1.0);
    let scale_x = image_width as f64 / viewport_width;
    let scale_y = image_height as f64 / viewport_height;
    let left = rect.x.max(0.0).min(viewport_width);
    let top = rect.y.max(0.0).min(viewport_height);
    let right = (rect.x + rect.width).max(0.0).min(viewport_width);
    let bottom = (rect.y + rect.height).max(0.0).min(viewport_height);
    if right <= left || bottom <= top {
        return Err(AuditaurError::new(
            "selector screenshot target does not intersect the visible viewport",
        ));
    }
    let x = (left * scale_x).floor().max(0.0) as u32;
    let y = (top * scale_y).floor().max(0.0) as u32;
    let right = (right * scale_x).ceil().min(image_width as f64) as u32;
    let bottom = (bottom * scale_y).ceil().min(image_height as f64) as u32;
    let width = right.saturating_sub(x);
    let height = bottom.saturating_sub(y);
    if width == 0 || height == 0 {
        return Err(AuditaurError::new(
            "selector screenshot target is empty after scaling to WebView pixels",
        ));
    }
    Ok(CropRect {
        x,
        y,
        width,
        height,
    })
}

#[cfg(not(target_os = "macos"))]
fn capture_webview_png<R: Runtime>(window: &WebviewWindow<R>) -> Result<Vec<u8>, AuditaurError> {
    let (tx, rx) = mpsc::channel();
    window
        .with_webview(move |webview| {
            let result = capture_platform_webview_png(webview).map_err(|error| error.to_string());
            let _ = tx.send(result);
        })
        .map_err(|error| {
            AuditaurError::new(format!(
                "could not dispatch WebView screenshot task: {error}"
            ))
        })?;
    rx.recv_timeout(Duration::from_secs(15))
        .map_err(|error| {
            AuditaurError::new(format!("webview screenshot task did not complete: {error}"))
        })?
        .map_err(AuditaurError::new)
}

#[cfg(target_os = "macos")]
fn capture_webview_png<R: Runtime>(window: &WebviewWindow<R>) -> Result<Vec<u8>, AuditaurError> {
    let (tx, rx) = mpsc::channel();
    window
        .with_webview(move |webview| {
            if let Err(error) = begin_platform_webview_png(webview, tx.clone()) {
                let _ = tx.send(Err(error.to_string()));
            }
        })
        .map_err(|error| {
            AuditaurError::new(format!(
                "could not dispatch WebView screenshot task: {error}"
            ))
        })?;
    rx.recv_timeout(Duration::from_secs(15))
        .map_err(|error| {
            AuditaurError::new(format!("webview screenshot task did not complete: {error}"))
        })?
        .map_err(AuditaurError::new)
}

#[cfg(windows)]
fn capture_platform_webview_png(
    webview: tauri::webview::PlatformWebview,
) -> Result<Vec<u8>, AuditaurError> {
    use webview2_com::CapturePreviewCompletedHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG;
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;

    let stream =
        unsafe { CreateStreamOnHGlobal(HGLOBAL(std::ptr::null_mut()), true) }.map_err(|error| {
            AuditaurError::new(format!(
                "could not allocate WebView2 screenshot stream: {error}"
            ))
        })?;
    let webview2 = unsafe {
        webview.controller().CoreWebView2().map_err(|error| {
            AuditaurError::new(format!("could not access CoreWebView2: {error}"))
        })?
    };
    let capture_stream = stream.clone();
    CapturePreviewCompletedHandler::wait_for_async_operation(
        Box::new(move |handler| unsafe {
            webview2
                .CapturePreview(
                    COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                    &capture_stream,
                    &handler,
                )
                .map_err(webview2_com::Error::WindowsError)
        }),
        Box::new(|error_code| error_code),
    )
    .map_err(|error| AuditaurError::new(format!("WebView2 CapturePreview failed: {error}")))?;

    read_istream(&stream)
}

#[cfg(windows)]
fn read_istream(stream: &windows::Win32::System::Com::IStream) -> Result<Vec<u8>, AuditaurError> {
    use windows::Win32::System::Com::{STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET};

    let mut stat = STATSTG::default();
    unsafe {
        stream.Stat(&mut stat, STATFLAG_NONAME).map_err(|error| {
            AuditaurError::new(format!(
                "could not inspect WebView2 screenshot stream: {error}"
            ))
        })?;
        stream.Seek(0, STREAM_SEEK_SET, None).map_err(|error| {
            AuditaurError::new(format!(
                "could not rewind WebView2 screenshot stream: {error}"
            ))
        })?;
    }
    let len: usize = stat.cbSize.try_into().map_err(|_| {
        AuditaurError::new(format!(
            "WebView2 screenshot stream is too large: {} bytes",
            stat.cbSize
        ))
    })?;
    let mut bytes = vec![0; len];
    let mut read = 0;
    let result = unsafe {
        stream.Read(
            bytes.as_mut_ptr().cast(),
            len.try_into().unwrap_or(u32::MAX),
            Some(&mut read),
        )
    };
    result.ok().map_err(|error| {
        AuditaurError::new(format!(
            "could not read WebView2 screenshot stream: {error}"
        ))
    })?;
    bytes.truncate(read as usize);
    Ok(bytes)
}

#[cfg(target_os = "macos")]
fn begin_platform_webview_png(
    webview: tauri::webview::PlatformWebview,
    tx: mpsc::Sender<Result<Vec<u8>, String>>,
) -> Result<(), AuditaurError> {
    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSImage;
    use objc2_foundation::NSError;
    use objc2_web_kit::{WKSnapshotConfiguration, WKWebView};

    let mtm = MainThreadMarker::new()
        .ok_or_else(|| AuditaurError::new("WKWebView screenshot must run on the main thread"))?;
    let snapshot_config = unsafe { WKSnapshotConfiguration::new(mtm) };
    unsafe {
        snapshot_config.setAfterScreenUpdates(true);
    }

    let config_for_callback = snapshot_config.clone();
    let completion = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
        let _keep_config_alive = &config_for_callback;
        let result = unsafe { ns_image_png_bytes(image, error) }.map_err(|error| error.to_string());
        let _ = tx.send(result);
    });
    let wkwebview = unsafe {
        (webview.inner() as *mut WKWebView)
            .as_ref()
            .ok_or_else(|| AuditaurError::new("WKWebView pointer was null"))?
    };
    unsafe {
        wkwebview
            .takeSnapshotWithConfiguration_completionHandler(Some(&snapshot_config), &completion);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
unsafe fn ns_image_png_bytes(
    image: *mut objc2_app_kit::NSImage,
    error: *mut objc2_foundation::NSError,
) -> Result<Vec<u8>, AuditaurError> {
    use objc2::{msg_send, rc::Retained};
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSBitmapImageRepPropertyKey};
    use objc2_foundation::{NSData, NSDictionary, NSString};

    if !error.is_null() {
        let description: *mut NSString = msg_send![error, localizedDescription];
        let description = description
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "unknown error".to_string());
        return Err(AuditaurError::new(format!(
            "WKWebView snapshot failed: {}",
            description
        )));
    }
    let image = image
        .as_ref()
        .ok_or_else(|| AuditaurError::new("WKWebView snapshot did not return an image"))?;
    let data: Option<Retained<NSData>> = image.TIFFRepresentation();
    let data =
        data.ok_or_else(|| AuditaurError::new("WKWebView snapshot image had no TIFF data"))?;
    let bitmap = NSBitmapImageRep::imageRepWithData(&data)
        .ok_or_else(|| AuditaurError::new("WKWebView snapshot image had no bitmap data"))?;
    let properties = NSDictionary::<NSBitmapImageRepPropertyKey, objc2::runtime::AnyObject>::new();
    let png = bitmap
        .representationUsingType_properties(NSBitmapImageFileType::PNG, &properties)
        .ok_or_else(|| AuditaurError::new("WKWebView snapshot image had no PNG data"))?;
    ns_data_bytes(&png)
}

#[cfg(target_os = "macos")]
unsafe fn ns_data_bytes(data: &objc2_foundation::NSData) -> Result<Vec<u8>, AuditaurError> {
    use objc2::msg_send;

    let len = data.length();
    let ptr: *const u8 = msg_send![data, bytes];
    if ptr.is_null() {
        return Err(AuditaurError::new("NSData bytes pointer was null"));
    }
    Ok(std::slice::from_raw_parts(ptr, len).to_vec())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn capture_platform_webview_png(
    _webview: tauri::webview::PlatformWebview,
) -> Result<Vec<u8>, AuditaurError> {
    Err(AuditaurError::new(
        "native WebView screenshots are not implemented on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_selector_rect_to_webview_pixels() {
        let crop = scaled_crop_rect(
            &ScreenshotTargetRect {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 50.0,
                viewport_width: 400.0,
                viewport_height: 300.0,
                device_pixel_ratio: 2.0,
            },
            800,
            600,
        )
        .expect("crop rect");

        assert_eq!(crop.x, 20);
        assert_eq!(crop.y, 40);
        assert_eq!(crop.width, 200);
        assert_eq!(crop.height, 100);
    }

    #[test]
    fn clips_selector_rect_to_visible_viewport() {
        let crop = scaled_crop_rect(
            &ScreenshotTargetRect {
                x: -10.0,
                y: 10.0,
                width: 30.0,
                height: 50.0,
                viewport_width: 100.0,
                viewport_height: 100.0,
                device_pixel_ratio: 1.0,
            },
            100,
            100,
        )
        .expect("crop rect");

        assert_eq!(crop.x, 0);
        assert_eq!(crop.y, 10);
        assert_eq!(crop.width, 20);
        assert_eq!(crop.height, 50);
    }
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
