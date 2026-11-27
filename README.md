# mockhub

Mock hub: mail (SMTP/IMAP/POP3), DNS, SMS for dev/test.

## Kiến trúc

Một binary `mockhub` (`apps/mockhub-server`) chạy nhiều service trong cùng process (mỗi service là một task tokio;
một service chết thì cả process dừng để container restart). Mỗi service là một crate riêng, dùng chung `core` và `storage`.

### Các crate

| Crate (`crates/`)  | Vai trò                                                                                                                | Trạng thái                                                              |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `mockhub-core`     | Config (`config/default.toml` + env), lỗi chung, **kiểu domain thuần** (`domain::dns`...), events. Không phụ thuộc DB/HTTP. | Dùng                                                                    |
| `mockhub-storage`  | **Trait** truy cập dữ liệu (`DnsRecordRepo`, `DnsFaultRepo`, `MailRepo`, gộp thành `Storage`) + backend SQLite + migration. Xem mục "Storage".       | Dùng (DNS record/fault, mail) |
| `mockhub-web`      | Web UI **và** API của UI: Dioxus fullstack (SSR + hydrate), server function trong `api.rs`. Xem mục "Web UI".           | Dùng                                                                    |
| `mockhub-mail`     | SMTP (nhận) + IMAP/POP3 (đọc); lưu người nhận To/CC/BCC và nguyên văn. Xem mục "Mail".                              | **Dùng được** |
| `mockhub-dns` | DNS mock UDP+TCP: record theo cú pháp zone file, CNAME, wildcard, NODATA/NXDOMAIN, tình huống lỗi, forward tùy chọn, log truy vấn. Dùng `hickory-proto`. | **Dùng được** |
| `mockhub-sms` | SMS mock: nhận tin qua HTTP, xem số + nội dung (không trạng thái giao/webhook). | Khung: bind TCP, trả 501 |
| `mockhub-http` | HTTP mock kiểu WireMock: khai báo stub, response/độ trễ/lỗi, nhật ký request (ROADMAP Phase 4). | **Chưa tạo** (dự kiến) |
| `mockhub-api` | REST cho script/test (`/v1/...`), DoH (`/resolve`, `/dns-query`), middleware xác thực. Cổng riêng. UI **không** gọi nó. Xem mục "REST API". | Dùng (DNS, mail) |

`apps/mockhub-server` là điểm lắp ráp: parse CLI (`all|web|mail|dns|sms|api`), nạp config, mở storage một lần, khởi chạy các service.

### Phụ thuộc giữa các crate

```
                    mockhub-server (apps/)         <- binary duy nhất, lắp ráp mọi thứ
        ┌────────────┬────────┴───┬────────────┐
   mockhub-web  mockhub-mail  mockhub-dns  mockhub-sms        (mockhub-api dùng mockhub-dns để trả `/resolve`)
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
cargo run -p mockhub-server -- web        # chỉ web (cũng có: mail | dns | sms | api)
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

# Dev UI có hot reload (server dioxus độc lập :8080 mặc định của dx, không đọc config, không chạy mail/dns/sms)
dx serve -p mockhub-web                 # chạy từ gốc repo; nếu dx không tự nhận fullstack thì thêm --fullstack
```

- **`dx serve` vs `mockhub-server`**: `dx serve` chỉ chạy web (build lại cả client wasm lẫn server, hot reload).
  `mockhub-server` chạy thêm SMTP/IMAP/DNS/SMS nhưng **chỉ phục vụ client đã `dx build` sẵn**.
- **Sửa code `mockhub-web` rồi chạy `cargo run -p mockhub-server` mà quên `dx build` lại** thì wasm cũ hydrate lên HTML
  SSR mới → lệch route/DOM (vd nav biến mất khi chuyển trang). Dev UI cứ dùng `dx serve`; muốn chạy server thật thì
  `dx build` lại client trước. Lưu ý server ưu tiên thư mục `release` hơn `debug`, bản cũ có thể che bản mới.
- `dx serve` không qua `mockhub-server` nên tự mở DB: mặc định `data/mockhub.db` ở **gốc repo** (không phụ thuộc cwd),
  override bằng `MOCKHUB__DATABASE__URL`.

