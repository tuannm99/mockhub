# How to implement

Hướng dẫn làm từng việc trong repo này. Mọi ví dụ bám code thật đang có (trang DNS là mẫu hoàn chỉnh nhất:
`crates/mockhub-web/src/{api.rs,pages/mod.rs}`, `crates/mockhub-storage/src/{repo.rs,sqlite/mod.rs}`). Các đoạn code dưới
đây là **khung để điền**, chưa được biên dịch trong repo — nếu API thư viện lệch phiên bản thì theo compiler.

Lộ trình: [ROADMAP.md](../ROADMAP.md) · Kiến trúc: mục "Kiến trúc" trong [README.md](../README.md).

## 0. Vòng lặp dev

Cần: Rust stable, `rustup target add wasm32-unknown-unknown`, `dx` (`cargo install dioxus-cli --version '^0.7' --locked`).
Node chỉ cần khi sửa style.

```sh
# Chạy toàn bộ service + web (cần client đã build, xem README "Chạy")
cd crates/mockhub-web && dx build --release --platform web && cd ../.. && cargo run -p mockhub-server

# Chỉ dev UI, hot reload (server dioxus độc lập, không chạy mail/dns/sms)
cd crates/mockhub-web && dx serve --fullstack
# Sửa class Tailwind: terminal khác chạy `npm run css:watch` (xem mục 6)

# Trước khi commit
cargo fmt --all && cargo clippy --workspace --all-targets && cargo test --workspace
```

Dữ liệu dev nằm ở `data/mockhub.db` (gitignore). Xoá thư mục `data/` để reset sạch.

## 1. Thêm dữ liệu mới end-to-end (ví dụ: `SmsMessage`)

Thứ tự: domain → trait → backend + migration → test → server function → UI. Làm đúng thứ tự thì mỗi bước build được.

### 1.1 Domain (`mockhub-core`)

`crates/mockhub-core/src/domain/sms.rs` (rồi `pub mod sms;` trong `domain/mod.rs`):

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmsMessage { pub id: i64, pub to: String, pub body: String, pub status: String }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewSmsMessage { pub to: String, pub body: String }
```

Domain là kiểu thuần: **không** import sqlx/dioxus/axum.

### 1.2 Trait (`mockhub-storage/src/repo.rs`)

```rust
#[async_trait]
pub trait SmsRepo: Send + Sync {
    async fn list_sms(&self) -> Result<Vec<SmsMessage>>;
    async fn create_sms(&self, new: NewSmsMessage) -> Result<SmsMessage>;
}

// Thêm vào supertrait để `Arc<dyn Storage>` có thêm method:
pub trait Storage: DnsRecordRepo + SmsRepo + 'static {}
impl<T: DnsRecordRepo + SmsRepo + 'static> Storage for T {}
```

**Đặt tên method duy nhất giữa các repo** (`list_sms`, không phải `list`): `Storage` gộp nhiều trait, hai trait cùng
có `list` thì `storage.list()` báo lỗi mơ hồ. Các repo hiện có theo quy ước này (`list_dns_records`, `list_mails`).

### 1.3 Migration + backend SQLite

File mới `crates/mockhub-storage/migrations/sqlite/0002_sms_messages.sql`:

```sql
CREATE TABLE sms_messages (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    to_number  TEXT NOT NULL,
    body       TEXT NOT NULL,
    status     TEXT NOT NULL DEFAULT 'queued',
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
```

**Không bao giờ sửa migration đã chạy** (sqlx kiểm tra checksum, sửa là lỗi lúc khởi động). Muốn đổi schema: viết migration
mới `0003_...`. `sqlx::migrate!` nhúng file lúc compile; thêm file mới thì build lại là nhận (nếu không nhận, `touch
crates/mockhub-storage/src/sqlite/mod.rs`).

`impl SmsRepo for SqliteStorage` trong `sqlite/mod.rs`, theo đúng khuôn của `DnsRecordRepo`: dùng `sqlx::query(...)` runtime
(không dùng `query!` vì cần DB lúc compile), `.bind()`, `RETURNING ...` để lấy lại hàng vừa tạo, map lỗi bằng
`StorageError::backend`. `to` là từ khoá SQL nên đặt tên cột `to_number` (map lại khi đọc).

### 1.4 Test

Thêm vào `mod tests` trong `sqlite/mod.rs`, dùng `SqliteStorage::connect("sqlite::memory:")` (mỗi test một DB riêng, nhanh,
không dọn dẹp). Test: tạo → list thấy đúng thứ tự → các trường khớp. Chạy: `cargo test -p mockhub-storage`.

### 1.5 Server function (API cho UI)

Trong `crates/mockhub-web/src/api.rs`: định nghĩa DTO (`#[derive(Serialize, Deserialize, Clone, PartialEq)]`, **không**
import `mockhub-core` ở phần chạy trên client) và hàm:

