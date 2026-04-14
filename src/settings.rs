use std::path::Path;

use axum::{extract::State, response::Html};
use axum_extra::extract::Form;
use chrono::{Datelike, Local, NaiveTime};
use serde::{Deserialize, Serialize};

use crate::{AppState, error::AppError, send_mail, users::CurrentUser};

const SETTINGS_PATH: &str = "./db/settings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_username: String,
    pub smtp_password: String,
    pub sender_name: String,
    pub sender_email: String,
    pub send_for_years: i64,
    pub disable_scheduled_mails: bool,
    pub schedule_at_local_time: String,
    pub tls_mode: TlsMode,
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
            send_for_years: 2,
            disable_scheduled_mails: false,
            schedule_at_local_time: "09:00".to_string(),
            tls_mode: TlsMode::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum TlsMode {
    #[default]
    Starttls,
    Smtps,
    None,
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
    disable_scheduled_mails: Option<String>,
    schedule_at_local_time: String,
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
    settings: AppSettings,
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
    let schedule_at_local_time = match parse_schedule_at_local_time(&form.schedule_at_local_time) {
        Some(value) => value,
        None => {
            return render_settings_from_form(
                &state,
                &current_user,
                form,
                Some("Schedule at local time must use HH:MM in 24-hour format."),
                None,
            );
        }
    };
    let tls_mode = match parse_tls_mode(&form.tls_mode) {
        Some(mode) => mode,
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
        disable_scheduled_mails: form.disable_scheduled_mails.is_some(),
        schedule_at_local_time,
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

pub async fn delete_expired_people(
    State(state): State<AppState>,
    current_user: CurrentUser,
) -> Result<Html<String>, AppError> {
    let settings = load_settings().await?;
    let deleted_count = prune_people_outside_send_window(&state, settings.send_for_years).await?;

    render_settings(
        &state,
        &current_user,
        settings,
        None,
        Some(&format!(
            "Deleted {} people outside the send-for-years timeframe.",
            deleted_count
        )),
        String::new(),
    )
}

pub async fn ensure_settings_file() -> Result<(), AppError> {
    if tokio::fs::try_exists(SETTINGS_PATH).await? {
        return Ok(());
    }

    save_settings(&AppSettings {
        ..AppSettings::default()
    })
    .await
}

pub(crate) async fn load_settings() -> Result<AppSettings, AppError> {
    let contents = tokio::fs::read_to_string(SETTINGS_PATH).await?;
    Ok(serde_json::from_str(&contents)?)
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

async fn prune_people_outside_send_window(
    state: &AppState,
    send_for_years: i64,
) -> Result<u64, AppError> {
    let current_year = i64::from(Local::now().year());
    let mut tx = state.db.begin().await?;

    sqlx::query!(
        r#"
        DELETE FROM sent
        WHERE user_id IN (
            SELECT id
            FROM people
            WHERE ? >= start_year + ?
        )
        "#,
        current_year,
        send_for_years
    )
    .execute(&mut *tx)
    .await?;

    let result = sqlx::query!(
        r#"
        DELETE FROM people
        WHERE ? >= start_year + ?
        "#,
        current_year,
        send_for_years
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(result.rows_affected())
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
        send_for_years: form.send_for_years.trim().parse::<i64>().unwrap_or(2),
        disable_scheduled_mails: form.disable_scheduled_mails.is_some(),
        schedule_at_local_time: parse_schedule_at_local_time(&form.schedule_at_local_time)
            .unwrap_or_else(|| "09:00".to_string()),
        tls_mode: parse_tls_mode(&form.tls_mode).unwrap_or_default(),
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
        settings,
        test_recipient_email,
    })?;
    Ok(Html(rendered))
}

fn parse_schedule_at_local_time(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let parsed = NaiveTime::parse_from_str(trimmed, "%H:%M").ok()?;
    Some(parsed.format("%H:%M").to_string())
}

fn parse_tls_mode(tls_mode: &str) -> Option<TlsMode> {
    match tls_mode.trim().to_ascii_lowercase().as_str() {
        "starttls" => Some(TlsMode::Starttls),
        "smtps" => Some(TlsMode::Smtps),
        "none" => Some(TlsMode::None),
        _ => None,
    }
}
