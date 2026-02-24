use std::collections::BTreeMap;
use std::path::Path;

use axum::{
    extract::{Multipart, State},
    http::{HeaderValue, header},
    response::{Html, IntoResponse},
};
use axum_extra::extract::Form;
use serde::Deserialize;
use serde::Serialize;

use crate::{AppState, error::AppError, placeholders, template_mailer, users::CurrentUser};

const TEMPLATE_PATH: &str = "./db/template.eml";
const KNOWN_PLACEHOLDER_NAMES: &[&str] = &["first_name", "last_name", "greeting"];
const ALLOWED_TEMPLATE_HEADERS: &[&str] = &["subject", "content-type"];

#[derive(Debug, Serialize)]
struct TemplateView {
    is_admin: bool,
    has_error: bool,
    error_message: Option<String>,
    has_success: bool,
    success_message: Option<String>,
    has_template: bool,
    test_recipient_email: String,
    placeholder_checks: Vec<PlaceholderCheck>,
    unknown_placeholders: Vec<UnknownPlaceholder>,
}

#[derive(Debug, Deserialize)]
pub struct TemplateTestMailForm {
    test_recipient_email: String,
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
        String::new(),
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
                String::new(),
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
                String::new(),
            );
        }

        let parent = Path::new(TEMPLATE_PATH)
            .parent()
            .ok_or_else(|| AppError::internal(anyhow::anyhow!("Invalid template path.")))?;
        tokio::fs::create_dir_all(parent).await?;
        let sanitized = sanitize_eml_headers(&data);
        tokio::fs::write(TEMPLATE_PATH, sanitized).await?;
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
            String::new(),
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
        String::new(),
    )
}

pub async fn send_test_mail(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Form(form): Form<TemplateTestMailForm>,
) -> Result<Html<String>, AppError> {
    let recipient_email = form.test_recipient_email.trim().to_string();
    let has_template = tokio::fs::try_exists(TEMPLATE_PATH).await?;
    let inspection = load_placeholder_inspection_if_exists(has_template).await?;
    let send_result = try_send_template_test_mail(has_template, &recipient_email).await;

    match send_result {
        Ok(()) => render_template_page(
            &state,
            &current_user,
            has_template,
            inspection.checks,
            inspection.unknown,
            None,
            Some("Template test email sent."),
            recipient_email,
        ),
        Err(error_message) => render_template_page(
            &state,
            &current_user,
            has_template,
            inspection.checks,
            inspection.unknown,
            Some(&error_message),
            None,
            recipient_email,
        ),
    }
}

async fn try_send_template_test_mail(
    has_template: bool,
    recipient_email: &str,
) -> Result<(), String> {
    if !has_template {
        return Err("Upload a template before sending a template test email.".to_string());
    }

    if recipient_email.is_empty() || !recipient_email.contains('@') {
        return Err("Test recipient must be a valid email address.".to_string());
    }

    let template_bytes = tokio::fs::read(TEMPLATE_PATH)
        .await
        .map_err(|err| format!("Could not read template file: {}", err))?;

    template_mailer::send_template_test_mail_with_loaded_settings(&template_bytes, recipient_email)
        .await
        .map_err(|err| {
            format!(
                "Template test email could not be sent. Check template headers/body and SMTP settings. Server response: {}",
                err
            )
        })
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
    test_recipient_email: String,
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
        test_recipient_email,
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
    while let Some(found) = placeholders::locate_any_placeholder(data, offset) {
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
    placeholders::locate_placeholders(content, name)
}

fn sanitize_eml_headers(raw: &[u8]) -> Vec<u8> {
    let (header_end, newline) = match find_header_end(raw) {
        Some(value) => value,
        None => return raw.to_vec(),
    };

    let header_bytes = &raw[..header_end];
    let body_start = header_end + newline.len() * 2;
    let body_bytes = if body_start <= raw.len() {
        &raw[body_start..]
    } else {
        &[]
    };

    let mut kept_lines: Vec<Vec<u8>> = Vec::new();
    let mut keep_current_header = false;

    for line in iter_lines(header_bytes, newline) {
        if line.is_empty() {
            continue;
        }

        let is_continuation = matches!(line.first(), Some(b' ' | b'\t'));
        if is_continuation {
            if keep_current_header {
                kept_lines.push(line.to_vec());
            }
            continue;
        }

        keep_current_header = header_name(line)
            .map(|name| {
                let lower = name.to_ascii_lowercase();
                ALLOWED_TEMPLATE_HEADERS
                    .iter()
                    .any(|allowed| lower == allowed.as_bytes())
            })
            .unwrap_or(false);

        if keep_current_header {
            kept_lines.push(line.to_vec());
        }
    }

    let mut out = Vec::new();
    for line in kept_lines {
        out.extend_from_slice(&line);
        out.extend_from_slice(newline);
    }
    out.extend_from_slice(newline);
    out.extend_from_slice(newline);
    out.extend_from_slice(body_bytes);
    out
}

fn find_header_end(raw: &[u8]) -> Option<(usize, &'static [u8])> {
    if let Some(pos) = find_subslice(raw, b"\r\n\r\n") {
        return Some((pos, b"\r\n"));
    }
    if let Some(pos) = find_subslice(raw, b"\n\n") {
        return Some((pos, b"\n"));
    }
    None
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn iter_lines<'a>(data: &'a [u8], newline: &[u8]) -> Vec<&'a [u8]> {
    let mut lines = Vec::new();
    let mut pos = 0;

    while pos < data.len() {
        if let Some(rel_end) = find_subslice(&data[pos..], newline) {
            let end = pos + rel_end;
            lines.push(&data[pos..end]);
            pos = end + newline.len();
        } else {
            lines.push(&data[pos..]);
            break;
        }
    }

    lines
}

fn header_name(line: &[u8]) -> Option<&[u8]> {
    let colon = line.iter().position(|b| *b == b':')?;
    Some(trim_ascii_bytes(&line[..colon]))
}

fn trim_ascii_bytes(bytes: &[u8]) -> &[u8] {
    let mut start = 0;
    let mut end = bytes.len();

    while start < end && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }

    &bytes[start..end]
}