```rust
#[get("/api/sms/messages")]
pub async fn list_sms() -> Result<Vec<SmsDto>> {
    Ok(crate::server::storage()?.list_sms().await?.into_iter().map(Into::into).collect())
}
```

Quy tắc:
- Thân hàm **chỉ chạy trên server**; trên client macro thay bằng lời gọi HTTP. Nên mọi thứ dùng trong thân (`crate::server`,
  `mockhub_core`, `mockhub_storage`) chỉ có khi bật feature `server` — giữ chúng ở trong thân hàm hoặc sau
  `#[cfg(feature = "server")]`, nếu không build wasm sẽ lỗi.
- Tham số/kết quả phải `Serialize + Deserialize`. Dùng `Result` của `dioxus::prelude` để `?` nhận mọi lỗi chuẩn.
- Chuyển domain → DTO bằng `impl From` đặt sau `#[cfg(feature = "server")]` (như `DnsRecord`).
- URL do bạn đặt (`/api/...`) nhưng **coi là nội bộ của UI**, đổi tự do. Cần contract ổn định cho bên ngoài thì xem mục 5.

### 1.6 Trang UI

Copy `DnsPage` làm khuôn:
- Đọc: `let mut items = use_server_future(api::list_sms)?;` — chạy trên server khi SSR, client hydrate dùng lại kết quả.
  (Dùng `use_resource` nếu không cần SSR.) Sau khi ghi: `items.restart()`. Trang cần tự làm mới thì dùng
  `live::use_live` (mẫu: `pages/mail.rs`): `restart()` đưa component về suspend nên trang chớp mỗi lần.
- Ghi: gọi hàm server trong event handler `async move { api::create_sms(...).await }`.
- Xử lý đủ 3 trạng thái của `items.read()`: `Some(Ok)`, `Some(Err)`, `None` (đang tải); và danh sách rỗng.
- Đăng ký route: thêm biến thể vào enum `Route` ở `app.rs`, link ở `layouts/mod.rs`.

## 2. Triển khai một service (DNS / Mail / SMS / HTTP mock)

Hiện mỗi crate có `pub async fn run(host, cfg)` là khung. Khuôn chung:

1. **Đưa storage vào.** Đổi chữ ký thành `run(host, cfg, storage: Arc<dyn Storage>)`. Trong `apps/mockhub-server/src/main.rs`
   mở storage **một lần** ở `run()` (hiện chỉ `web()` mở) và `.clone()` `Arc` cho từng task:

   ```rust
   let storage = mockhub_storage::connect(&cfg.database.url).await?;
   mockhub_web::server::init(Engine::new(storage.clone()), ports);
   // ...
   tasks.spawn(async move { Ok(mockhub_mail::run(&host, &cfg.mail, storage).await?) });
   ```
