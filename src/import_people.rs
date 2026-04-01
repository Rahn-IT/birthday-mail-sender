use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use axum::{
    extract::{Multipart, State},
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Serialize;
use uuid::Uuid;

use crate::{AppState, error::AppError, users::CurrentUser};

const UPLOADS_PATH: &str = "./db/uploads";
const MAX_UPLOAD_AGE: Duration = Duration::from_secs(60 * 60);
const CLEANUP_INTERVAL: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Serialize)]
struct ImportPageView {
    is_admin: bool,
    has_error: bool,
    error_message: Option<String>,
}

pub async fn ensure_uploads_dir() -> Result<(), AppError> {
    tokio::fs::create_dir_all(UPLOADS_PATH).await?;
    Ok(())
}

pub async fn index(
    State(state): State<AppState>,
    current_user: CurrentUser,
) -> Result<Html<String>, AppError> {
    let template = state
        .jinja
        .get_template("import.html")
        .expect("template is loaded");
    let rendered = template.render(ImportPageView {
        is_admin: current_user.is_admin,
        has_error: false,
        error_message: None,
    })?;
    Ok(Html(rendered))
}

pub async fn upload(
    State(state): State<AppState>,
    current_user: CurrentUser,
    mut multipart: Multipart,
) -> Result<Response, AppError> {
    ensure_uploads_dir().await?;

    while let Some(field) = multipart.next_field().await? {
        if field.name() != Some("spreadsheet_file") {
            continue;
        }

        let original_name = field.file_name().unwrap_or_default().to_string();
        let extension = match normalize_spreadsheet_extension(&original_name) {
            Some(value) => value,
            None => {
                return render_index_with_error(
                    &state,
                    &current_user,
                    "Please upload a spreadsheet file with one of these extensions: .xls, .xlsx, .xlsm, .xlsb, .ods.",
                )
                .map(IntoResponse::into_response);
            }
        };

        let data = field.bytes().await?;
        if data.is_empty() {
            return render_index_with_error(&state, &current_user, "Uploaded file is empty.")
                .map(IntoResponse::into_response);
        }

        let filename = format!("{}.{}", Uuid::new_v4(), extension);
        let upload_path = upload_path_for(&filename);
        tokio::fs::write(upload_path, data).await?;

        return Ok(Redirect::to(&format!("/import/{}", filename)).into_response());
    }

    render_index_with_error(&state, &current_user, "Please choose a spreadsheet file to upload.")
        .map(IntoResponse::into_response)
}

pub async fn run_upload_cleanup_scheduler() {
    let mut interval = tokio::time::interval(CLEANUP_INTERVAL);
    interval.tick().await;

    loop {
        interval.tick().await;

        match delete_old_uploads().await {
            Ok(0) => {
                println!("Upload cleanup: no expired file(s) found.");
            }
            Ok(count) => {
                println!("Upload cleanup: deleted {} expired file(s).", count);
            }
            Err(err) => {
                eprintln!("Upload cleanup failed: {}", err);
            }
        }
    }
}

async fn delete_old_uploads() -> Result<u64, AppError> {
    ensure_uploads_dir().await?;

    let mut deleted = 0_u64;
    let mut entries = tokio::fs::read_dir(UPLOADS_PATH).await?;
    let now = SystemTime::now();

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let metadata = match entry.metadata().await {
            Ok(value) => value,
            Err(err) => {
                eprintln!("Upload cleanup: could not read metadata for {:?}: {}", path, err);
                continue;
            }
        };

        if !metadata.is_file() {
            continue;
        }

        let modified = match metadata.modified() {
            Ok(value) => value,
            Err(err) => {
                eprintln!("Upload cleanup: could not read modified time for {:?}: {}", path, err);
                continue;
            }
        };

        let age = match now.duration_since(modified) {
            Ok(value) => value,
            Err(_) => Duration::ZERO,
        };

        if age <= MAX_UPLOAD_AGE {
            continue;
        }

        tokio::fs::remove_file(&path).await?;
        deleted += 1;
    }

    Ok(deleted)
}

fn render_index_with_error(
    state: &AppState,
    current_user: &CurrentUser,
    error_message: &str,
) -> Result<Html<String>, AppError> {
    let template = state
        .jinja
        .get_template("import.html")
        .expect("template is loaded");
    let rendered = template.render(ImportPageView {
        is_admin: current_user.is_admin,
        has_error: true,
        error_message: Some(error_message.to_string()),
    })?;
    Ok(Html(rendered))
}

fn normalize_spreadsheet_extension(file_name: &str) -> Option<String> {
    let extension = Path::new(file_name).extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "xls" | "xlsx" | "xlsm" | "xlsb" | "ods" => Some(extension),
        _ => None,
    }
}

fn upload_path_for(filename: &str) -> PathBuf {
    Path::new(UPLOADS_PATH).join(filename)
}
