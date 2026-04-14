use axum::{
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::Form;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppState, dsgvo, error::AppError, users::CurrentUser};

const PEOPLE_PAGE_SIZE: i64 = 25;

#[derive(Debug, Clone)]
struct Person {
    id: Uuid,
    first_name: String,
    last_name: String,
    greeting: String,
    email: String,
    birthday: String,
    start_year: i64,
}

impl Person {
    fn into_list_item(self) -> PersonListItem {
        PersonListItem {
            id: self.id,
            first_name: self.first_name,
            last_name: self.last_name,
            greeting: self.greeting,
            email: self.email,
            birthday: self.birthday,
            start_year: self.start_year,
        }
    }

    fn into_form_view(self) -> PersonFormView {
        PersonFormView {
            id: Some(self.id),
            first_name: self.first_name,
            last_name: self.last_name,
            greeting: self.greeting,
            email: self.email,
            birthday: self.birthday,
        }
    }

    fn into_detail_view(self) -> PersonDetailView {
        PersonDetailView {
            id: self.id,
            first_name: self.first_name,
            last_name: self.last_name,
            greeting: self.greeting,
            email: self.email,
            birthday: self.birthday,
            start_year: self.start_year,
        }
    }
}

#[derive(Debug, Serialize)]
struct PeopleIndexView {
    is_admin: bool,
    people: Vec<PersonListItem>,
    page: i64,
    total_pages: i64,
    has_previous_page: bool,
    has_next_page: bool,
    previous_page_href: String,
    next_page_href: String,
}

#[derive(Debug, Serialize)]
struct PersonListItem {
    id: Uuid,
    first_name: String,
    last_name: String,
    greeting: String,
    email: String,
    birthday: String,
    start_year: i64,
}

#[derive(Debug, Serialize)]
struct PersonFormPageView {
    is_admin: bool,
    title: String,
    form_action: String,
    submit_label: String,
    cancel_href: String,
    has_error: bool,
    error_message: Option<String>,
    form: PersonFormView,
}

#[derive(Debug, Serialize)]
struct PersonShowView {
    is_admin: bool,
    person: PersonDetailView,
}

#[derive(Debug, Serialize)]
struct PersonFormView {
    id: Option<Uuid>,
    first_name: String,
    last_name: String,
    greeting: String,
    email: String,
    birthday: String,
}

impl Default for PersonFormView {
    fn default() -> Self {
        Self {
            id: None,
            first_name: String::new(),
            last_name: String::new(),
            greeting: String::new(),
            email: String::new(),
            birthday: String::new(),
        }
    }
}

#[derive(Debug, Serialize)]
struct PersonDetailView {
    id: Uuid,
    first_name: String,
    last_name: String,
    greeting: String,
    email: String,
    birthday: String,
    start_year: i64,
}

#[derive(Debug, Deserialize)]
pub struct PersonForm {
    first_name: String,
    last_name: String,
    greeting: String,
    email: String,
    birthday: String,
}

#[derive(Debug, Deserialize)]
pub struct PeopleIndexQuery {
    page: Option<i64>,
}

enum FormPageMode {
    Create,
    Edit,
}

pub async fn index(
    State(state): State<AppState>,
    Query(query): Query<PeopleIndexQuery>,
    current_user: CurrentUser,
) -> Result<Html<String>, AppError> {
    let page = query.page.unwrap_or(1).max(1);
    let total_people = count_people(&state).await?;
    let total_pages = ((total_people + PEOPLE_PAGE_SIZE - 1) / PEOPLE_PAGE_SIZE).max(1);
    let page = page.min(total_pages);
    let people = load_people(&state, page).await?;
    render_index(&state, &current_user, people, page, total_pages)
}

pub async fn create_get(
    State(state): State<AppState>,
    current_user: CurrentUser,
) -> Result<Html<String>, AppError> {
    render_form(
        &state,
        &current_user,
        PersonFormView::default(),
        None,
        FormPageMode::Create,
    )
}

pub async fn create_post(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Form(form): Form<PersonForm>,
) -> Result<Response, AppError> {
    let first_name = form.first_name.trim().to_string();
    let last_name = form.last_name.trim().to_string();
    let greeting = form.greeting.trim().to_string();
    let email = form.email.trim().to_string();
    let birthday = form.birthday.trim().to_string();

    if dsgvo::mail_is_blocked(&state.db, &email).await? {
        return render_form(
            &state,
            &current_user,
            PersonFormView {
                id: None,
                first_name,
                last_name,
                greeting,
                email,
                birthday,
            },
            Some(
                "This email address is DSGVO-blocked and cannot be saved. Delete this entry instead if it should be removed.",
            ),
            FormPageMode::Create,
        )
        .map(IntoResponse::into_response);
    }

    let id = Uuid::new_v4();
    sqlx::query!(
        r#"
        INSERT INTO people (id, first_name, last_name, greeting, email, birthday, start_year)
        VALUES (?, ?, ?, ?, ?, ?, CAST(strftime('%Y', 'now') AS INTEGER))
        "#,
        id,
        first_name,
        last_name,
        greeting,
        email,
        birthday
    )
    .execute(&state.db)
    .await?;

    let _ = current_user;
    Ok(Redirect::to(&format!("/people/{}", id)).into_response())
}