2. **Giữ vòng accept mỏng**: accept → `tokio::spawn` mỗi kết nối → hàm xử lý nhận `Arc<dyn Storage>`.
   Không để một kết nối lỗi làm chết cả service: log lỗi rồi tiếp tục (`run` chỉ trả `Err` khi không bind được cổng).
3. **Tách giao thức khỏi I/O** để test không cần mạng: hàm parse/xây response thuần (`&str -> Command`, `Command -> String`),
   rồi lớp mỏng đọc/ghi socket. Test phần thuần bằng unit test, phần socket bằng test tích hợp (mục 4).
4. **Chạy riêng để debug**: `cargo run -p mockhub-server -- mail` (chỉ service đó, cổng theo config).

Chi tiết theo từng service (đúng phạm vi trong [ROADMAP](../ROADMAP.md)) ở các mục 2.1–2.4. Mọi đoạn code là **khung**; tên hàm
của thư viện ngoài (`hickory-server`, `mail-parser`) có thể lệch theo phiên bản — đối chiếu docs.rs của đúng version trong `Cargo.lock`.

### 2.1 DNS — mock mọi case

**Dữ liệu.** Migration mới (không sửa `0001`):

```sql
ALTER TABLE dns_records ADD COLUMN ttl      INTEGER NOT NULL DEFAULT 300;
ALTER TABLE dns_records ADD COLUMN priority INTEGER;            -- MX/SRV

CREATE TABLE dns_rules (        -- luật lỗi: khớp (tên, loại) -> hành vi
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    name_pattern TEXT NOT NULL,           -- 'app.test' hoặc '*.app.test'
    kind         TEXT,                    -- NULL = mọi loại
    action       TEXT NOT NULL,           -- nxdomain|nodata|servfail|refused|drop|truncate
    delay_ms     INTEGER NOT NULL DEFAULT 0,
    percent      INTEGER NOT NULL DEFAULT 100,   -- áp dụng bao nhiêu % query (chập chờn)
    priority     INTEGER NOT NULL DEFAULT 0
);
```

Thêm `DnsRuleRepo` (cùng khuôn mục 1; nhớ đặt tên method duy nhất như `list_dns_rules`). Domain: `DnsRule` + `enum DnsAction`.

**Tách logic quyết định khỏi I/O** — phần dễ lỗi nhất, nên test được mà không cần socket:

```rust
// mockhub-dns/src/decide.rs (hàm thuần)
pub enum Decision {
    Answer(Vec<DnsRecord>),   // có record khớp
    NoData,                   // tên có, nhưng không có loại này
    NxDomain,                 // tên không tồn tại
    ServFail, Refused,
    Drop,                     // im lặng -> client timeout
    Truncate,                 // TC bit -> client thử lại bằng TCP
}
pub struct Plan { pub decision: Decision, pub delay: std::time::Duration }

pub fn decide(name: &str, kind: &str, records: &[DnsRecord], rules: &[DnsRule], roll: u8) -> Plan { /* ... */ }
```

Thứ tự trong `decide`: (1) tìm luật khớp (exact thắng wildcard, rồi theo `priority`), bỏ qua nếu `roll >= percent`
(`roll` là số 0–99 truyền vào để test xác định, runtime lấy ngẫu nhiên); (2) không luật → tra `records`; (3) theo dõi CNAME
(**giới hạn độ sâu ~8, vòng lặp → `ServFail`**); (4) tên có nhưng khác loại → `NoData`, tên hoàn toàn không có → `NxDomain`.
Chuẩn hoá tên trước khi so (không phân biệt hoa/thường, bỏ dấu chấm cuối). Wildcard `*.app.test` khớp `a.app.test` và
`b.a.app.test` nhưng **không** khớp chính `app.test` (đúng ngữ nghĩa DNS thật — viết test cho cả ba).

