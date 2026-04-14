FROM rust AS builder
WORKDIR /app
COPY *.toml .
COPY Cargo.lock .
COPY ./build.rs .
COPY ./src ./src
COPY ./assets ./assets
COPY ./migrations ./migrations
ENV DATABASE_URL="sqlite://dev.sqlite"
RUN cargo install sqlx-cli --no-default-features --features sqlite
RUN cargo sqlx database create
RUN cargo sqlx migrate run
RUN cargo build --release

FROM debian:stable-slim AS runner
RUN mkdir -p /app/db
WORKDIR /app
COPY --from=builder /app/target/release/birthday-mail-sender /app/birthday-mail-sender
EXPOSE 4046
VOLUME /app/db
CMD ["/app/birthday-mail-sender"]
