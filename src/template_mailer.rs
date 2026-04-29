use anyhow::anyhow;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};

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
    println!("subject: {}", subject);

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
            subject = decode_utf8_encoded_words(&value);
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

fn decode_utf8_encoded_words(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    let mut last_was_encoded = false;

    while let Some(relative_start) = input[cursor..].find("=?") {
        let start = cursor + relative_start;
        let between = &input[cursor..start];

        if !(last_was_encoded && between.chars().all(char::is_whitespace)) {
            output.push_str(between);
        }

        if let Some((decoded, end)) = decode_utf8_encoded_word(&input[start..]) {
            output.push_str(&decoded);
            cursor = start + end;
            last_was_encoded = true;
        } else {
            output.push_str("=?");
            cursor = start + 2;
            last_was_encoded = false;
        }
    }

    output.push_str(&input[cursor..]);
    output
}

fn decode_utf8_encoded_word(input: &str) -> Option<(String, usize)> {
    if !input.starts_with("=?") {
        return None;
    }

    let charset_end = input[2..].find('?')? + 2;
    let charset = &input[2..charset_end];
    if !charset.eq_ignore_ascii_case("utf-8") && !charset.eq_ignore_ascii_case("utf8") {
        return None;
    }

    let encoding_start = charset_end + 1;
    let encoding_end = input[encoding_start..].find('?')? + encoding_start;
    let encoding = &input[encoding_start..encoding_end];
    let encoded_start = encoding_end + 1;
    let encoded_end = input[encoded_start..].find("?=")? + encoded_start;
    let encoded = &input[encoded_start..encoded_end];

    let bytes = if encoding.eq_ignore_ascii_case("b") {
        BASE64_STANDARD.decode(encoded).ok()?
    } else if encoding.eq_ignore_ascii_case("q") {
        decode_q_encoded_word(encoded)?
    } else {
        return None;
    };

    let decoded = String::from_utf8(bytes).ok()?;
    Some((decoded, encoded_end + 2))
}

fn decode_q_encoded_word(input: &str) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut cursor = 0;

    while cursor < bytes.len() {
        match bytes[cursor] {
            b'_' => {
                output.push(b' ');
                cursor += 1;
            }
            b'=' => {
                if cursor + 2 >= bytes.len() {
                    return None;
                }
                let high = hex_value(bytes[cursor + 1])?;
                let low = hex_value(bytes[cursor + 2])?;
                output.push((high << 4) | low);
                cursor += 3;
            }
            byte => {
                output.push(byte);
                cursor += 1;
            }
        }
    }

    Some(output)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn replace_placeholders(input: &str, values: &TemplateValues<'_>) -> String {
    let mut output = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut cursor = 0;

    while let Some(found) = placeholders::locate_any_placeholder(bytes, cursor) {
        println!(
            "Found placeholder: \"{}\"",
            String::from_utf8_lossy(&found.name)
        );
        if found.start > cursor {
            output.push_str(&input[cursor..found.start]);
        }

        let replacement = match found.name.trim_ascii() {
            b"greeting" => Some(values.greeting),
            b"last_name" => Some(values.last_name),
            b"first_name" => Some(values.first_name),
            _ => None,
        };

        println!("setting replacement to: {:?}", replacement);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_template_decodes_base64_utf8_subject_before_placeholder_replacement() {
        let parsed = parse_template(
            b"Subject: =?UTF-8?B?SGFwcHkgQmlydGhkYXkge3sgZmlyc3RfbmFtZSB9fQ==?=\r\n\r\nBody",
        )
        .unwrap();

        assert_eq!(parsed.subject, "Happy Birthday {{ first_name }}");

        let values = TemplateValues {
            greeting: "Dear Ms.",
            last_name: "Example",
            first_name: "Ada",
        };
        assert_eq!(
            replace_placeholders(&parsed.subject, &values),
            "Happy Birthday Ada"
        );
    }

    #[test]
    fn parse_template_decodes_q_encoded_utf8_subject() {
        let parsed = parse_template(
            b"Subject: =?UTF-8?Q?Happy_Birthday_=E2=98=BA_{{_first=5Fname_}}?=\n\nBody",
        )
        .unwrap();

        assert_eq!(parsed.subject, "Happy Birthday \u{263a} {{ first_name }}");
    }

    #[test]
    fn parse_template_joins_adjacent_encoded_subject_words() {
        let parsed =
            parse_template(b"Subject: =?UTF-8?B?SGFwcHkg?= =?UTF-8?B?QmlydGhkYXk=?=\n\nBody")
                .unwrap();

        assert_eq!(parsed.subject, "Happy Birthday");
    }
}
