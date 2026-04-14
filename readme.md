# Birthday mail sender

This is a simple utility to send Mails to people on their Birthday.

## ⚠️WARNING⚠️

> [!WARNING]
> **This repository is heavily vibe coded.**
>
> While I built the template and database scheme myself, a lot of the code was built using codex.
>
> It's a very simple app and we use it ourselves, but I still feel like it should be openly disclosed in my opinion
> it will always influence code quality.
>
> Feel free to check out the code if you're unsure

## features

- Send E-Mails on birthday
- Protection against accidentally sending a birthday mail twice
- DSGVO functions including Data deletion, generating a report and blocking addresses
- show scheduled mails for today
- uses SMTP to send emails

Templates are just normal EML files, so send yourself an email and incoperate the placeholders like you want.

## Installation

You can deploy this app using Docker.
Take a look at the [compose.yaml](compose.yaml) for a simple reference.

## Tech Stack

This is built with Rust, SQLx and HTML templates.
