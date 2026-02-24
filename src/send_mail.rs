use anyhow::anyhow;
use lettre::{
    Address, AsyncSmtpTransport, AsyncTransport, Tokio1Executor, address::Envelope,
    transport::smtp::authentication::Credentials,
};

use crate::{
    error::AppError,
    settings::{self, AppSettings},
};

const TLS_MODE_STARTTLS: &str = "starttls";
const TLS_MODE_SMTPS: &str = "smtps";
const TLS_MODE_NONE: &str = "none";

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
    let subject_header = sanitize_header_value(if subject.trim().is_empty() {
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
    let tls_mode = normalize_tls_mode(&settings.tls_mode).unwrap_or(TLS_MODE_STARTTLS);
    let builder = match tls_mode {
        TLS_MODE_STARTTLS => {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&settings.smtp_host)?
        }
        TLS_MODE_SMTPS => AsyncSmtpTransport::<Tokio1Executor>::relay(&settings.smtp_host)?,
        TLS_MODE_NONE => {
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(settings.smtp_host.clone())
        }
        _ => unreachable!("unsupported TLS mode"),
    };
    Ok(builder.port(settings.smtp_port))
}

fn normalize_tls_mode(tls_mode: &str) -> Option<&'static str> {
    match tls_mode.trim().to_ascii_lowercase().as_str() {
        TLS_MODE_STARTTLS => Some(TLS_MODE_STARTTLS),
        TLS_MODE_SMTPS => Some(TLS_MODE_SMTPS),
        TLS_MODE_NONE => Some(TLS_MODE_NONE),
        _ => None,
    }
}

fn sanitize_header_value(value: &str) -> String {
    value.replace('\r', "").replace('\n', "")
}

fn format_mailbox_header(name: &str, email: &str) -> String {
    let clean_name = sanitize_header_value(name).trim().to_string();
    if clean_name.is_empty() {
        sanitize_header_value(email)
    } else {
        format!("{} <{}>", clean_name, sanitize_header_value(email))
    }
}
