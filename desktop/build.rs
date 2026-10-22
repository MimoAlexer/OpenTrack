fn main() {
    if std::env::var_os("CARGO_FEATURE_SHELL").is_some() {
        tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&[
                "connection_read",
                "connection_select",
                "connection_forget",
                "auth_begin",
                "auth_cancel",
                "auth_logout",
                "open_browser_action",
                "open_document_link",
                "api_request",
                "attachment_pick",
                "attachment_release",
                "attachment_upload",
                "attachment_cancel",
                "attachment_download",
                "issue_link_read",
                "issue_link_dismiss",
            ]),
        ))
        .expect("Build the desktop permission manifest");
    }
}
