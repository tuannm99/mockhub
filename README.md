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

### Web UI (Dioxus fullstack: SSR + hydrate)

`crates/mockhub-web` là một crate Dioxus 0.7 dùng cho cả hai phía (feature `web` = client wasm,
`server` = SSR + server functions; `dx` tự chọn). `mockhub web` render HTML sẵn (SSR), client tải wasm
rồi hydrate và chạy tiếp như CSR. API gọi bằng _server function_ trong `src/api.rs` (`#[get]`/`#[post]`,
xuất ra `/api/...` JSON, type dùng chung hai phía). Mẫu hoàn chỉnh: trang DNS (`pages/mod.rs`) + `api.rs`.

#### Cần cài gì

| Việc                                      | Cần                                                                                                        |
| ----------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| Chạy server / build client / build Docker | Rust, `rustup target add wasm32-unknown-unknown`, `dx` (bên dưới). **Không cần Node.**                      |
| Sửa style (thêm/đổi class trong `rsx!`)   | Thêm Node + npm (chỉ để build CSS), rồi chạy `npm ci` một lần trong `crates/mockhub-web`.                  |

```sh
cargo install dioxus-cli --version '^0.7' --locked   # `dx` (hoặc: curl -sSL https://dioxus.dev/install.sh | bash)
```

Mạng private: mọi thứ cài từ crates.io (cargo) và, chỉ khi sửa style, npm registry. Không có CDN lúc chạy hay lúc build.
`dx` lần đầu có thể tải thêm `wasm-bindgen`/`wasm-opt` từ GitHub — cần mirror hoặc cài sẵn nếu máy build không ra được internet.

#### Chạy

```sh
# 1) Build client (wasm + file tĩnh) -> target/dx/mockhub-web/release/web/public
cd crates/mockhub-web && dx build --release --platform web && cd ../..

# 2) Chạy server. Tự tìm thư mục public ở bước 1 (xem "Thứ tự tìm client" bên dưới)
cargo run -p mockhub-server -- web      # hoặc không có `web` để chạy cả mail/dns/sms

# Dev UI có hot reload (server dioxus độc lập :8080, không chạy mail/dns/sms)
cd crates/mockhub-web && dx serve --fullstack
```

Thứ tự tìm client (thư mục `public`): biến `DIOXUS_PUBLIC_PATH` → `<thư mục chứa exe>/public` (Docker) →
`target/dx/mockhub-web/{release,debug}/web/public` (chạy từ gốc repo). Không thấy thì `mockhub all` bỏ qua web
(mail/dns/sms vẫn chạy) và `mockhub web` báo lỗi kèm hướng dẫn.

API mẫu (từ `api.rs`): `GET/POST /api/dns/records`, `POST /api/dns/records/delete`. Store DNS ở đó đang
in-memory, thay bằng `mockhub-storage`/`mockhub-dns` khi làm thật.

#### Style (Tailwind v4 + DaisyUI v5)

- Viết class thẳng trong `rsx!`: `class: "flex gap-2 btn btn-primary"` (utility của Tailwind + component của
  [DaisyUI](https://daisyui.com/components)). Component bọc sẵn ở `src/components/mod.rs`.
- Tailwind sinh CSS từ `styles/input.css`, quét `src/**/*.rs`, **chỉ giữ class thật sự có trong code**.
  Kết quả là `crates/mockhub-web/public/app.css` — **file sinh ra nhưng được commit** để máy build không cần Node.
  `dx` copy cả thư mục `public/` vào gốc output nên trang gọi `/app.css`.
- **Mỗi khi thêm/đổi class trong code Rust, phải build lại và commit `app.css`** (quên thì class mới không có style):

```sh
cd crates/mockhub-web
npm ci                # lần đầu (Tailwind + DaisyUI, xem package.json / package-lock.json)
npm run css           # sinh lại public/app.css
npm run css:watch     # dev: tự sinh lại khi sửa code (terminal riêng, chạy cùng `dx serve`)
npm run css:check     # kiểm tra app.css đã khớp code chưa (thoát lỗi nếu cũ) — dùng trước khi commit/CI
```

- Đổi theme/màu: sửa `themes:` trong `styles/input.css` (xem danh sách theme của DaisyUI) rồi `npm run css`.
  Theme đang bật: `light` (mặc định) và `dark` (theo hệ điều hành).
- Không dùng macro `asset!()` cho CSS: server build bằng `cargo` không được `dx` xử lý hậu kỳ nên đường dẫn
  trong HTML SSR sẽ sai. File trong `public/` có đường dẫn cố định (`/app.css`) nên không bị lỗi này.

### Cấu hình

`config/default.toml` (đổi đường dẫn bằng `-c` hoặc `MOCKHUB_CONFIG`). Mọi key override được bằng env
`MOCKHUB__<SECTION>__<KEY>`, ví dụ `MOCKHUB__SERVER__PORT=9000`, `MOCKHUB__DNS__ENABLED=false`.
Log: `RUST_LOG=debug`.

| Service     | Port mặc định |
| ----------- | ------------- |
| web         | 8080          |
| smtp / imap | 2525 / 1143   |
| sms api     | 8081          |
| dns         | 5353/udp      |

### Docker

Image build bằng `cargo` + `dx` (client wasm), **không cần Node** vì `public/app.css` đã commit. Image runtime
`debian:trixie-slim` (glibc phải khớp image build `rust:1`), client nằm ở `/app/public` và `DIOXUS_PUBLIC_PATH` đã đặt sẵn.

```sh
docker compose up --build          # mockhub + postgres
docker build -t mockhub . && docker run --rm -p 8080:8080 -p 2525:2525 -p 8081:8081 -p 5353:5353/udp mockhub
```

Container chạy non-root nên dùng cổng cao; muốn cổng chuẩn (25/53/143) thì map ở phía host (`-p 25:2525`).
Nếu cổng host bị chiếm (vd 1143), đổi vế trái của `-p`.
