use std::collections::BTreeMap;
use std::path::Path;

use axum::{
    extract::{Multipart, State},
    http::{HeaderValue, header},
    response::{Html, IntoResponse},
};
use serde::Serialize;

use crate::{AppState, error::AppError, users::CurrentUser};

const TEMPLATE_PATH: &str = "./db/template.eml";
const KNOWN_PLACEHOLDER_NAMES: &[&str] = &["first_name", "last_name", "greeting"];

#[derive(Debug, Serialize)]
struct TemplateView {
    is_admin: bool,
    has_error: bool,
    error_message: Option<String>,
    has_success: bool,
    success_message: Option<String>,
    has_template: bool,
    placeholder_checks: Vec<PlaceholderCheck>,
    unknown_placeholders: Vec<UnknownPlaceholder>,
}

#[derive(Debug, Serialize)]
struct PlaceholderCheck {
    placeholder: String,
    exists: bool,
}

#[derive(Debug, Serialize)]
struct UnknownPlaceholder {
    placeholder: String,
    count: usize,
}

struct PlaceholderInspection {
    checks: Vec<PlaceholderCheck>,
    unknown: Vec<UnknownPlaceholder>,
}

pub async fn index(
    State(state): State<AppState>,
    current_user: CurrentUser,
) -> Result<Html<String>, AppError> {
    let has_template = tokio::fs::try_exists(TEMPLATE_PATH).await?;
    let inspection = if has_template {
        let data = tokio::fs::read(TEMPLATE_PATH).await?;
        inspect_placeholders(&data)
    } else {
        inspect_placeholders(&[])
    };
    render_template_page(
        &state,
        &current_user,
        has_template,
        inspection.checks,
        inspection.unknown,
        None,
        Some("Upload a .eml file to replace the current template."),
    )
}

pub async fn upload(
    State(state): State<AppState>,
    current_user: CurrentUser,
    mut multipart: Multipart,
) -> Result<Html<String>, AppError> {
    let mut uploaded = false;

    while let Some(field) = multipart.next_field().await? {
        if field.name() != Some("template_file") {
            continue;
        }

        let file_name = field.file_name().unwrap_or_default().to_ascii_lowercase();
        if !file_name.ends_with(".eml") {
            let has_template = tokio::fs::try_exists(TEMPLATE_PATH).await?;
            let inspection = load_placeholder_inspection_if_exists(has_template).await?;
            return render_template_page(
                &state,
                &current_user,
                has_template,
                inspection.checks,
                inspection.unknown,
                Some("Only .eml files are allowed."),
                None,
            );
        }

        let data = field.bytes().await?;
        if data.is_empty() {
            let has_template = tokio::fs::try_exists(TEMPLATE_PATH).await?;
            let inspection = load_placeholder_inspection_if_exists(has_template).await?;
            return render_template_page(
                &state,
                &current_user,
                has_template,
                inspection.checks,
                inspection.unknown,
                Some("Uploaded file is empty."),
                None,
            );
        }

        let parent = Path::new(TEMPLATE_PATH)
            .parent()
            .ok_or_else(|| AppError::internal(anyhow::anyhow!("Invalid template path.")))?;
        tokio::fs::create_dir_all(parent).await?;
        tokio::fs::write(TEMPLATE_PATH, data).await?;
        uploaded = true;
        break;
    }

    if !uploaded {
        let has_template = tokio::fs::try_exists(TEMPLATE_PATH).await?;
        let inspection = load_placeholder_inspection_if_exists(has_template).await?;
        return render_template_page(
            &state,
            &current_user,
            has_template,
            inspection.checks,
            inspection.unknown,
            Some("Please choose a .eml file to upload."),
            None,
        );
    }

    let inspection = {
        let data = tokio::fs::read(TEMPLATE_PATH).await?;
        inspect_placeholders(&data)
    };
    render_template_page(
        &state,
        &current_user,
        true,
        inspection.checks,
        inspection.unknown,
        None,
        Some("Template uploaded."),
    )
}

pub async fn download(current_user: CurrentUser) -> Result<impl IntoResponse, AppError> {
    let _ = current_user;

    if !tokio::fs::try_exists(TEMPLATE_PATH).await? {
        return Err(AppError::not_found_for(
            "Template",
            "No template file has been uploaded yet.",
        ));
    }

    let data = tokio::fs::read(TEMPLATE_PATH).await?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("message/rfc822"),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("attachment; filename=\"template.eml\""),
            ),
        ],
        data,
    ))
}