Thứ tự tìm client (thư mục `public`): biến `DIOXUS_PUBLIC_PATH` → `<thư mục chứa exe>/public` (Docker) →
`target/dx/mockhub-web/{release,debug}/web/public` (chạy từ gốc repo). Không thấy thì `mockhub all` bỏ qua web
(mail/dns/sms vẫn chạy) và `mockhub web` báo lỗi kèm hướng dẫn.

Trang đã nối dữ liệu thật: Home (số liệu Mail, DNS), **Mail** (lọc, đã đọc/chưa đọc, HTML trong `iframe sandbox`,
văn bản, bản gốc, header, link kèm nút sao chép/mở, xoá hết) và **DNS** (record, lỗi giả lập, tra thử, log truy vấn).
Cả hai tự làm mới mỗi 2 giây (`src/live.rs`). SMS và API Mock còn là dữ liệu mẫu.
Dữ liệu đi qua server function trong `api.rs` và lưu qua `mockhub-storage` (xem mục "Storage").

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
| web         | 8686          |
| smtp        | 2525          |
| imap / pop3 | 1143 / 1110 (`0` = tắt) |
| sms api     | 8081          |
| dns         | 5353/udp+tcp  |
| REST + DoH  | 8053          |

Trên máy dev, cổng 5353 thường bị mDNS chiếm: đặt `MOCKHUB__DNS__PORT=15353`.

### REST API

Cổng `[api]`, không cần build client web. Nghe trên `server.host` (mặc định `0.0.0.0`) và **mặc định không xác thực**:
chạy chung thì đặt admin token (mục "Xác thực"); muốn giới hạn ở một máy thì đặt `MOCKHUB__SERVER__HOST=127.0.0.1`
(khi đó container khác không gọi vào được). Thành công trả `{"data": ...}`, lỗi trả `{"error": {"code", "message"}}`.

| Method + path | Việc |
|---|---|
| `GET /resolve?name=&type=` | DNS-over-HTTPS dạng JSON như dns.google (`type`: tên hoặc số, mặc định `A`) |
| `GET /dns-query?dns=`, `POST /dns-query` | DNS-over-HTTPS dạng wire (RFC 8484, `application/dns-message`) |
| `GET /v1/dns/queries?limit=`, `DELETE /v1/dns/queries` | log truy vấn (500 dòng gần nhất, mới nhất trước): `time, proto, name, type, rcode, answers, source` |
| `GET /v1/dns/records` | liệt kê record |
| `POST /v1/dns/records` | thêm `{name, type, value, ttl?, replace?}`; `replace` thay mọi record cùng tên + loại |
| `DELETE /v1/dns/records?name=&type=` / `/v1/dns/records/{id}` | xoá |
| `GET/POST /v1/dns/faults`, `DELETE /v1/dns/faults[/{id}]` | `{name, kind?, action, delayMs?}`; `action`: `servfail`, `refused`, `nxdomain`, `timeout`, `delay` |
| `GET /v1/mail/messages?to=&limit=` | thư, mới nhất trước; `to` khớp mọi người nhận (cả CC/BCC) |
| `GET /v1/mail/messages/latest?to=` / `/{id}` | một thư (`404` khi chưa có) |
| `DELETE /v1/mail/messages` | xoá thư |
| `DELETE /v1/reset` | xoá record, fault, log truy vấn và thư |

```sh
curl -XPOST localhost:8053/v1/dns/records -H 'content-type: application/json' \
  -d '{"name":"_waf-verify.shop.test","type":"TXT","value":"token-123","replace":true}'
dig @127.0.0.1 -p 15353 _waf-verify.shop.test TXT
curl 'localhost:8053/v1/mail/messages/latest?to=bob@mock.test'
```

DNS: tên không phân biệt hoa thường; CNAME được đi theo và trả cả chuỗi (vòng lặp trả `SERVFAIL`); wildcard chỉ phủ
tên không tồn tại; tên có nhưng không có loại được hỏi trả rỗng (`NOERROR`); tên lạ trả `NXDOMAIN`, trừ khi đặt
`dns.upstream` (`MOCKHUB__DNS__UPSTREAM=1.1.1.1:53`): khi đó tên mock không sở hữu, kể cả đích của CNAME đi ra ngoài
mock, được hỏi resolver thật (không cache; resolver im lặng thì `SERVFAIL`). Instance có upstream và mở ra ngoài là
một **open resolver**: chỉ để trong mạng tin cậy. `proto` trong log: `udp`, `tcp`, `doh`, `doh-json`, `ui`; `source`:
`mock`, `upstream`, `fault`, `none`.
Câu trả lời không vừa gói UDP thì bật cờ TC để client hỏi lại qua TCP. `dns.seed_file` (vd
`config/seed.example.json`) nạp record khi DB chưa có record nào.