**Lớp mạng.** Implement handler của `hickory-server` (trait `RequestHandler`): lấy query → `repo.list_dns_records()` +
`repo.list_dns_rules()` → `decide` → `tokio::time::sleep(plan.delay)` → dựng response theo `Decision` (`Drop` = không gửi gì).
Đăng ký **cả UDP lẫn TCP** trên cùng cổng (`:5353`) — bắt buộc để `Truncate` có ý nghĩa; cổng TCP cần thêm vào Docker `EXPOSE 5353/tcp`
và `-p 5353:5353/tcp`. Đổi `kind`/`value` sang `RData` ở **một** hàm chuyển đổi; parse lỗi thì log + `ServFail`, đừng panic.
Validate đúng hàm đó ngay lúc tạo record ở server function để UI báo lỗi sớm.

**Kiểm tra.**
```sh
dig @127.0.0.1 -p 5353 app.test A
dig @127.0.0.1 -p 5353 app.test MX +short
dig @127.0.0.1 -p 5353 big.test TXT +tcp        # đường TCP
dig @127.0.0.1 -p 5353 slow.test A +time=1      # luật delay_ms > 1000 phải timeout
```
Test tự động: unit test cho `decide` (không socket) phủ từng `Decision`; test tích hợp gửi gói thật tới cổng `0` (xem mục 4).

### 2.2 Mail — SMTP nhận, IMAP + POP3 đọc, UI xem đủ

**Dữ liệu** (3 bảng: `mails`, `mail_recipients`, `mail_attachments` — cột trong ROADMAP Phase 2). Điểm dễ sai:
- **BCC không có trong header.** SMTP session giữ envelope: `MAIL FROM` và **mọi** `RCPT TO`. Khi lưu: `to`/`cc` lấy từ header đã parse;
  `bcc` = địa chỉ trong envelope mà **không** xuất hiện ở `To`/`Cc` (so sánh không phân biệt hoa/thường, bỏ phần tên hiển thị).
  Viết hàm thuần `fn bcc(envelope_rcpts: &[String], to: &[String], cc: &[String]) -> Vec<String>` và test riêng.
- Lưu **raw nguyên văn** làm nguồn sự thật (IMAP `FETCH BODY[]` và POP3 `RETR` trả đúng raw); các cột đã parse chỉ để hiển thị/tìm.
- File đính kèm: lưu BLOB trong bảng riêng (đơn giản, một file DB) hoặc cắt từ `raw` khi tải. Giới hạn kích thước mail (vd 25MB) và trả `552` khi vượt.

**SMTP** (`mockhub-mail/src/smtp`): tách `struct Session { helo, mail_from, rcpts, data }` và hàm thuần
`fn handle_line(&mut Session, &str) -> Reply` (test không cần socket). Lưu ý:
- Trạng thái: `EHLO/HELO → MAIL FROM → RCPT TO (nhiều lần) → DATA`; sai thứ tự trả `503`; `RSET` xoá envelope.
- `DATA` kết thúc bằng dòng chỉ có `.` (`\r\n.\r\n`); dòng thân bắt đầu bằng `.` được "dot-stuffing" thành `..` → phải bỏ một dấu `.` khi đọc.
- Chấp nhận mọi địa chỉ nhận (đây là mock); trả `250` sau khi **đã ghi storage** thành công, lỗi storage → `451`.
- Parse bằng `mail-parser` (`MessageParser::default().parse(&raw)`): lấy `subject`, `from`, `to`, `cc`, phần `html`/`text`, danh sách attachment (tên, content-type, bytes). Parse lỗi vẫn **lưu raw** (hiển thị bản thô), đừng làm mất mail.

**IMAP** (`imap/`): lệnh tối thiểu `CAPABILITY, LOGIN, LIST, SELECT, FETCH, STORE, SEARCH, EXPUNGE, LOGOUT`. UID = `mails.id`,
cờ `\Seen` ↔ `read`, `\Deleted` + `EXPUNGE` ↔ xoá. Mỗi dòng lệnh có **tag** của client, response `<tag> OK ...`. Thử:
`curl --url "imap://127.0.0.1:1143/INBOX" -u any:any` (và `;UID=1` để lấy một mail), hoặc `python3 -c "import imaplib..."`.

