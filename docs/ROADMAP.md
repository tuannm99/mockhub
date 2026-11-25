# Roadmap

Trạng thái: ✅ xong · 🟡 khung/đang dở · ⬜ chưa làm. Cách làm từng bước: [docs/how-to-implement.md](docs/how-to-implement.md).
Kiến trúc tổng quan: mục "Kiến trúc" trong [README.md](README.md).

## Hiện trạng (đã xong)

- ✅ Workspace nhiều crate, một binary `mockhub` (`all|web|mail|dns|sms`), config TOML + env override.
- ✅ Web UI Dioxus fullstack (SSR + hydrate), server function làm API cho UI.
- ✅ Style Tailwind v4 + DaisyUI v5, `public/app.css` commit sẵn (build không cần Node).
- ✅ Storage: trait `Storage` + backend SQLite + migration nhúng; mới có bảng `dns_records`.
- ✅ Trang DNS chạy đủ vòng: form → server function → storage → bảng.
- ✅ Docker (multi-stage, runtime `trixie-slim`, SQLite ở `/data` volume), `docker compose`.
- 🟡 Mail / DNS / SMS: chỉ là khung — bind cổng, trả banner/501, chưa xử lý thật.
- 🟡 Trang Mail, SMS, số liệu ở Home: dữ liệu mẫu cứng (chưa nối storage).

## Phạm vi đã chốt

| Service | Phải làm được                                                                                                     | Cố ý **không** làm              |
| ------- | ----------------------------------------------------------------------------------------------------------------- | ------------------------------- |
| DNS     | Mock **mọi case DNS**: mọi loại record phổ biến + các tình huống lỗi/chậm/cắt gói (xem Phase 1)                    | Recursive resolver đầy đủ       |
| Mail    | Xem mail **có style** (HTML), thấy người gửi / To / CC / **BCC**, **file đính kèm**; đọc qua **IMAP và POP3**      | Gửi mail ra ngoài, spam filter  |
| SMS     | Xem **số điện thoại + nội dung** tin đã nhận                                                                       | Trạng thái giao, webhook, retry |
| HTTP    | **HTTP mock**: khai báo stub (request khớp → response mong muốn), có độ trễ/lỗi, xem lại request đã nhận (kiểu WireMock) | Proxy/record-replay (làm sau)   |

Thứ tự làm: DNS → Mail → SMS → HTTP mock (đúng thứ tự ưu tiên bạn đưa ra), rồi Phase 5.

## Phase 1 — DNS đủ để mock mọi case

Mục tiêu: `dig @127.0.0.1 -p 5353 <tên> <loại>` cho ra đúng tình huống đã cấu hình trên UI/DB.

**Loại record** (mở rộng `kind`, thêm `ttl`, `priority` cho MX/SRV):
- ⬜ `A`, `AAAA`, `CNAME`, `MX`, `TXT`, `NS`, `PTR`, `SRV`, `CAA`, `SOA`.
- ⬜ Nhiều record cùng tên + loại (round-robin), TTL tuỳ chỉnh, chuỗi CNAME (và vòng lặp CNAME để test client).
- ⬜ Wildcard (`*.app.test`), tên có ký tự hoa/thường, reverse (`in-addr.arpa`) cho PTR.

**Tình huống lỗi/bất thường** (bảng luật `dns_rules`: khớp tên [exact/wildcard] + loại → hành vi):
- ⬜ `NXDOMAIN`, `NODATA` (tên có nhưng không có loại đó), `SERVFAIL`, `REFUSED`.
- ⬜ Không trả lời (timeout), trả lời **chậm** (`delay_ms`), trả lời chập chờn (tỉ lệ lỗi %).
- ⬜ Cắt gói (**TC bit**) để client chuyển sang TCP → cần nghe cả **TCP lẫn UDP** (hiện chỉ có UDP).
- ⬜ Gói lỗi cú pháp (malformed) để test client xử lý (tuỳ chọn).
- ⬜ Tên không có trong DB: mặc định `NXDOMAIN` (quyết định forward hay không: xem câu hỏi mở).

**Kỹ thuật**
- ⬜ `mockhub-dns` nhận `Arc<dyn Storage>`, tra `DnsRecordRepo` + `DnsRuleRepo` mỗi query (SQLite local đủ nhanh); dùng `hickory-server`.
- ⬜ Validate `kind`/`value` khi tạo (parse thử thành `RData`), báo lỗi rõ cho UI.
- ⬜ UI: bảng record (đã có, mở rộng cột TTL/priority) + trang quản lý luật lỗi; log query gần đây (tên, loại, hành vi đã áp) để debug.
- ⬜ Test: tạo record/luật qua `Storage`, gửi query UDP/TCP thật, kiểm tra response và độ trễ.

