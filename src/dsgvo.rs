use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, header},
    response::{Html, IntoResponse, Response},
};
use axum_extra::extract::Form;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::{AppState, error::AppError, settings, users::CurrentUser};

#[derive(Debug, Deserialize)]
pub struct DeleteByEmailForm {
    email: String,
}

#[derive(Debug, Deserialize)]
pub struct CheckMailForm {
    email: String,
}

#[derive(Debug, Serialize)]
struct DsgvoView {
    is_admin: bool,
    has_error: bool,
    error_message: Option<String>,
    has_success: bool,
    success_message: Option<String>,
    email: String,
    report: Option<String>,
}

pub async fn index(
    State(state): State<AppState>,
    current_user: CurrentUser,
) -> Result<Html<String>, AppError> {
    render_page(
        &state,
        &current_user,
        String::new(),
        None,
        None,
        None,
    )
}

pub async fn delete_by_email(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Form(form): Form<DeleteByEmailForm>,
) -> Result<Html<String>, AppError> {
    let email = form.email.trim().to_string();
    if email.is_empty() || !email.contains('@') {
        return render_page(
            &state,
            &current_user,
            email,
            None,
            Some("Please enter a valid email address."),
            None,
        );
    }

    let deleted_count = delete_people_by_email(&state.db, &email).await?;
    render_page(
        &state,
        &current_user,
        email,
        None,
        None,
        Some(&format!(
            "Deleted {} record(s) for the submitted email address.",
            deleted_count
        )),
    )
}

pub async fn check_mail(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Form(form): Form<CheckMailForm>,
) -> Result<Html<String>, AppError> {
    let email = form.email.trim().to_string();
    if email.is_empty() || !email.contains('@') {
        return render_page(
            &state,
            &current_user,
            email,
            None,
            Some("Please enter a valid email address."),
            None,
        );
    }

    let mut report = generate_report(&state.db, &email).await?;
    if mail_is_blocked(&state.db, &email).await? {
        report.push_str("\n\nDSGVO-Block active");
    }
    render_page(
        &state,
        &current_user,
        email,
        Some(report),
        None,
        None,
    )
}

pub async fn download_report(
    State(state): State<AppState>,
    Form(form): Form<CheckMailForm>,
) -> Result<Response, AppError> {
    let email = form.email.trim().to_string();
    if email.is_empty() || !email.contains('@') {
        return Err(AppError::conflict("Please enter a valid email address."));
    }

    let report = generate_report(&state.db, &email).await?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"dsgvo-report-{}.txt\"",
            sanitize_filename(&email)
        ))
        .map_err(AppError::internal)?,
    );

    Ok((headers, report).into_response())
}

pub async fn block_mail_post(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Form(form): Form<CheckMailForm>,
) -> Result<Html<String>, AppError> {
    let email = form.email.trim().to_string();
    if email.is_empty() || !email.contains('@') {
        return render_page(
            &state,
            &current_user,
            email,
            None,
            Some("Please enter a valid email address."),
            None,
        );
    }

    block_mail(&state.db, &email).await?;
    render_page(
        &state,
        &current_user,
        email,
        None,
        None,
        Some("Email has been blocked and matching data has been deleted."),
    )
}

pub async fn unblock_mail_post(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Form(form): Form<CheckMailForm>,
) -> Result<Html<String>, AppError> {
    let email = form.email.trim().to_string();
    if email.is_empty() || !email.contains('@') {
        return render_page(
            &state,
            &current_user,
            email,
            None,
            Some("Please enter a valid email address."),
            None,
        );
    }

    let removed = unblock_mail(&state.db, &email).await?;
    render_page(
        &state,
        &current_user,
        email,
        None,
        None,
        Some(if removed {
            "Email has been unblocked."
        } else {
            "Email was not blocked."
        }),
    )
}

fn render_page(
    state: &AppState,
    current_user: &CurrentUser,
    email: String,
    report: Option<String>,
    error_message: Option<&str>,
    success_message: Option<&str>,
) -> Result<Html<String>, AppError> {
    let template = state
        .jinja
        .get_template("dsgvo.html")
        .expect("template is loaded");
    let rendered = template.render(DsgvoView {
        is_admin: current_user.is_admin,
        has_error: error_message.is_some(),
        error_message: error_message.map(str::to_string),
        has_success: success_message.is_some(),
        success_message: success_message.map(str::to_string),
        email,
        report,
    })?;
    Ok(Html(rendered))
}

