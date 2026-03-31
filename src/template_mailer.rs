use anyhow::anyhow;

use crate::{error::AppError, placeholders, send_mail};

pub struct TemplateValues<'a> {
    pub greeting: &'a str,
    pub last_name: &'a str,
    pub first_name: &'a str,
}

async fn send_template_mail(
    template_bytes: &[u8],
    recipient_email: &str,
    values: &TemplateValues<'_>,
) -> Result<(), AppError> {
    let parsed = parse_template(template_bytes)?;

    let subject = replace_placeholders(&parsed.subject, values);
    let body = replace_placeholders(&parsed.body, values);

    let subject = if subject.trim().is_empty() {
        "Template Test Email".to_string()
    } else {
        subject
    };
    let mime_type = if parsed.content_type.trim().is_empty() {
        "text/plain; charset=utf-8".to_string()
    } else {
        parsed.content_type
    };

    send_mail::send_mail(recipient_email, &body, &mime_type, &subject).await
}

pub async fn send_template_mail_with_loaded_settings(
    template_bytes: &[u8],
    recipient_email: &str,
    values: &TemplateValues<'_>,
) -> Result<(), AppError> {
    send_template_mail(template_bytes, recipient_email, values).await
}

pub async fn send_template_test_mail_with_loaded_settings(
    template_bytes: &[u8],
    recipient_email: &str,
) -> Result<(), AppError> {
    let values = TemplateValues {
        greeting: "Dear Mr.",
        last_name: "Doe",
        first_name: "John",
    };
    send_template_mail(template_bytes, recipient_email, &values).await
}

struct ParsedTemplate {
    subject: String,
    content_type: String,
    body: String,
}

fn parse_template(template_bytes: &[u8]) -> Result<ParsedTemplate, AppError> {
    let raw = String::from_utf8(template_bytes.to_vec())?;
    let (headers_raw, body_raw) = split_headers_and_body(&raw)
        .ok_or_else(|| AppError::internal(anyhow!("Template has no header/body separator.")))?;

    let mut subject = String::new();
    let mut content_type = "text/plain".to_string();

    for (name, value) in parse_headers(headers_raw) {
        if name.eq_ignore_ascii_case("subject") {
            subject = value;
        } else if name.eq_ignore_ascii_case("content-type") {
            content_type = value;
        }
    }

    Ok(ParsedTemplate {
        subject,
        content_type,
        body: body_raw.to_string(),
    })
}

fn split_headers_and_body(raw: &str) -> Option<(&str, &str)> {
    if let Some(pos) = raw.find("\r\n\r\n") {
        return Some((&raw[..pos], &raw[pos + 4..]));
    }
    if let Some(pos) = raw.find("\n\n") {
        return Some((&raw[..pos], &raw[pos + 2..]));
    }
    None
}

fn parse_headers(headers_raw: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut current_name = String::new();
    let mut current_value = String::new();

    for line in headers_raw.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if !current_name.is_empty() {
                if !current_value.is_empty() {
                    current_value.push(' ');
                }
                current_value.push_str(line.trim());
            }
            continue;
        }

        if !current_name.is_empty() {
            out.push((current_name.clone(), current_value.trim().to_string()));
            current_name.clear();
            current_value.clear();
        }

        if let Some((name, value)) = line.split_once(':') {
            current_name = name.trim().to_string();
            current_value = value.trim().to_string();
        }
    }

    if !current_name.is_empty() {
        out.push((current_name, current_value.trim().to_string()));
    }

    out
}

fn replace_placeholders(input: &str, values: &TemplateValues<'_>) -> String {
    let mut output = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut cursor = 0;

    while let Some(found) = placeholders::locate_any_placeholder(bytes, cursor) {
        if found.start > cursor {
            output.push_str(&input[cursor..found.start]);
        }

        let replacement = match found.name.as_slice() {
            b"greeting" => Some(values.greeting),
            b"last_name" => Some(values.last_name),
            b"first_name" => Some(values.first_name),
            _ => None,
        };

        if let Some(value) = replacement {
            output.push_str(value);
        } else {
            output.push_str(&input[found.start..found.end]);
        }

        cursor = found.end;
    }

    if cursor < input.len() {
        output.push_str(&input[cursor..]);
    }

    output
}
