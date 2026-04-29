use anyhow::anyhow;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use lettre::{
    Address, AsyncSmtpTransport, AsyncTransport, Tokio1Executor, address::Envelope,
    transport::smtp::authentication::Credentials,
};

use crate::{
    error::AppError,
    settings::{self, AppSettings, TlsMode},
};

pub async fn send_mail(
    target_mail: &str,
    body_content: &str,
    mime_type: &str,
    subject: &str,
) -> Result<(), AppError> {
    let settings = settings::load_settings().await?;
    validate_settings(&settings)?;

    let from_address: Address = settings.sender_email.parse()?;
    let to_address: Address = target_mail.parse()?;
    let envelope = Envelope::new(Some(from_address), vec![to_address])?;

    let from_header = format_mailbox_header(&settings.sender_name, &settings.sender_email);
    let to_header = sanitize_header_value(target_mail);
    let subject_header = format_subject_header(if subject.trim().is_empty() {
        "No Subject"
    } else {
        subject
    });
    let content_type_header = sanitize_header_value(if mime_type.trim().is_empty() {
        "text/plain; charset=utf-8"
    } else {
        mime_type
    });

    let raw_message = format!(
        "From: {from}\r\nTo: {to}\r\nSubject: {subject}\r\nMIME-Version: 1.0\r\nContent-Type: {content_type}\r\n\r\n{body}",
        from = from_header,
        to = to_header,
        subject = subject_header,
        content_type = content_type_header,
        body = body_content
    );

    let mut smtp_builder = build_smtp_transport(&settings)?;
    if !settings.smtp_username.is_empty() {
        smtp_builder = smtp_builder.credentials(Credentials::new(
            settings.smtp_username.clone(),
            settings.smtp_password.clone(),
        ));
    }

    smtp_builder
        .build()
        .send_raw(&envelope, raw_message.as_bytes())
        .await?;
    Ok(())
}

fn validate_settings(settings: &AppSettings) -> Result<(), AppError> {
    if settings.smtp_host.trim().is_empty() {
        return Err(AppError::internal(anyhow!(
            "SMTP host is empty. Save SMTP settings first."
        )));
    }
    if settings.sender_name.trim().is_empty() {
        return Err(AppError::internal(anyhow!(
            "Sender name is empty. Save SMTP settings first."
        )));
    }
    if settings.sender_email.trim().is_empty() || !settings.sender_email.contains('@') {
        return Err(AppError::internal(anyhow!(
            "Sender email is invalid. Save SMTP settings first."
        )));
    }
    Ok(())
}

fn build_smtp_transport(
    settings: &AppSettings,
) -> Result<lettre::transport::smtp::AsyncSmtpTransportBuilder, AppError> {
    let builder = match settings.tls_mode {
        TlsMode::Starttls => {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&settings.smtp_host)?
        }
        TlsMode::Smtps => AsyncSmtpTransport::<Tokio1Executor>::relay(&settings.smtp_host)?,
        TlsMode::None => {
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(settings.smtp_host.clone())
        }
    };
    Ok(builder.port(settings.smtp_port))
}

fn sanitize_header_value(value: &str) -> String {
    value.replace('\r', "").replace('\n', "")
}

fn format_subject_header(subject: &str) -> String {
    let clean_subject = sanitize_header_value(subject);
    if clean_subject.is_ascii() {
        return clean_subject;
    }

    encode_utf8_header_value(&clean_subject)
}

fn encode_utf8_header_value(value: &str) -> String {
    value
        .chars()
        .fold(Vec::<String>::new(), |mut encoded_words, character| {
            let mut bytes = [0; 4];
            let encoded = character.encode_utf8(&mut bytes).as_bytes();

            let should_start_new_word = encoded_words
                .last()
                .map(|current| current.len() + encoded.len() > 45)
                .unwrap_or(true);

            if should_start_new_word {
                encoded_words.push(String::new());
            }
            encoded_words
                .last_mut()
                .expect("encoded word exists")
                .push_str(character.encode_utf8(&mut bytes));
            encoded_words
        })
        .into_iter()
        .map(|chunk| format!("=?UTF-8?B?{}?=", BASE64_STANDARD.encode(chunk.as_bytes())))
        .collect::<Vec<_>>()
        .join("\r\n ")
}

fn format_mailbox_header(name: &str, email: &str) -> String {
    let clean_name = sanitize_header_value(name).trim().to_string();
    if clean_name.is_empty() {
        sanitize_header_value(email)
    } else {
        format!("{} <{}>", clean_name, sanitize_header_value(email))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_subject_is_not_encoded() {
        assert_eq!(
            format_subject_header("Happy Birthday Ada"),
            "Happy Birthday Ada"
        );
    }

    #[test]
    fn non_ascii_subject_is_encoded_as_utf8_base64() {
        assert_eq!(
            format_subject_header("Happy Birthday Jörg"),
            "=?UTF-8?B?SGFwcHkgQmlydGhkYXkgSsO2cmc=?="
        );
    }

    #[test]
    fn subject_is_sanitized_before_encoding() {
        assert_eq!(
            format_subject_header("Grüße\r\nBcc: hidden@example.com"),
            "=?UTF-8?B?R3LDvMOfZUJjYzogaGlkZGVuQGV4YW1wbGUuY29t?="
        );
    }
}