**POP3** (thêm mới): crate `mockhub-mail/src/pop3/mod.rs`, `run(addr, storage)` giống SMTP/IMAP; thêm vào `tokio::try_join!` ở `lib.rs`.
- Cấu hình: `pop3_port` trong `MailConfig` (core) + `config/default.toml` (`1110`) + bảng cổng README + Docker `EXPOSE`.
- Phiên có 3 pha: AUTHORIZATION (`USER/PASS` — chấp nhận mọi giá trị) → TRANSACTION (`STAT, LIST, UIDL, RETR, TOP, DELE, NOOP, RSET`) → UPDATE (`QUIT`).
- **Chụp danh sách mail lúc đăng nhập** và đánh số 1..N theo snapshot đó; `DELE` chỉ *đánh dấu*, xoá thật khi `QUIT` (`RSET` bỏ đánh dấu).
  `UIDL` = `mails.id` (ổn định giữa các phiên). Response nhiều dòng (`RETR/TOP/LIST/UIDL`) kết thúc bằng `.` và phải dot-stuff dòng bắt đầu bằng `.`.
- Dòng trả lời: `+OK ...` / `-ERR ...`. Thử: `curl pop3://127.0.0.1:1110/ -u a:b` (liệt kê), `curl pop3://127.0.0.1:1110/1 -u a:b` (RETR).

**UI.**
- Danh sách/chi tiết: theo mục 1.6. Chi tiết hiển thị `From`, `To`, `CC`, `BCC` (ghi chú "từ envelope"), tiêu đề, thời gian, tab HTML / Text / Header gốc, danh sách file.
- **HTML mail phải vào `iframe` cô lập, không được nhúng vào DOM của app** (mail có thể chứa script/form/CSS phá trang):

  ```rust
  iframe {
      class: "w-full h-[32rem] bg-white rounded",
      sandbox: "",                               // rỗng = chặn script, form, popup, same-origin
      srcdoc: "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src data:\">{html}",
  }
  ```
  CSP trong `srcdoc` chặn tải tài nguyên từ xa (ảnh theo dõi...) nhưng vẫn giữ style inline. Ảnh `cid:` trong mail cần thay bằng
  `data:` URL hoặc URL tới route đính kèm (bước dưới) trước khi đưa vào `srcdoc`.
