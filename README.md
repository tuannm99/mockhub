# mockhub

Mock hub: mail (SMTP/IMAP), DNS, SMS for dev/test.

## Build & chạy

Yêu cầu: Rust stable (>= 1.85, edition 2024); Docker nếu chạy bằng container.

Toàn bộ chạy trong **một binary** `mockhub` (package `mockhub-server`):

```sh
cargo build --release -p mockhub-server   # -> target/release/mockhub
cargo run -p mockhub-server               # = `mockhub all`: web + các service đang enabled
cargo run -p mockhub-server -- web        # chỉ web (cũng có: mail | dns | sms)
cargo run -p mockhub-server -- -c path/to/config.toml all

cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all
```

### Cấu hình

`config/default.toml` (đổi đường dẫn bằng `-c` hoặc `MOCKHUB_CONFIG`). Mọi key override được bằng env
`MOCKHUB__<SECTION>__<KEY>`, ví dụ `MOCKHUB__SERVER__PORT=9000`, `MOCKHUB__DNS__ENABLED=false`.
Log: `RUST_LOG=debug`.

| Service | Port mặc định |
|---|---|
| web | 8080 |
| smtp / imap | 2525 / 1143 |
| sms api | 8081 |
| dns | 5353/udp |

### Docker

```sh
docker compose up --build          # mockhub + postgres
docker build -t mockhub . && docker run --rm -p 8080:8080 -p 2525:2525 -p 8081:8081 -p 5353:5353/udp mockhub
```

Container chạy non-root nên dùng cổng cao; muốn cổng chuẩn (25/53/143) thì map ở phía host (`-p 25:2525`).
Nếu cổng host bị chiếm (vd 1143), đổi vế trái của `-p`.