## Phase 2 — Mail (SMTP nhận; IMAP + POP3 đọc; UI xem đủ)

Mục tiêu: app gửi SMTP tới `:2525`; đọc được bằng client IMAP (`:1143`) **hoặc** POP3 (`:1110`, mới); UI hiển thị đúng như một hộp thư.

**Lưu trữ** (migration mới):
- ⬜ `mails`: `message_id`, `envelope_from`, `from`, `subject`, `raw` (nguyên văn), `text`, `html`, `received_at`, `read`.
- ⬜ `mail_recipients`: `mail_id`, `kind` (`to`/`cc`/`bcc`), `address`. **BCC không có trong header** — suy ra từ envelope
  (`RCPT TO` trừ các địa chỉ đã thấy ở `To`/`Cc`), nên SMTP phải ghi lại envelope, không chỉ parse header.
- ⬜ `mail_attachments`: `mail_id`, `filename`, `content_type`, `size`, nội dung (BLOB hoặc cắt ra từ `raw` khi tải).

**Giao thức**
- ⬜ SMTP: `EHLO/HELO, MAIL FROM, RCPT TO, DATA, RSET, NOOP, QUIT`; parse bằng `mail-parser`; giới hạn kích thước.
- ⬜ IMAP tối thiểu cho client thật: `CAPABILITY, LOGIN, LIST, SELECT, FETCH, STORE, SEARCH(đơn giản), EXPUNGE, LOGOUT`.
- ⬜ **POP3 (mới)**: `USER, PASS, STAT, LIST, UIDL, RETR, TOP, DELE, NOOP, RSET, QUIT` (xoá có hiệu lực khi `QUIT`).
  Thêm `pop3_port` vào `MailConfig` + `config/default.toml` + bảng cổng README (đề xuất `1110`, cùng kiểu với IMAP `1143`).
- ⬜ IMAP và POP3 cùng đọc **một kho** (`MailRepo`), không có bản sao riêng.

**UI**
- ⬜ Danh sách: người gửi, tiêu đề, thời gian, chưa đọc, biểu tượng đính kèm.
- ⬜ Chi tiết: người gửi, To / CC / **BCC** (ghi chú rõ nguồn từ envelope), tiêu đề, **nội dung HTML có style**, bản text, header gốc, danh sách file đính kèm kèm nút tải.
- ⬜ **An toàn khi hiển thị HTML**: render trong `<iframe sandbox srcdoc=...>` (không script, không submit form), không nhúng HTML mail vào DOM của UI; tuỳ chọn chặn ảnh từ xa.
- ⬜ Tải file đính kèm cần **route axum riêng** (trả byte + `Content-Disposition`), vì server function chỉ trả JSON; merge vào router của `mockhub web`.
- ⬜ Xoá mail, đánh dấu đã đọc, tự làm mới danh sách.
- ⬜ Test: gửi mail (có HTML + file đính kèm + CC/BCC) qua SMTP thật, đọc lại bằng cả IMAP lẫn POP3, so sánh nội dung.

## Phase 3 — SMS (thu gọn)

Mục tiêu: nhận SMS qua API, xem được **số điện thoại + nội dung**. Không trạng thái giao, không webhook.

- ⬜ `core::domain::sms`: `SmsMessage { id, to_number, body, received_at }`; repo + migration (`sms_messages`).
- ⬜ HTTP API ở `:8081`: một endpoint nhận tin (`POST`, JSON gồm số + nội dung); chọn dạng body bắt chước provider đang dùng (xem câu hỏi mở).
- ⬜ Trang SMS: danh sách số + nội dung + thời gian, xoá một/tất cả, tự làm mới.
- ⬜ Dọn crate: bỏ module `delivery`, `webhook` trống trong `mockhub-sms` (ngoài phạm vi), bỏ phần "trạng thái" khỏi UI mẫu.
- ⬜ Test: gọi API → kiểm tra tin xuất hiện trong storage.

## Phase 4 — HTTP mock (crate mới `mockhub-http`)

Mục tiêu: app của bạn gọi `http://localhost:8082/...` và nhận response bạn đã khai báo; xem lại request đã gọi để assert.