- **Tải file đính kèm** cần route axum riêng vì server function chỉ trả JSON. Trong `apps/mockhub-server/src/main.rs::web()`:

  ```rust
  let files = Router::new()
      .route("/mail/{id}/attachments/{idx}", get(download_attachment)) // axum 0.8: path param là {id}
      .with_state(storage.clone());
  let app = dioxus::server::router(App).merge(files);
  ```
  Handler trả byte với `Content-Type` đã lưu, `Content-Disposition: attachment; filename*=UTF-8''<tên đã mã hoá %>`, và
  `X-Content-Type-Options: nosniff`. **Làm sạch tên file** (bỏ `/`, `\`, xuống dòng) để tránh chèn header.

**Test tích hợp (bắt buộc cho phase này).** Gửi một mail có HTML + 1 file + To/CC/BCC qua SMTP thật, rồi: đọc lại bằng IMAP và bằng POP3, so sánh
raw; kiểm tra `bcc()`; kiểm tra bảng recipients; kiểm tra tải file ra đúng byte.

### 2.3 SMS — chỉ nhận và xem

Đơn giản nhất trong bốn service, đừng thêm trạng thái giao/webhook (ngoài phạm vi).
1. Mục 1 với `SmsMessage { id, to_number, body, received_at }` (bảng `sms_messages`); xoá module `delivery`, `webhook` trống.
2. Thay stub TCP trong `mockhub-sms/src/lib.rs` bằng router axum (cùng cách `web()` dùng `axum::serve`):

   ```rust
   pub async fn run(host: &str, cfg: &SmsConfig, storage: Arc<dyn Storage>) -> Result<()> {
       let app = Router::new().route("/sms", post(receive)).with_state(storage);
       let listener = tokio::net::TcpListener::bind(format!("{host}:{}", cfg.port)).await?;
       axum::serve(listener, app).await?;
       Ok(())
   }
   // receive: State<Arc<dyn Storage>>, Json<{ to, body }> -> lưu rồi trả 201 + id
   ```
   Cần thêm `axum`, `serde` vào `mockhub-sms/Cargo.toml`. **Dạng body/URL nên bắt chước provider bạn đang dùng** để app chỉ đổi base URL
   (đây là câu hỏi mở số 3); có thể mount nhiều route cho nhiều provider trên cùng cổng.
3. Kiểm tra: `curl -X POST localhost:8081/sms -H 'content-type: application/json' -d '{"to":"+84901234567","body":"hi"}'`.
4. UI: copy `DnsPage` (danh sách số + nội dung + thời gian, nút xoá, xoá tất cả).

### 2.4 HTTP mock — crate mới `mockhub-http`

**Tạo crate.** `crates/mockhub-http` (workspace tự nhận `crates/*`) → thêm `mockhub-http = { path = ... }` vào `[workspace.dependencies]` →
dependency trong `apps/mockhub-server` → biến thể `Cmd::Http` + nhánh spawn ở `main.rs` → `[http_mock] enabled/port` trong `core::config` và
`config/default.toml` → `EXPOSE` ở Dockerfile và hàng mới trong bảng cổng README. Cần thêm `regex` vào workspace deps cho matcher.

**Mô hình dữ liệu** (domain trong `core`, kiểu enum + `serde` để lưu JSON trong cột TEXT cho gọn schema):

```rust
pub struct HttpStub {
    pub id: i64, pub name: String, pub enabled: bool, pub priority: i32,
    pub request: RequestMatcher, pub response: ResponseSpec,
}
pub struct RequestMatcher {
    pub method: Option<String>,                // None = mọi method
    pub path: PathMatcher,                     // Exact | Prefix | Regex | Pattern("/users/{id}")
    pub query: Vec<(String, StringMatcher)>,   // Equals | Contains | Regex
    pub headers: Vec<(String, StringMatcher)>, // tên header so không phân biệt hoa/thường
    pub body: Option<BodyMatcher>,             // Equals | Contains | Regex | JsonPath(.. = ..)
}
pub struct ResponseSpec { pub status: u16, pub headers: Vec<(String, String)>, pub body: Vec<u8>, pub delay_ms: u64 }
```

Bảng: `http_stubs` (cột scalar + `request_json`, `response_json`) và `http_requests` (nhật ký: `received_at, method, path, query,
headers_json, body, matched_stub_id NULL`). Thêm `HttpStubRepo` (CRUD stub, `append_request`, `list_requests`, `clear_requests`, `reset`).
**Giới hạn nhật ký** (vd giữ 1000 dòng gần nhất, xoá dòng cũ khi ghi) để DB không phình vô hạn.

**Khớp stub — hàm thuần, test kỹ nhất:**

```rust
pub fn pick<'a>(stubs: &'a [HttpStub], req: &IncomingRequest) -> Option<&'a HttpStub>
// lọc enabled -> chỉ giữ stub mà MỌI điều kiện đều khớp -> chọn priority cao nhất, hoà thì id mới hơn
```
Dùng `Pattern("/users/{id}")` bằng cách biên dịch thành regex một lần (cache theo stub id) thay vì biên dịch mỗi request. Regex hỏng thì **từ chối ngay lúc
tạo stub** (validate ở server function/REST), không để lỗi tới lúc có request.

**Server.** Một router axum chỉ có `fallback(handle)` nghe `:8082` (cổng riêng để không đụng đường dẫn của UI):
đọc body (đặt giới hạn kích thước) → dựng `IncomingRequest` → `stubs = repo.list_stubs()` → `pick` → ghi nhật ký (kèm stub khớp hay không) →
`tokio::time::sleep(delay_ms)` → trả response. Không khớp: `404` + JSON gợi ý (path/method nhận được, vài stub gần nhất) để debug nhanh.
Giới hạn của axum: **không** mô phỏng được "đóng kết nối giữa chừng" hay gửi nửa response; nếu cần các lỗi mức TCP đó thì phải hạ xuống `hyper`/socket thô — để sau.

**Cách cấu hình stub** (cùng một repo, hai cửa vào):
- **UI**: server function trong `mockhub-web/src/api.rs` (danh sách, form tạo/sửa, bật/tắt, xem nhật ký) — theo mục 1.5–1.6.
- **REST cho code test** (`mockhub-api`, mục 5): `POST/GET/DELETE /v1/stubs`, `GET/DELETE /v1/requests`, `DELETE /v1/reset`. Đây là chỗ code test
  của bạn tạo stub rồi gọi mock server, nên hợp đồng này phải ổn định (DTO riêng, có version).

**Test.** (1) `pick` thuần: ưu tiên, wildcard path, header khác hoa/thường, body JSON, stub tắt; (2) tích hợp: tạo stub qua `Storage`, gọi mock server bằng `reqwest`,
kiểm tra status/body/header, độ trễ (đo thời gian ≥ `delay_ms`) và nhật ký; (3) request không khớp trả `404` + đúng nội dung gợi ý. Cần `run` nhận `TcpListener`
đã bind (mục 4) để test bind cổng `0`.

**Làm sau (đã cố ý loại ở phạm vi hiện tại):** templating response (`{{request.path.id}}`), kịch bản nhiều bước (lần 1 trả A, lần 2 trả B để test retry), proxy/record-replay.

## 3. Thêm trang / component UI

- **Trang**: hàm `#[component]` trong `pages/mod.rs` + biến thể `Route` + link ở layout.
- **Component dùng lại**: thêm vào `components/mod.rs`, bọc class DaisyUI (tra [daisyui.com/components](https://daisyui.com/components)).
- **Class phải viết nguyên văn** trong chuỗi (`"btn btn-primary"`), đừng ghép chuỗi kiểu `format!("btn-{x}")` — Tailwind chỉ
  thấy class xuất hiện đầy đủ trong source. Cần động thì viết bảng ánh xạ ra các chuỗi nguyên vẹn (xem `Tone::badge`).
- **Sau khi đổi class: build lại CSS** (mục 6), nếu không class mới không có style.

## 4. Test

| Cần test                    | Cách                                                                                                              |
| --------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| Storage / backend           | `#[tokio::test]` + `sqlite::memory:` (mẫu: `dns_crud`). Backend mới phải qua cùng bộ test CRUD.                    |
| Giao thức (SMTP/IMAP/DNS)   | Unit test cho hàm parse thuần; test tích hợp: bind cổng `0` (hệ điều hành chọn cổng trống), kết nối bằng `TcpStream`/`UdpSocket`. |
| Server function / HTTP      | Chạy `mockhub web` rồi `curl` (ví dụ ở README), hoặc test `reqwest` trỏ vào server khởi động trong test.            |
| UI                          | Theo quy ước của dự án: tự kiểm tra trên trình duyệt thật; typecheck/clippy bắt lỗi biên dịch.                     |

Để test tích hợp bind cổng ngẫu nhiên được, hàm `run` nên nhận `TcpListener`/`UdpSocket` đã bind (hoặc trả về cổng thật),
thay vì tự `bind(cfg.port)` bên trong — sửa khi bắt đầu viết test tích hợp.

## 5. REST công khai (`mockhub-api`)

Chỉ làm khi bên ngoài (code test của người khác) cần gọi hub — UI **không** cần (đã có server function).
- Router axum trong `mockhub-api`, prefix `/v1`, nhận `Arc<dyn Storage>` qua `State`; trả JSON bằng DTO riêng của API
  (không trả thẳng kiểu domain, để đổi domain không vỡ contract).
- Mount vào router của `mockhub web`: `dioxus::server::router(App).merge(api_router)` (hoặc `.nest("/v1", ...)`) trong
  `apps/mockhub-server/src/main.rs::web()`.
- Có `DELETE /v1/reset` để test tự dọn dữ liệu (thêm method `clear` vào repo).

## 6. Style (Tailwind + DaisyUI)

`public/app.css` là file **sinh ra nhưng được commit** — máy build không cần Node. Mỗi khi thêm/đổi class trong code Rust:

```sh
cd crates/mockhub-web
npm ci                 # lần đầu
npm run css            # sinh lại public/app.css rồi commit
npm run css:watch      # lúc dev
npm run css:check      # CI/trước commit: lỗi nếu app.css cũ
```

Đổi theme: sửa `themes:` trong `styles/input.css`. **Đừng** dùng `asset!()` cho CSS (xem mục 8).

## 7. Chuyển sang Postgres (khi thật sự cần)

1. `Cargo.toml` workspace: thêm `"postgres"` vào features của `sqlx`.
2. `crates/mockhub-storage/src/postgres/mod.rs`: struct bọc `PgPool`, `impl` **mọi** repo trait, chạy
   `sqlx::migrate!("migrations/postgres")`.
3. `crates/mockhub-storage/migrations/postgres/`: viết lại schema theo Postgres (`BIGSERIAL`, `TIMESTAMPTZ`, tham số `$1` thay `?`).
4. `lib.rs::connect`: thêm nhánh `url.starts_with("postgres:")`.
5. Chạy lại **cùng bộ test CRUD** trên Postgres (cần DB test, ví dụ container) rồi đổi `database.url`.
6. Dữ liệu SQLite cũ không tự chuyển — viết script export/import nếu cần giữ.
Không đụng web/dns/mail/sms vì chúng chỉ biết `dyn Storage`. Nếu phải sửa ngoài `mockhub-storage`, interface đang rò rỉ chi tiết backend — sửa interface.

## 8. Cạm bẫy đã gặp trong repo này

- **`asset!()` + SSR**: server build bằng `cargo` không được `dx` xử lý hậu kỳ nên `asset!()` trả chuỗi lỗi
  ("This should be replaced by dx...") trong HTML SSR. Dùng file trong `crates/mockhub-web/public/` với đường dẫn cố định (`/app.css`).
- **Thiếu client build**: `cargo run` không tự build wasm. Server tìm `DIOXUS_PUBLIC_PATH` → `<exe>/public` →
  `target/dx/.../public`; không thấy thì `mockhub all` bỏ qua web, `mockhub web` báo lỗi (xem README).
- **Feature `server` vs wasm**: `mockhub-core`/`mockhub-storage` là dependency tuỳ chọn của `mockhub-web` (chỉ bật với
  `server`). Dùng chúng ngoài thân server function hoặc `#[cfg(feature = "server")]` sẽ làm build wasm lỗi.
- **`std::env::set_var` là `unsafe` (edition 2024)** và chỉ an toàn trước khi có thread: `main()` đặt env trước khi dựng runtime.
- **glibc**: image build `rust:1` (Debian mới) phải khớp image runtime; hiện cả hai `trixie`. Đổi một bên thì đổi bên kia.
- **Cache Docker**: sửa bất kỳ file nào làm vỡ cache `COPY . .` → build lại server + wasm (vài phút).
- **`cargo clean`** xoá `target/dx` (client build) — chạy lại `dx build` trước khi `cargo run`.
- **Quên build lại CSS**: class mới trong `rsx!` không có style → `npm run css`, commit `app.css`.