async fn delete_people_by_email(db: &SqlitePool, email: &str) -> Result<u64, AppError> {
    let normalized_email = email.trim();
    let mut tx = db.begin().await?;

    sqlx::query!(
        r#"
        DELETE FROM sent
        WHERE user_id IN (
            SELECT id
            FROM people
            WHERE LOWER(email) = LOWER(?)
        )
        "#,
        normalized_email
    )
    .execute(&mut *tx)
    .await?;

    let result = sqlx::query!(
        r#"
        DELETE FROM people
        WHERE LOWER(email) = LOWER(?)
        "#,
        normalized_email
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(result.rows_affected())
}

pub async fn mail_is_blocked(db: &SqlitePool, email: &str) -> Result<bool, AppError> {
    let sha_hash = peppered_mail_hash(email).await?;
    let count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) as "count!: i64"
        FROM blocked
        WHERE sha_hash = ?
        "#
    )
    .bind(sha_hash)
    .fetch_one(db)
    .await?;

    Ok(count > 0)
}

pub async fn block_mail(db: &SqlitePool, email: &str) -> Result<(), AppError> {
    let normalized_email = email.trim();
    let sha_hash = peppered_mail_hash(normalized_email).await?;

    delete_people_by_email(db, normalized_email).await?;

    sqlx::query(
        r#"
        INSERT INTO blocked (sha_hash)
        VALUES (?)
        ON CONFLICT(sha_hash) DO NOTHING
        "#
    )
    .bind(sha_hash)
    .execute(db)
    .await?;

    Ok(())
}

pub async fn unblock_mail(db: &SqlitePool, email: &str) -> Result<bool, AppError> {
    let sha_hash = peppered_mail_hash(email).await?;
    let result = sqlx::query(
        r#"
        DELETE FROM blocked
        WHERE sha_hash = ?
        "#
    )
    .bind(sha_hash)
    .execute(db)
    .await?;

    Ok(result.rows_affected() > 0)
}

pub async fn generate_report(db: &SqlitePool, email: &str) -> Result<String, AppError> {
    #[derive(Debug)]
    struct ReportRow {
        id: Uuid,
        first_name: String,
        last_name: String,
        greeting: String,
        email: String,
        birthday: String,
        start_year: i64,
    }

    #[derive(Debug)]
    struct SentReportRow {
        id: i64,
        user_id: Uuid,
        sent_at: i64,
    }

    let normalized_email = email.trim();
    let people_rows = sqlx::query_as!(
        ReportRow,
        r#"
        SELECT
            id as "id!: uuid::Uuid",
            first_name as "first_name!",
            last_name as "last_name!",
            greeting as "greeting!",
            email as "email!",
            birthday as "birthday!",
            start_year as "start_year!"
        FROM people
        WHERE LOWER(email) = LOWER(?)
        ORDER BY last_name ASC, first_name ASC, start_year ASC
        "#,
        normalized_email
    )
    .fetch_all(db)
    .await?;

    let sent_rows = sqlx::query_as!(
        SentReportRow,
        r#"
        SELECT
            sent.id as "id!: i64",
            sent.user_id as "user_id!: uuid::Uuid",
            sent.sent_at as "sent_at!: i64"
        FROM sent
        INNER JOIN people ON people.id = sent.user_id
        WHERE LOWER(people.email) = LOWER(?)
        ORDER BY sent.sent_at ASC, sent.id ASC
        "#,
        normalized_email
    )
    .fetch_all(db)
    .await?;

    let generated_at_utc = Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    let mut report = format!(
        "DSGVO-Report for {}\nGenerated at: {}",
        normalized_email, generated_at_utc
    );
    if people_rows.is_empty() && sent_rows.is_empty() {
        report.push_str("\n\nno data found");
        return Ok(report);
    }

    if !people_rows.is_empty() {
        report.push_str("\n\npeople");
        for row in people_rows {
            report.push_str("\n\n");
            report.push_str(&format!("id: {}", row.id));
            report.push_str(&format!("\nfirst_name: {}", row.first_name));
            report.push_str(&format!("\nlast_name: {}", row.last_name));
            report.push_str(&format!("\ngreeting: {}", row.greeting));
            report.push_str(&format!("\nemail: {}", row.email));
            report.push_str(&format!("\nbirthday: {}", row.birthday));
            report.push_str(&format!("\nstart_year: {}", row.start_year));
        }
    }

    if !sent_rows.is_empty() {
        report.push_str("\n\nsent");
        for row in sent_rows {
            report.push_str("\n\n");
            report.push_str(&format!("id: {}", row.id));
            report.push_str(&format!("\nuser_id: {}", row.user_id));
            report.push_str(&format!("\nsent_at: {}", row.sent_at));
        }
    }

    Ok(report)
}

async fn peppered_mail_hash(email: &str) -> Result<String, AppError> {
    let settings = settings::load_settings().await?;
    let normalized_email = email.trim().to_ascii_lowercase();
    let mut hasher = Sha256::new();
    hasher.update(settings.pepper.as_bytes());
    hasher.update(b":");
    hasher.update(normalized_email.as_bytes());
    Ok(format!("{:x}", hasher.finalize()))
}

fn sanitize_filename(email: &str) -> String {
    email
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '_' | '-' => ch,
            _ => '_',
        })
        .collect()
}
