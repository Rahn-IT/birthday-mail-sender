use std::path::Path;

use axum::{
    extract::State,
    response::Html,
};
use axum_extra::extract::Form;
use serde::{Deserialize, Serialize};

use crate::{AppState, error::AppError, send_mail, users::CurrentUser};

const SETTINGS_PATH: &str = "./db/settings.json";
const TLS_MODE_STARTTLS: &str = "starttls";
const TLS_MODE_SMTPS: &str = "smtps";
const TLS_MODE_NONE: &str = "none";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_username: String,
    pub smtp_password: String,
    pub sender_name: String,
    pub sender_email: String,
    #[serde(default = "default_send_for_years")]
    pub send_for_years: i64,
    #[serde(default = "default_tls_mode")]
    pub tls_mode: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            smtp_host: String::new(),
            smtp_port: 587,
            smtp_username: String::new(),
            smtp_password: String::new(),
            sender_name: String::new(),
            sender_email: String::new(),
            send_for_years: default_send_for_years(),
            tls_mode: default_tls_mode(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SettingsForm {
    smtp_host: String,
    smtp_port: String,
    smtp_username: String,
    smtp_password: String,
    sender_name: String,
    sender_email: String,
    send_for_years: String,
    tls_mode: String,
}

#[derive(Debug, Deserialize)]
pub struct TestMailForm {
    test_recipient_email: String,
}

#[derive(Debug, Serialize)]
struct SettingsView {
    is_admin: bool,
    has_error: bool,
    error_message: Option<String>,
    has_success: bool,
    success_message: Option<String>,
    smtp_host: String,
    smtp_port: u16,
    smtp_username: String,
    smtp_password: String,
    sender_name: String,
    sender_email: String,
    send_for_years: i64,
    tls_mode: String,
    test_recipient_email: String,
}

pub async fn index(
    State(state): State<AppState>,
    current_user: CurrentUser,
) -> Result<Html<String>, AppError> {
    let settings = load_settings().await?;
    render_settings(
        &state,
        &current_user,
        settings,
        None,
        Some("Settings loaded."),
        String::new(),
    )
}

pub async fn save(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Form(form): Form<SettingsForm>,
) -> Result<Html<String>, AppError> {
    let smtp_host = form.smtp_host.trim().to_string();
    let smtp_username = form.smtp_username.trim().to_string();
    let smtp_password = form.smtp_password.trim().to_string();
    let sender_name = form.sender_name.trim().to_string();
    let sender_email = form.sender_email.trim().to_string();
    let send_for_years = match form.send_for_years.trim().parse::<i64>() {
        Ok(value) if value >= 0 => value,
        _ => {
            return render_settings_from_form(
                &state,
                &current_user,
                form,
                Some("Send for years must be a valid number greater than or equal to 0."),
                None,
            );
        }
    };
    let tls_mode = match normalize_tls_mode(&form.tls_mode) {
        Some(mode) => mode.to_string(),
        None => {
            return render_settings_from_form(
                &state,
                &current_user,
                form,
                Some("TLS mode must be one of: STARTTLS, SMTPS, None."),
                None,
            );
        }
    };

    if smtp_host.is_empty() {
        return render_settings_from_form(
            &state,
            &current_user,
            form,
            Some("SMTP host cannot be empty."),
            None,
        );
    }

    let smtp_port = match form.smtp_port.trim().parse::<u16>() {
        Ok(port) if port > 0 => port,
        _ => {
            return render_settings_from_form(
                &state,
                &current_user,
                form,
                Some("SMTP port must be a valid number from 1 to 65535."),
                None,
            );
        }
    };

    if sender_name.is_empty() {
        return render_settings_from_form(
            &state,
            &current_user,
            form,
            Some("Sender name cannot be empty."),
            None,
        );
    }

    if sender_email.is_empty() || !sender_email.contains('@') {
        return render_settings_from_form(
            &state,
            &current_user,
            form,
            Some("Sender email must be a valid email address."),
            None,
        );
    }

    let settings = AppSettings {
        smtp_host,
        smtp_port,
        smtp_username,
        smtp_password,
        sender_name,
        sender_email,
        send_for_years,
        tls_mode,
    };

    save_settings(&settings).await?;
    render_settings(
        &state,
        &current_user,
        settings,
        None,
        Some("Settings saved."),
        String::new(),
    )
}

pub async fn send_test_mail(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Form(form): Form<TestMailForm>,
) -> Result<Html<String>, AppError> {
    let settings = load_settings().await?;
    let test_recipient_email = form.test_recipient_email.trim().to_string();
    let result: Result<(), String> = if settings.smtp_host.trim().is_empty() {
        Err("Save SMTP settings first.".to_string())
    } else if settings.sender_name.trim().is_empty() {
        Err("Sender name cannot be empty.".to_string())
    } else if settings.sender_email.trim().is_empty() || !settings.sender_email.contains('@') {
        Err("Sender email must be a valid email address.".to_string())
    } else if test_recipient_email.is_empty() || !test_recipient_email.contains('@') {
        Err("Test recipient must be a valid email address.".to_string())
    } else {
        send_mail::send_mail(
            &test_recipient_email,
            "This is a test email from birthday-mail-sender.",
            "text/plain; charset=utf-8",
            "SMTP Test Email",
        )
        .await
        .map_err(|err| {
            format!(
                "Test email could not be sent. Check SMTP host/port, TLS mode, username and password. Server response: {}",
                err
            )
        })
    };

    match result {
        Ok(()) => render_settings(
            &state,
            &current_user,
            settings,
            None,
            Some("Test email sent."),
            test_recipient_email,
        ),
        Err(error_message) => render_settings(
            &state,
            &current_user,
            settings,
            Some(&error_message),
            None,
            test_recipient_email,
        ),
    }
}

pub async fn ensure_settings_file() -> Result<(), AppError> {
    if tokio::fs::try_exists(SETTINGS_PATH).await? {
        return Ok(());
    }

    save_settings(&AppSettings::default()).await
}

pub(crate) async fn load_settings() -> Result<AppSettings, AppError> {
    if !tokio::fs::try_exists(SETTINGS_PATH).await? {
        return Ok(AppSettings::default());
    }

    let contents = tokio::fs::read_to_string(SETTINGS_PATH).await?;
    if contents.trim().is_empty() {
        return Ok(AppSettings::default());
    }

    let settings: AppSettings = serde_json::from_str(&contents)?;
    Ok(settings)
}

async fn save_settings(settings: &AppSettings) -> Result<(), AppError> {
    let parent = Path::new(SETTINGS_PATH)
        .parent()
        .ok_or_else(|| AppError::internal(anyhow::anyhow!("Invalid settings path.")))?;
    tokio::fs::create_dir_all(parent).await?;
    let json = serde_json::to_string_pretty(settings)?;
    tokio::fs::write(SETTINGS_PATH, format!("{}\n", json)).await?;
    Ok(())
}

fn render_settings_from_form(
    state: &AppState,
    current_user: &CurrentUser,
    form: SettingsForm,
    error_message: Option<&str>,
    success_message: Option<&str>,
) -> Result<Html<String>, AppError> {
    let smtp_port = form.smtp_port.trim().parse::<u16>().unwrap_or(587);
    let settings = AppSettings {
        smtp_host: form.smtp_host,
        smtp_port,
        smtp_username: form.smtp_username,
        smtp_password: form.smtp_password,
        sender_name: form.sender_name,
        sender_email: form.sender_email,
        send_for_years: form.send_for_years.trim().parse::<i64>().unwrap_or(default_send_for_years()),
        tls_mode: form.tls_mode,
    };
    render_settings(
        state,
        current_user,
        settings,
        error_message,
        success_message,
        String::new(),
    )
}

fn render_settings(
    state: &AppState,
    current_user: &CurrentUser,
    settings: AppSettings,
    error_message: Option<&str>,
    success_message: Option<&str>,
    test_recipient_email: String,
) -> Result<Html<String>, AppError> {
    let template = state
        .jinja
        .get_template("settings.html")
        .expect("template is loaded");
    let rendered = template.render(SettingsView {
        is_admin: current_user.is_admin,
        has_error: error_message.is_some(),
        error_message: error_message.map(str::to_string),
        has_success: success_message.is_some(),
        success_message: success_message.map(str::to_string),
        smtp_host: settings.smtp_host,
        smtp_port: settings.smtp_port,
        smtp_username: settings.smtp_username,
        smtp_password: settings.smtp_password,
        sender_name: settings.sender_name,
        sender_email: settings.sender_email,
        send_for_years: settings.send_for_years,
        tls_mode: normalize_tls_mode(&settings.tls_mode)
            .unwrap_or(TLS_MODE_STARTTLS)
            .to_string(),
        test_recipient_email,
    })?;
    Ok(Html(rendered))
}

fn default_tls_mode() -> String {
    TLS_MODE_STARTTLS.to_string()
}

fn default_send_for_years() -> i64 {
    1
}

fn normalize_tls_mode(tls_mode: &str) -> Option<&'static str> {
    match tls_mode.trim().to_ascii_lowercase().as_str() {
        TLS_MODE_STARTTLS => Some(TLS_MODE_STARTTLS),
        TLS_MODE_SMTPS => Some(TLS_MODE_SMTPS),
        TLS_MODE_NONE => Some(TLS_MODE_NONE),
        _ => None,
    }
}
