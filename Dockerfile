# syntax=docker/dockerfile:1
FROM rust:1 AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
# Toolchain build client wasm (hydrate): target wasm32 + dx (phiên bản khớp dioxus trong Cargo.toml)
RUN rustup target add wasm32-unknown-unknown && cargo install dioxus-cli --version '^0.7' --locked
COPY --from=planner /app/recipe.json recipe.json
# Build dependency riêng để cache layer
RUN cargo chef cook --release --recipe-path recipe.json -p mockhub-server
COPY . .
RUN cargo build --release -p mockhub-server
# public/app.css đã commit sẵn (xem README) nên không cần Node ở đây
RUN cd crates/mockhub-web && dx build --release --platform web

FROM debian:trixie-slim AS runtime
RUN useradd --system --uid 10001 mockhub \
    && mkdir /data && chown mockhub /data
WORKDIR /app
COPY --from=builder /app/target/release/mockhub /usr/local/bin/mockhub
COPY --from=builder /app/target/dx/mockhub-web/release/web/public ./public
COPY config ./config
USER mockhub

# web | smtp | imap | sms | dns(udp)
EXPOSE 8080 2525 1143 8081 5353/udp
# SQLite nằm ở /data (mount volume để giữ dữ liệu giữa các lần chạy)
VOLUME /data
ENV MOCKHUB__SERVER__HOST=0.0.0.0 \
    MOCKHUB__DATABASE__URL=sqlite:///data/mockhub.db \
    DIOXUS_PUBLIC_PATH=/app/public
ENTRYPOINT ["mockhub"]
CMD ["all"]
