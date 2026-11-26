use dioxus::prelude::*;

use crate::api;
use crate::app::Route;
use crate::components::{Badge, Button, Card, PageHeader, Stat, Table, TextInput, Tone};

#[component]
pub fn HomePage() -> Element {
    rsx! {
        PageHeader { title: "MockHub", subtitle: "Mock mail, DNS, SMS cho dev/test." }
        div { class: "grid grid-cols-[repeat(auto-fit,minmax(15rem,1fr))] gap-4",
            Stat { title: "Mail", value: "3", desc: "SMTP :2525 · IMAP :1143",
                Link { class: "link link-primary", to: Route::MailPage {}, "Mở hộp thư →" }
            }
            Stat { title: "DNS", value: "3", desc: "UDP :5353",
                Link { class: "link link-primary", to: Route::DnsPage {}, "Quản lý record →" }
            }
            Stat { title: "SMS", value: "3", desc: "API :8081",
                Link { class: "link link-primary", to: Route::SmsPage {}, "Xem tin nhắn →" }
            }
            Stat { title: "API Mock", value: "3", desc: "API :8086",
                Link { class: "link link-primary", to: Route::ApiMockPage {}, "Xem endpoint →" }
            }
        }
    }
}

#[component]
pub fn MailPage() -> Element {
    let rows = [
        ("noreply@app.test", "Xác nhận đăng ký", "10:02", true),
        ("billing@shop.test", "Hóa đơn #1024", "09:40", false),
        ("alice@example.test", "Re: họp sprint", "Hôm qua", false),
    ];
    rsx! {
        PageHeader { title: "Mail", subtitle: "Hộp thư giả lập (SMTP nhận, IMAP đọc)." }
        Card {
            Table { head: vec!["Từ", "Tiêu đề", "Thời gian", ""],
                for (from, subject, at, unread) in rows {
                    tr {
                        td { "{from}" }
                        td { "{subject}" }
                        td { "{at}" }
                        td { if unread { Badge { tone: Tone::Primary, "Mới" } } }
                    }
                }
            }
        }
    }
}

#[component]
pub fn DnsPage() -> Element {
    // `use_server_future`: chạy trên server khi SSR, client hydrate lại từ kết quả đó (không fetch lần 2).
    let mut records = use_server_future(api::list_dns_records)?;
    let mut name = use_signal(String::new);
    let mut value = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);

    rsx! {
        PageHeader { title: "DNS", subtitle: "Zone giả lập, trả lời qua UDP :5353." }
        Card { title: "Thêm record (A)",
            form {
                class: "flex flex-wrap items-center gap-2",
                onsubmit: move |e| async move {
                    e.prevent_default();
                    if name().is_empty() || value().is_empty() {
                        return;
                    }
                    match api::add_dns_record(name(), "A".into(), value()).await {
                        Ok(_) => {
                            name.set(String::new());
                            value.set(String::new());
                            error.set(None);
                            records.restart();
                        }
                        Err(e) => error.set(Some(e.to_string())),
                    }
                },
                TextInput { placeholder: "tên", value: name(), oninput: move |v| name.set(v) }
                TextInput { placeholder: "địa chỉ IP", value: value(), oninput: move |v| value.set(v) }
                Button { r#type: "submit", "Thêm" }
            }
            if let Some(err) = error() {
                div { role: "alert", class: "alert alert-error", "Lỗi: {err}" }
            }
        }
        Card {
            match &*records.read() {
                Some(Ok(rows)) if rows.is_empty() => rsx! {
                    p { class: "opacity-60", "Chưa có record nào. Thêm ở form phía trên." }
                },
                Some(Ok(rows)) => rsx! {
                    Table { head: vec!["Tên", "Loại", "Giá trị", ""],
                        for r in rows.clone() {
                            tr { key: "{r.id}",
                                td { "{r.name}" }
                                td { Badge { outline: true, "{r.kind}" } }
                                td { "{r.value}" }
                                td {
                                    Button {
                                        ghost: true,
                                        small: true,
                                        onclick: move |_| async move {
                                            if api::delete_dns_record(r.id).await.is_ok() {
                                                records.restart();
                                            }
                                        },
                                        "Xóa"
                                    }
                                }
                            }
                        }
                    }
                },
                Some(Err(e)) => rsx! { div { role: "alert", class: "alert alert-error", "Lỗi: {e}" } },
                None => rsx! { span { class: "loading loading-spinner" } },
            }
        }
    }
}

#[component]
pub fn SmsPage() -> Element {
    let rows = [
        (
            "+84901000001",
            "Mã OTP của bạn là 123456",
            "Đã gửi",
            Tone::Success,
        ),
        (
            "+84901000002",
            "Đơn hàng #1024 đang giao",
            "Đang chờ",
            Tone::Warning,
        ),
        (
            "+84901000003",
            "Chào mừng đến MockHub",
            "Đã gửi",
            Tone::Success,
        ),
    ];
    rsx! {
        PageHeader { title: "SMS", subtitle: "Tin nhắn giả lập nhận qua API :8081." }
        Card {
            Table { head: vec!["Số nhận", "Nội dung", "Trạng thái"],
                for (to, body, status, tone) in rows {
                    tr {
                        td { "{to}" }
                        td { "{body}" }
                        td { Badge { tone, "{status}" } }
                    }
                }
            }
        }
    }
}

#[component]
pub fn ApiMockPage() -> Element {
    let rows = [
        ("GET", "/api/users", "200", "Danh sách người dùng", Tone::Success),
        ("POST", "/api/orders", "201", "Tạo đơn hàng", Tone::Success),
        ("GET", "/api/orders/1024", "404", "Không tìm thấy đơn", Tone::Error),
    ];
    rsx! {
        PageHeader { title: "API Mock", subtitle: "Endpoint giả lập trả về response cấu hình sẵn." }
        Card {
            Table { head: vec!["Method", "Đường dẫn", "Status", "Mô tả"],
                for (method, path, status, desc, tone) in rows {
                    tr {
                        td { "{method}" }
                        td { "{path}" }
                        td { Badge { tone: tone, "{status}" } }
                        td { "{desc}" }
                    }
                }
            }
        }
    }
}

#[component]
pub fn NotFound(segments: Vec<String>) -> Element {
    rsx! {
        PageHeader { title: "404", subtitle: format!("Không tìm thấy /{}", segments.join("/")) }
        Link { class: "link link-primary", to: Route::HomePage {}, "← Về trang chủ" }
    }
}
