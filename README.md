# mockhub

Mock hub: mail (SMTP/IMAP), DNS, SMS for dev/test.

## Kiến trúc

Một binary `mockhub` (`apps/mockhub-server`) chạy nhiều service trong cùng process (mỗi service là một task tokio;
một service chết thì cả process dừng để container restart). Mỗi service là một crate riêng, dùng chung `core` và `storage`.

### Các crate

| Crate (`crates/`)  | Vai trò                                                                                                                | Trạng thái                                                              |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `mockhub-core`     | Config (`config/default.toml` + env), lỗi chung, **kiểu domain thuần** (`domain::dns`...), events. Không phụ thuộc DB/HTTP. | Dùng                                                                    |
| `mockhub-storage`  | **Trait** truy cập dữ liệu (`DnsRecordRepo`, gộp thành `Storage`) + backend SQLite + migration. Xem mục "Storage".       | Dùng (mới có DNS records)                                               |
| `mockhub-web`      | Web UI **và** API của UI: Dioxus fullstack (SSR + hydrate), server function trong `api.rs`. Xem mục "Web UI".           | Dùng                                                                    |
| `mockhub-mail`     | SMTP (nhận) + IMAP/POP3 (đọc) mock; lưu To/CC/BCC + file đính kèm.                                                                                          | Khung: bind cổng, trả banner rồi đóng; chưa nhận/lưu mail               |
| `mockhub-dns` | DNS mock UDP+TCP: mọi loại record + tình huống lỗi/chậm (định hướng `hickory-server`). | Khung: bind UDP, bỏ qua gói tin; chưa đọc records từ storage |
| `mockhub-sms` | SMS mock: nhận tin qua HTTP, xem số + nội dung (không trạng thái giao/webhook). | Khung: bind TCP, trả 501 |
| `mockhub-http` | HTTP mock kiểu WireMock: khai báo stub, response/độ trễ/lỗi, nhật ký request (ROADMAP Phase 4). | **Chưa tạo** (dự kiến) |
| `mockhub-api` | Dự phòng cho **REST công khai, có versioning** cho code test (dự kiến: REST admin của HTTP mock `/v1/stubs`, reset dữ liệu...). UI **không** cần nó. | Skeleton rỗng, chưa ai dùng |

`apps/mockhub-server` là điểm lắp ráp: parse CLI (`all|web|mail|dns|sms`), nạp config, mở storage, khởi chạy các service.

### Phụ thuộc giữa các crate

```
                    mockhub-server (apps/)         <- binary duy nhất, lắp ráp mọi thứ
        ┌────────────┬────────┴───┬────────────┐
   mockhub-web  mockhub-mail  mockhub-dns  mockhub-sms        (mockhub-api: chưa nối vào đâu)
        │            └────────────┴────────────┘
        │                         │
        └───────────────┬─────────┘
                  mockhub-storage   ← trait Storage + backend (sqlite)
                        │
                   mockhub-core     ← config, errors, domain
```

Quy tắc: mũi tên chỉ đi xuống. `core` không biết ai; service crate không biết SQLite/Postgres (chỉ thấy
`Arc<dyn Storage>`); `mockhub-web` chỉ kéo `storage`/`core` ở phía server (feature `server`), không vào wasm.

### Luồng một request (web)

```
Trình duyệt ──GET /dns──▶ mockhub web (axum) ──▶ Dioxus SSR: chạy DnsPage trên server
                                                   └─ server function list_dns_records() gọi thẳng Storage
                          ◀── HTML đã có dữ liệu + link /app.css + wasm ──
Trình duyệt tải wasm ──▶ hydrate (gắn event vào HTML có sẵn, không fetch lại dữ liệu lần đầu)
Bấm "Thêm"  ──POST /api/dns/records (JSON)──▶ server function add_dns_record() ──▶ Storage ──▶ SQLite
```

- **SSR không thay được API**: SSR chỉ lo lần tải đầu; sau hydrate, thao tác (thêm/xoá) vẫn phải gọi server.
  Phần đó do **server function** trong `mockhub-web/src/api.rs` đảm nhiệm (Dioxus tự đăng ký route `/api/...`,
  JSON, type request/response dùng chung hai phía) nên **không cần crate API riêng cho UI**.
