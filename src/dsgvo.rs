use axum::{extract::State, response::Html};
use axum_extra::extract::Form;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::{AppState, error::AppError, users::CurrentUser};

#[derive(Debug, Deserialize)]
pub struct DeleteByEmailForm {
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
}

pub async fn index(
    State(state): State<AppState>,
    current_user: CurrentUser,
) -> Result<Html<String>, AppError> {
    render_page(&state, &current_user, String::new(), None, None)
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
            Some("Please enter a valid email address."),
            None,
        );
    }

    let deleted_count = delete_people_by_email(&state.db, &email).await?;
    render_page(
        &state,
        &current_user,
        String::new(),
        None,
        Some(&format!(
            "Deleted {} record(s) for the submitted email address.",
            deleted_count
        )),
    )
}

fn render_page(
    state: &AppState,
    current_user: &CurrentUser,
    email: String,
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