SMTP: không TLS, nhận mọi địa chỉ, `AUTH PLAIN/LOGIN` nhận mọi tài khoản, tối đa 10 MiB một thư, giữ
`mail.capacity` thư mới nhất (500). BCC được suy ra từ phong bì (`RCPT TO` không có ở header `To`/`Cc`).

### Mail: đọc qua IMAP / POP3

Không TLS (trong client chọn "không mã hoá"). Mỗi địa chỉ nhận là một hộp thư, không cần khai báo: đăng nhập bằng
chính địa chỉ đó, mật khẩu bất kỳ (có admin token thì mật khẩu phải là token). User `*` đọc mọi thư. Đã đọc, cờ và
xoá là trạng thái riêng của từng hộp (lưu trong DB): REST, web UI và hộp khác vẫn thấy thư.

- IMAP4rev1: `CAPABILITY, LOGIN, AUTHENTICATE PLAIN, NAMESPACE, LIST, LSUB, STATUS, SELECT, EXAMINE, FETCH, SEARCH,
  STORE, EXPUNGE, CLOSE, UNSELECT, IDLE` và dạng `UID`. Thư mới báo `EXISTS` ở `NOOP`/`IDLE`. Chỉ có `INBOX`; không
  `APPEND`/`COPY`; không nhận literal `{n}` từ client; `BODYSTRUCTURE` chỉ là bản text/plain (client dựa vào nó để
  tải từng phần sẽ không thấy HTML hay file đính kèm; tải cả thư bằng `BODY[]` thì đủ). `UIDVALIDITY` đổi mỗi lần
  khởi động lại, nên client sẽ đồng bộ lại.
- POP3: `USER, PASS, STAT, LIST, UIDL, RETR, TOP, DELE, RSET, NOOP, CAPA, QUIT` (xoá có hiệu lực khi `QUIT`).

### Xác thực (instance dùng chung)

Mặc định mở hoàn toàn. Đặt `auth.admin_token` (env `MOCKHUB__AUTH__ADMIN_TOKEN`; docker compose: `MOCKHUB_ADMIN_TOKEN`):

| Đường vào | Khi có token |
|---|---|
| REST `/v1/*` | `Authorization: Bearer <token>`, không thì `401` |
| Web UI (trang, server function, file tĩnh) | HTTP Basic: tên bất kỳ, mật khẩu = token; request ghi từ origin khác bị `403` |
| IMAP, POP3 | mật khẩu = token |
| DNS, `/resolve`, `/dns-query`, `/health` | vẫn mở, như một resolver |
| SMTP | vẫn mở (chỉ nhận thư); SMS còn là khung |

Không giao thức nào có TLS, nên token đi dạng văn bản: ra ngoài mạng tin cậy thì đặt sau proxy/VPN có TLS. Proxy trước
web UI phải giữ nguyên header `Host`, nếu không thao tác ghi bị coi là cross-site. Không giới hạn số lần thử: dùng
token ngẫu nhiên dài (vd `openssl rand -hex 32`). `dx serve` (dev) không có xác thực.

### Docker

Image build bằng `cargo` + `dx` (client wasm), **không cần Node** vì `public/app.css` đã commit. Image runtime
`debian:trixie-slim` (glibc phải khớp image build `rust:1`), client nằm ở `/app/public` và `DIOXUS_PUBLIC_PATH` đã đặt sẵn.

```sh
docker compose up --build          # mockhub (SQLite trong volume `mockhub-data`)
docker build -t mockhub . && docker run --rm -p 8686:8686 -p 2525:2525 -p 1143:1143 -p 1110:1110 -p 8081:8081 -p 8053:8053 -p 5353:5353/udp -p 5353:5353/tcp mockhub
```

Container chạy non-root nên dùng cổng cao; muốn cổng chuẩn (25/53/143) thì map ở phía host (`-p 25:2525`).
Nếu cổng host bị chiếm (vd 1143), đổi vế trái của `-p`.