pub async fn show(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Path(id): Path<Uuid>,
) -> Result<Html<String>, AppError> {
    let person = load_person(&state, id).await?;
    render_show(&state, &current_user, person.into_detail_view())
}

pub async fn edit_get(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Path(id): Path<Uuid>,
) -> Result<Html<String>, AppError> {
    let person = load_person(&state, id).await?;
    render_form(
        &state,
        &current_user,
        person.into_form_view(),
        None,
        FormPageMode::Edit,
    )
}

pub async fn edit_post(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Path(id): Path<Uuid>,
    Form(form): Form<PersonForm>,
) -> Result<Response, AppError> {
    let existing = load_person(&state, id).await?;

    let first_name = form.first_name.trim().to_string();
    let last_name = form.last_name.trim().to_string();
    let greeting = form.greeting.trim().to_string();
    let email = form.email.trim().to_string();
    let birthday = form.birthday.trim().to_string();

    if dsgvo::mail_is_blocked(&state.db, &email).await? {
        return render_form(
            &state,
            &current_user,
            PersonFormView {
                id: Some(id),
                first_name,
                last_name,
                greeting,
                email,
                birthday,
            },
            Some(
                "This email address is DSGVO-blocked and cannot be saved. Delete this entry instead if it should be removed.",
            ),
            FormPageMode::Edit,
        )
        .map(IntoResponse::into_response);
    }

    sqlx::query!(
        r#"
        UPDATE people
        SET first_name = ?, last_name = ?, greeting = ?, email = ?, birthday = ?
        WHERE id = ?
        "#,
        first_name,
        last_name,
        greeting,
        email,
        birthday,
        existing.id
    )
    .execute(&state.db)
    .await?;

    let _ = current_user;
    Ok(Redirect::to(&format!("/people/{}", id)).into_response())
}

async fn count_people(state: &AppState) -> Result<i64, AppError> {
    Ok(
        sqlx::query_scalar!("SELECT COUNT(*) as \"count!: i64\" FROM people")
            .fetch_one(&state.db)
            .await?,
    )
}

async fn load_people(state: &AppState, page: i64) -> Result<Vec<PersonListItem>, AppError> {
    let offset = (page - 1) * PEOPLE_PAGE_SIZE;
    let people = sqlx::query_as!(
        Person,
        r#"
        SELECT
            id as "id: uuid::Uuid",
            first_name,
            last_name,
            greeting,
            email,
            birthday,
            start_year
        FROM people
        ORDER BY last_name ASC, first_name ASC, start_year ASC
        LIMIT ? OFFSET ?
        "#,
        PEOPLE_PAGE_SIZE,
        offset
    )
    .fetch_all(&state.db)
    .await?;

    Ok(people.into_iter().map(Person::into_list_item).collect())
}

async fn load_person(state: &AppState, id: Uuid) -> Result<Person, AppError> {
    let person = sqlx::query_as!(
        Person,
        r#"
        SELECT
            id as "id: uuid::Uuid",
            first_name,
            last_name,
            greeting,
            email,
            birthday,
            start_year
        FROM people
        WHERE id = ?
        LIMIT 1
        "#,
        id
    )
    .fetch_optional(&state.db)
    .await?;

    person.ok_or_else(|| AppError::not_found_for("User", format!("No user exists for id: {}", id)))
}

fn render_show(
    state: &AppState,
    current_user: &CurrentUser,
    person: PersonDetailView,
) -> Result<Html<String>, AppError> {
    let template = state
        .jinja
        .get_template("person_show.html")
        .expect("template is loaded");
    let rendered = template.render(PersonShowView {
        is_admin: current_user.is_admin,
        person,
    })?;
    Ok(Html(rendered))
}

fn render_index(
    state: &AppState,
    current_user: &CurrentUser,
    people: Vec<PersonListItem>,
    page: i64,
    total_pages: i64,
) -> Result<Html<String>, AppError> {
    let template = state
        .jinja
        .get_template("people.html")
        .expect("template is loaded");
    let rendered = template.render(PeopleIndexView {
        is_admin: current_user.is_admin,
        people,
        page,
        total_pages,
        has_previous_page: page > 1,
        has_next_page: page < total_pages,
        previous_page_href: format!("/people?page={}", page - 1),
        next_page_href: format!("/people?page={}", page + 1),
    })?;
    Ok(Html(rendered))
}

fn render_form(
    state: &AppState,
    current_user: &CurrentUser,
    form: PersonFormView,
    error_message: Option<&str>,
    mode: FormPageMode,
) -> Result<Html<String>, AppError> {
    let template = state
        .jinja
        .get_template("person_form.html")
        .expect("template is loaded");
    let (title, form_action, submit_label, cancel_href) = match mode {
        FormPageMode::Create => (
            "Add Person".to_string(),
            "/people/new".to_string(),
            "Create User".to_string(),
            "/people".to_string(),
        ),
        FormPageMode::Edit => {
            let id = form.id.expect("edit form requires id");
            (
                "Edit User".to_string(),
                format!("/people/edit/{}", id),
                "Save Changes".to_string(),
                format!("/people/{}", id),
            )
        }
    };
    let rendered = template.render(PersonFormPageView {
        is_admin: current_user.is_admin,
        title,
        form_action,
        submit_label,
        cancel_href,
        has_error: error_message.is_some(),
        error_message: error_message.map(str::to_string),
        form,
    })?;
    Ok(Html(rendered))
}
