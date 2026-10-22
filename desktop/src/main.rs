#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use opentrack_desktop::{
    attachments::{
        CancelRequest, DownloadReply, DownloadRequest, PickedFile, ReleaseRequest, UploadRequest,
    },
    auth::{BeginReply, BeginRequest},
    credentials::SystemVault,
    links::DocumentLinkRequest,
    persistence::Persistence,
    protocol::{ApiRequest, NativeError},
    state::{
        BrowserRequest, ClearLinkRequest, ConnectionState, Expected, Generation, IssueLinkState,
        LogoutState, Runtime, SelectRequest,
    },
    transport::ApiReply,
};
use std::{process::Stdio, sync::Arc};
use tauri::{
    AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder,
    webview::NewWindowResponse,
};
use tauri_plugin_dialog::DialogExt;
type Native<'a> = State<'a, Arc<Runtime>>;
#[tauri::command]
async fn connection_read(native: Native<'_>) -> Result<ConnectionState, NativeError> {
    Ok(native.read().await)
}
#[tauri::command]
async fn connection_select(
    native: Native<'_>,
    request: SelectRequest,
) -> Result<ConnectionState, NativeError> {
    native.select(request).await
}
#[tauri::command]
async fn connection_forget(
    native: Native<'_>,
    request: Expected,
) -> Result<ConnectionState, NativeError> {
    native.forget(request).await
}
#[tauri::command]
async fn auth_begin(native: Native<'_>, request: BeginRequest) -> Result<BeginReply, NativeError> {
    native.inner().begin(request).await
}
#[tauri::command]
async fn auth_cancel(
    native: Native<'_>,
    request: Expected,
) -> Result<ConnectionState, NativeError> {
    native.cancel_auth(request).await
}
#[tauri::command]
async fn auth_logout(native: Native<'_>, request: Expected) -> Result<LogoutState, NativeError> {
    native.logout(request).await
}
#[tauri::command]
async fn open_browser_action(
    native: Native<'_>,
    request: BrowserRequest,
) -> Result<Generation, NativeError> {
    native.browser_action(request).await
}
#[tauri::command]
async fn api_request(native: Native<'_>, request: ApiRequest) -> Result<ApiReply, NativeError> {
    native.api(request).await
}
#[tauri::command]
async fn attachment_pick(
    app: AppHandle,
    native: Native<'_>,
    request: Expected,
) -> Result<Vec<PickedFile>, NativeError> {
    let _permit = native
        .attachment_dialog_permit(&request.expected_generation)
        .await?;
    let (send, receive) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Select attachments for OpenTrack")
        .pick_files(move |paths| {
            let _ = send.send(paths);
        });
    let paths = receive
        .await
        .map_err(|_| NativeError::invalid())?
        .unwrap_or_default()
        .into_iter()
        .map(|path| path.into_path().map_err(|_| NativeError::invalid()))
        .collect::<Result<Vec<_>, _>>()?;
    native.accept_attachment_selection(request, paths).await
}
#[tauri::command]
async fn attachment_release(
    native: Native<'_>,
    request: ReleaseRequest,
) -> Result<(), NativeError> {
    native.attachment_release(request).await
}
#[tauri::command]
async fn attachment_cancel(native: Native<'_>, request: CancelRequest) -> Result<(), NativeError> {
    native.attachment_cancel(request).await
}
#[tauri::command]
async fn attachment_upload(
    app: AppHandle,
    native: Native<'_>,
    request: UploadRequest,
) -> Result<ApiReply, NativeError> {
    native
        .attachment_upload(
            request,
            Arc::new(move |progress| {
                let _ = app.emit_to("main", "opentrack-attachment-progress", progress);
            }),
        )
        .await
}
#[tauri::command]
async fn attachment_download(
    app: AppHandle,
    native: Native<'_>,
    request: DownloadRequest,
) -> Result<DownloadReply, NativeError> {
    native
        .attachment_download(request, move |filename| async move {
            let (send, receive) = tokio::sync::oneshot::channel();
            app.dialog()
                .file()
                .set_title("Save attachment from OpenTrack")
                .set_file_name(filename)
                .save_file(move |path| {
                    let _ = send.send(path);
                });
            receive
                .await
                .ok()
                .flatten()
                .and_then(|path| path.into_path().ok())
        })
        .await
}
#[tauri::command]
async fn open_document_link(request: DocumentLinkRequest) -> Result<(), NativeError> {
    open_browser(request.checked().map_err(|_| NativeError::invalid())?)
}
#[tauri::command]
async fn issue_link_read(native: Native<'_>) -> Result<Option<IssueLinkState>, NativeError> {
    Ok(native.issue_link().await)
}
#[tauri::command]
async fn issue_link_dismiss(
    native: Native<'_>,
    request: ClearLinkRequest,
) -> Result<(), NativeError> {
    native.clear_link(request).await
}
struct LinkInbox(tokio::sync::mpsc::Sender<String>);
fn accept_links(app: &AppHandle, arguments: Vec<String>) {
    let Some(inbox) = app.try_state::<LinkInbox>() else {
        return;
    };
    for argument in arguments.into_iter().take(32) {
        if argument.len() <= 4096 && argument.starts_with("opentrack:") {
            let _ = inbox.0.try_send(argument);
        }
    }
}
fn open_browser(url: &str) -> Result<(), NativeError> {
    static LANE: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> = std::sync::OnceLock::new();
    let permit = LANE
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(1)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            NativeError::new(
                "browser_busy",
                "The system browser is already opening. Try again shortly.",
                true,
            )
        })?;
    let mut process = tokio::process::Command::new("xdg-open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| {
            NativeError::new(
                "browser_unavailable",
                "The system browser could not be opened.",
                true,
            )
        })?;
    tauri::async_runtime::spawn(async move {
        let _permit = permit;
        let _ = tokio::time::timeout(std::time::Duration::from_secs(15), process.wait()).await;
    });
    Ok(())
}
fn main() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(
            |app, arguments, _cwd| {
                accept_links(app, arguments);
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            },
        ))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            connection_read,
            connection_select,
            connection_forget,
            auth_begin,
            auth_cancel,
            auth_logout,
            open_browser_action,
            open_document_link,
            api_request,
            attachment_pick,
            attachment_release,
            attachment_upload,
            attachment_cancel,
            attachment_download,
            issue_link_read,
            issue_link_dismiss
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let persistence = Persistence::new(app.path().app_config_dir()?)
                .map_err(|_| "Private desktop configuration is unavailable")?;
            let native = Runtime::new(
                persistence,
                Arc::new(SystemVault),
                Arc::new(move |event| {
                    let _ = handle.emit_to("main", "opentrack-native-auth", event);
                }),
                Arc::new(open_browser),
            )
            .map_err(|_| "Desktop initialization failed")?;
            let (link_tx, mut link_rx) = tokio::sync::mpsc::channel::<String>(8);
            let link_native = native.clone();
            let link_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Some(input) = link_rx.recv().await {
                    if let Some(link) = link_native.accept_link(&input).await {
                        let _ = link_app.emit_to("main", "opentrack-open-issue", link);
                    }
                }
            });
            app.manage(LinkInbox(link_tx));
            app.manage(native);
            accept_links(app.handle(), std::env::args().skip(1).collect());
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("OpenTrack")
                .inner_size(1280.0, 850.0)
                .min_inner_size(760.0, 560.0)
                .on_navigation(opentrack_desktop::links::bundled_navigation)
                .on_new_window(|_, _| NewWindowResponse::Deny)
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!());
    if result.is_err() {
        eprintln!(
            "OpenTrack desktop could not start. Check the display and private configuration."
        );
        std::process::exit(1);
    }
}
