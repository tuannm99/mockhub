# syntax=docker/dockerfile:1
FROM rust:1 AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependency riêng để cache layer
RUN cargo chef cook --release --recipe-path recipe.json -p mockhub-server
COPY . .
RUN cargo build --release -p mockhub-server

FROM debian:bookworm-slim AS runtime
RUN useradd --system --uid 10001 mockhub
WORKDIR /app
COPY --from=builder /app/target/release/mockhub /usr/local/bin/mockhub
COPY config ./config
USER mockhub

# web | smtp | imap | sms | dns(udp)
EXPOSE 8080 2525 1143 8081 5353/udp
ENV MOCKHUB__SERVER__HOST=0.0.0.0
ENTRYPOINT ["mockhub"]
CMD ["all"]