- Mock service (SMTP/IMAP/DNS/SMS) nghe cổng riêng của chúng (xem bảng cổng ở mục "Cấu hình"), không đi qua web.
- `mockhub-api` chỉ đáng làm khi cần REST công khai ổn định cho bên ngoài; endpoint của UI có thể đổi theo UI nên
  không nên coi là contract.

### Thêm một tính năng dữ liệu mới (ví dụ: lưu mail nhận được)

1. Kiểu domain mới trong `mockhub-core::domain`.
2. Trait `XxxRepo` trong `mockhub-storage::repo` (thêm vào supertrait của `Storage`) + implement SQLite + migration.
3. Service (`mockhub-mail`...) nhận `Arc<dyn Storage>` và ghi/đọc qua trait.
4. Muốn hiện trên UI: thêm server function vào `mockhub-web/src/api.rs` + trang/component; style xem mục "Style".

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

API mẫu (từ `api.rs`): `GET/POST /api/dns/records`, `POST /api/dns/records/delete`. Dữ liệu lưu qua `mockhub-storage`
(xem mục "Storage").

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

### Storage (SQLite, có interface để đổi backend)

Mặc định dùng **SQLite** (`database.url = "sqlite://data/mockhub.db"`, file + thư mục cha tự tạo, migration tự chạy lúc
khởi động; `data/` đã gitignore). Dạng URL: `sqlite://data/x.db` (tương đối), `sqlite:///data/x.db` (tuyệt đối),
`sqlite::memory:` (mất khi tắt, tiện cho test). Override bằng env `MOCKHUB__DATABASE__URL`.

Code ngoài `crates/mockhub-storage` **chỉ biết trait**, không biết SQLite:

```
mockhub-core::domain   kiểu domain thuần (DnsRecord, NewDnsRecord...)
mockhub-storage::repo  trait DnsRecordRepo, ... gộp thành `trait Storage`; caller dùng `Arc<dyn Storage>`
mockhub-storage::sqlite  backend hiện tại (sqlx, migration ở migrations/sqlite/)
mockhub-storage::connect(url)  chọn backend theo scheme của url (nay chỉ `sqlite:`; scheme khác -> lỗi rõ ràng)
```

- Lỗi dùng `StorageError` (không lộ kiểu của sqlx) nên đổi backend không đụng caller.
- **Thêm dữ liệu mới**: kiểu domain trong `core` → trait `XxxRepo` trong `repo.rs` (thêm vào supertrait của `Storage`)
  → implement + migration mới (`migrations/sqlite/000N_*.sql`).
- **Chuyển sang Postgres sau này**: thêm feature `postgres` cho `sqlx` ở workspace, tạo `src/postgres/` implement các
  trait (cú pháp SQL khác: `$1`, `BIGSERIAL`...) cùng `migrations/postgres/`, thêm nhánh `postgres://` vào `connect`.
  Chi tiết ở doc đầu `crates/mockhub-storage/src/lib.rs`. Web/DNS/mail/sms không phải sửa. Dữ liệu cũ không tự chuyển
  (cần export/import riêng).
- Test: `cargo test -p mockhub-storage` (dùng SQLite in-memory). Backend mới nên chạy lại đúng bộ kiểm tra CRUD đó.
- Docker: DB ở `/data/mockhub.db`, `docker compose` mount volume `mockhub-data` nên dữ liệu giữ qua các lần chạy.

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
docker compose up --build          # mockhub (SQLite trong volume `mockhub-data`)
docker build -t mockhub . && docker run --rm -p 8080:8080 -p 2525:2525 -p 8081:8081 -p 5353:5353/udp mockhub
```

Container chạy non-root nên dùng cổng cao; muốn cổng chuẩn (25/53/143) thì map ở phía host (`-p 25:2525`).
Nếu cổng host bị chiếm (vd 1143), đổi vế trái của `-p`.
