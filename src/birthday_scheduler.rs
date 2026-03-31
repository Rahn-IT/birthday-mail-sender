use axum::{extract::State, response::Html};
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::{
    AppState, error::AppError,
    settings,
    template_mailer::{self, TemplateValues},
    users::CurrentUser,
};

const TEMPLATE_PATH: &str = "./db/template.eml";
const RECENT_SEND_WINDOW_SECONDS: i64 = 60 * 60 * 72;

#[derive(Debug)]
struct ScheduledRecipient {
    id: Uuid,
    first_name: String,
    last_name: String,
    greeting: String,
    email: String,
    last_sent_at: Option<i64>,
}

#[derive(Debug, Serialize)]
struct BirthdayScheduleView {
    is_admin: bool,
    entries: Vec<BirthdayScheduleItemView>,
}

#[derive(Debug, Serialize)]
struct BirthdayScheduleItemView {
    person_id: Uuid,
    full_name: String,
    email: String,
    has_been_sent: bool,
    last_sent_at: Option<i64>,
}

pub async fn index(
    State(state): State<AppState>,
    current_user: CurrentUser,
) -> Result<Html<String>, AppError> {
    let settings = settings::load_settings().await?;
    let recipients = if settings.send_for_years <= 0 {
        Vec::new()
    } else {
        load_scheduled_recipients(&state.db, settings.send_for_years).await?
    };
    let recent_threshold = unix_now().saturating_sub(RECENT_SEND_WINDOW_SECONDS);
    let entries = recipients
        .into_iter()
        .map(|recipient| BirthdayScheduleItemView {
            person_id: recipient.id,
            full_name: format!("{} {}", recipient.first_name, recipient.last_name),
            email: recipient.email,
            has_been_sent: recipient
                .last_sent_at
                .is_some_and(|value| value >= recent_threshold),
            last_sent_at: recipient.last_sent_at,
        })
        .collect();

    let template = state
        .jinja
        .get_template("birthday_schedule.html")
        .expect("template is loaded");
    let rendered = template.render(BirthdayScheduleView {
        is_admin: current_user.is_admin,
        entries,
    })?;
    Ok(Html(rendered))
}

pub async fn send_scheduled(db: &SqlitePool) -> Result<u64, AppError> {
    let settings = settings::load_settings().await?;
    if settings.send_for_years <= 0 {
        return Ok(0);
    }

    let template_bytes = tokio::fs::read(TEMPLATE_PATH).await?;
    let recipients = load_scheduled_recipients(db, settings.send_for_years).await?;
    let mut sent_count = 0_u64;
    let recent_threshold = unix_now().saturating_sub(RECENT_SEND_WINDOW_SECONDS);

    for recipient in recipients {
        if recipient
            .last_sent_at
            .is_some_and(|value| value >= recent_threshold)
        {
            continue;
        }

        let values = TemplateValues {
            greeting: &recipient.greeting,
            last_name: &recipient.last_name,
            first_name: &recipient.first_name,
        };

        template_mailer::send_template_mail_with_loaded_settings(
            &template_bytes,
            &recipient.email,
            &values,
        )
        .await?;

        let sent_at = unix_now();
        sqlx::query!(
            "INSERT INTO sent (user_id, sent_at) VALUES (?, ?)",
            recipient.id,
            sent_at
        )
        .execute(db)
        .await?;

        sent_count += 1;
    }

    Ok(sent_count)
}

async fn load_scheduled_recipients(
    db: &SqlitePool,
    send_for_years: i64,
) -> Result<Vec<ScheduledRecipient>, AppError> {
    let recipients = sqlx::query_as!(
        ScheduledRecipient,
        r#"
        SELECT
            people.id as "id!: uuid::Uuid",
            people.first_name as "first_name!",
            people.last_name as "last_name!",
            people.greeting as "greeting!",
            people.email as "email!",
            MAX(sent.sent_at) as "last_sent_at?: i64"
        FROM people
        LEFT JOIN sent ON sent.user_id = people.id
        WHERE strftime('%m-%d', people.birthday) = strftime('%m-%d', 'now', 'localtime')
            AND CAST(strftime('%Y', 'now', 'localtime') AS INTEGER) >= people.start_year
            AND CAST(strftime('%Y', 'now', 'localtime') AS INTEGER) < people.start_year + ?
        GROUP BY
            people.id,
            people.first_name,
            people.last_name,
            people.greeting,
            people.email
        ORDER BY people.last_name ASC, people.first_name ASC
        "#,
        send_for_years
    )
    .fetch_all(db)
    .await?;

    Ok(recipients)
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}