fn render_template_page(
    state: &AppState,
    current_user: &CurrentUser,
    has_template: bool,
    placeholder_checks: Vec<PlaceholderCheck>,
    unknown_placeholders: Vec<UnknownPlaceholder>,
    error_message: Option<&str>,
    success_message: Option<&str>,
) -> Result<Html<String>, AppError> {
    let template = state
        .jinja
        .get_template("template.html")
        .expect("template is loaded");
    let rendered = template.render(TemplateView {
        is_admin: current_user.is_admin,
        has_error: error_message.is_some(),
        error_message: error_message.map(str::to_string),
        has_success: success_message.is_some(),
        success_message: success_message.map(str::to_string),
        has_template,
        placeholder_checks,
        unknown_placeholders,
    })?;
    Ok(Html(rendered))
}

async fn load_placeholder_inspection_if_exists(
    has_template: bool,
) -> Result<PlaceholderInspection, AppError> {
    if !has_template {
        return Ok(inspect_placeholders(&[]));
    }
    let data = tokio::fs::read(TEMPLATE_PATH).await?;
    Ok(inspect_placeholders(&data))
}

fn inspect_placeholders(data: &[u8]) -> PlaceholderInspection {
    let checks = KNOWN_PLACEHOLDER_NAMES
        .iter()
        .map(|name| PlaceholderCheck {
            placeholder: format!("{{{{ {} }}}}", name),
            exists: !locate_placeholders(data, name.as_bytes()).is_empty(),
        })
        .collect();

    let unknown = build_unknown_placeholders(data);
    PlaceholderInspection { checks, unknown }
}

fn build_unknown_placeholders(data: &[u8]) -> Vec<UnknownPlaceholder> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();

    let mut offset = 0;
    while let Some(found) = locate_any_placeholder(data, offset) {
        if !is_known_placeholder_name(&found.name) {
            let name = String::from_utf8_lossy(&found.name).to_string();
            *counts.entry(name).or_insert(0) += 1;
        }
        offset = found.end;
    }

    counts
        .into_iter()
        .map(|(placeholder, count)| UnknownPlaceholder { placeholder, count })
        .collect()
}

fn is_known_placeholder_name(name: &[u8]) -> bool {
    KNOWN_PLACEHOLDER_NAMES
        .iter()
        .any(|candidate| name == candidate.as_bytes())
}

fn locate_placeholders(content: &[u8], name: &[u8]) -> Vec<(usize, usize)> {
    if name.is_empty() || content.len() < 4 {
        return Vec::new();
    }

    let mut matches: Vec<(usize, usize)> = Vec::new();
    let mut offset = 0;
    while let Some(found) = locate_any_placeholder(content, offset) {
        if found.name.as_slice() == name {
            matches.push((found.start, found.end));
        }
        offset = found.end;
    }

    matches
}

struct PlaceholderSpan {
    name: Vec<u8>,
    start: usize,
    end: usize,
}

fn locate_any_placeholder(content: &[u8], from: usize) -> Option<PlaceholderSpan> {
    if content.len() < 4 || from >= content.len() {
        return None;
    }

    let mut i = from;
    while i + 1 < content.len() {
        if content[i] != b'{' || content[i + 1] != b'{' {
            i += 1;
            continue;
        }

        let mut j = i + 2;
        while j < content.len() && content[j].is_ascii_whitespace() {
            j += 1;
        }

        let name_start = j;
        while j < content.len()
            && !content[j].is_ascii_whitespace()
            && content[j] != b'}'
            && content[j] != b'{'
        {
            j += 1;
        }
        let name_end = j;

        if name_start == name_end {
            i += 1;
            continue;
        }

        while j < content.len() && content[j].is_ascii_whitespace() {
            j += 1;
        }

        if j + 1 < content.len() && content[j] == b'}' && content[j + 1] == b'}' {
            return Some(PlaceholderSpan {
                name: content[name_start..name_end].to_vec(),
                start: i,
                end: j + 2,
            });
        }

        i += 1;
    }

    None
}
