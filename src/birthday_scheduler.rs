use axum::{
    extract::State,
    response::{Html, Redirect},
};
use chrono::{DateTime, Datelike, Local, NaiveTime, Timelike, Utc};
use serde::Serialize;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::{
    AppState,
    error::AppError,
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
    sent_at_display: Option<String>,
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
            sent_at_display: recipient.last_sent_at.and_then(format_sent_at_local),
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

pub async fn send(State(state): State<AppState>) -> Result<Redirect, AppError> {
    send_scheduled_mails(&state.db).await?;
    Ok(Redirect::to("/schedule"))
}

pub async fn send_scheduled_mails(db: &SqlitePool) -> Result<u64, AppError> {
    let settings = settings::load_settings().await?;

    if settings.disable_scheduled_mails || settings.send_for_years <= 0 {
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

pub async fn run_daily_scheduler(db: SqlitePool) {
    loop {
        let settings = match settings::load_settings().await {
            Ok(value) => value,
            Err(err) => {
                eprintln!("Birthday scheduler: could not load settings: {}", err);
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                continue;
            }
        };

        let sleep_seconds = seconds_until_next_run(&settings.schedule_at_local_time).max(60);

        tokio::time::sleep(std::time::Duration::from_secs(sleep_seconds as u64)).await;

        match send_scheduled_mails(&db).await {
            Ok(count) => {
                println!("Birthday scheduler: sent {} scheduled mail(s).", count);
            }
            Err(err) => {
                eprintln!("Birthday scheduler failed: {}", err);
            }
        }
    }
}

async fn load_scheduled_recipients(
    db: &SqlitePool,
    send_for_years: i64,
) -> Result<Vec<ScheduledRecipient>, AppError> {
    let now = Local::now();
    let today_month_day = now.format("%m-%d").to_string();
    let today_year = now.year();

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
        WHERE strftime('%m-%d', people.birthday) = ?
            AND ? >= people.start_year
            AND ? < people.start_year + ?
        GROUP BY
            people.id,
            people.first_name,
            people.last_name,
            people.greeting,
            people.email
        ORDER BY people.last_name ASC, people.first_name ASC
        "#,
        today_month_day,
        today_year,
        today_year,
        send_for_years
    )
    .fetch_all(db)
    .await?;

    Ok(recipients)
}

fn seconds_until_next_run(schedule_at_local_time: &str) -> i64 {
    let target = NaiveTime::parse_from_str(schedule_at_local_time, "%H:%M")
        .unwrap_or_else(|_| NaiveTime::from_hms_opt(9, 0, 0).expect("valid default time"));
    let now = Local::now().time();
    let now_seconds = now.num_seconds_from_midnight() as i64;
    let target_seconds = target.num_seconds_from_midnight() as i64;

    if now_seconds < target_seconds {
        target_seconds - now_seconds
    } else {
        (24 * 3600 - now_seconds) + target_seconds
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn format_sent_at_local(timestamp: i64) -> Option<String> {
    let utc: DateTime<Utc> = DateTime::from_timestamp(timestamp, 0)?;
    Some(
        utc.with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
    )
}