**Mô hình** (kiểu WireMock):
- ⬜ **Stub** = điều kiện khớp + response. Khớp theo: method, đường dẫn (exact / prefix / regex / có tham số `{id}`), query, header, body (exact / chứa / JSON path).
- ⬜ Response: status, header, body (text/JSON/file), **độ trễ** (`delay_ms`), tuỳ chọn lỗi (đóng kết nối giữa chừng, response rỗng, chậm dần).
- ⬜ Ưu tiên khi nhiều stub cùng khớp (`priority`, mới hơn thắng); không stub nào khớp → `404` kèm gợi ý stub gần nhất.
- ⬜ (Tuỳ chọn) Template đơn giản trong response (lấy `{{request.path.id}}`, `{{request.body.x}}`), và **kịch bản** nhiều bước (lần gọi 1 trả A, lần 2 trả B) để test retry.
- ⬜ **Nhật ký request**: lưu mọi request đã nhận (kèm stub nào khớp / không khớp) để xem và assert.

**Cách cấu hình**
- ⬜ Mock server nghe **cổng riêng** (đề xuất `:8082`) để không đụng đường dẫn với UI; thêm `[http_mock]` vào config + bảng cổng.
- ⬜ UI: danh sách stub, form tạo/sửa (khớp + response), bật/tắt, nhật ký request.
- ⬜ **REST admin cho code test** (`/v1/stubs`, `/v1/requests`, `DELETE /v1/reset`) — test cần tạo stub bằng code mà không bấm UI.
  Đây là lúc `mockhub-api` có lý do tồn tại (xem Phase 5 / câu hỏi mở).
- ⬜ Test: tạo stub qua `Storage`, gọi mock server bằng `reqwest`, kiểm tra response, độ trễ và nhật ký.

## Phase 5 — Chất lượng & vận hành

- ⬜ **`mockhub-api`**: nếu làm REST admin cho HTTP mock (Phase 4) thì crate này được dùng; ngược lại xoá.
- ⬜ CI (build, `cargo test`, `clippy -D warnings`, `fmt --check`, `npm run css:check`, build Docker).
- ⬜ Test tích hợp: chạy server thật trên cổng ngẫu nhiên, đi qua SMTP/DNS/HTTP.
- ⬜ Postgres backend (khi cần): xem "Chuyển sang Postgres" trong [how-to](docs/how-to-implement.md).
- ⬜ Quan sát: log có cấu trúc, `/healthz`, (tuỳ chọn) metrics.
- ⬜ Xác thực UI/API nếu hub chạy ngoài máy dev.
- ⬜ Dọn nợ kỹ thuật (bên dưới).

## Nợ kỹ thuật đã biết

- Số liệu ở Home (`"3"`), trang Mail và SMS là dữ liệu mẫu cứng trong `pages/mod.rs`.
- `Routable` derive báo cảnh báo `unreachable call` (từ macro Dioxus, không phải lỗi của mình).
- Workspace deps thừa gây cảnh báo (`toml`, `mockhub-api` nếu không dùng) — dọn khi chốt các quyết định mở.
- `web()` tự mở storage; khi `mail/dns/sms` cần storage thì nên mở **một lần** ở `run()` rồi truyền `Arc<dyn Storage>` xuống.
- `dx` lần đầu tải `wasm-bindgen`/`wasm-opt` từ GitHub: mạng private cần mirror hoặc cài sẵn.
- Docker build chậm (compile server + wasm); cache dựa vào `cargo-chef`, đổi `Cargo.toml` là build lại nhiều.

## Câu hỏi mở (cần quyết định trước khi làm phase tương ứng)

1. **DNS**: tên không có record thì `NXDOMAIN` hay forward lên resolver thật? Có cần DNSSEC không (đề xuất: không)? (Phase 1)
2. **Mail**: nhận mọi địa chỉ vào một hộp chung, hay hộp theo địa chỉ nhận? Có xác thực SMTP/IMAP/POP3 không (đề xuất: chấp nhận mọi user/pass)? Giới hạn kích thước mail/file đính kèm? (Phase 2)
3. **SMS**: bắt chước body của provider nào để client đổi URL là dùng được? (Phase 3)
4. **HTTP mock**: cổng riêng `:8082` có ổn không? Có cần proxy/record-replay không (hiện loại ra)? Khớp body mức nào là đủ (JSON path/regex)? (Phase 4)
5. **`mockhub-api`**: dùng cho REST admin của HTTP mock (khuyến nghị) hay xoá crate? (Phase 4/5)
6. **Postgres**: có thật sự cần không? SQLite đủ cho một hub chạy một instance. (Phase 5)
