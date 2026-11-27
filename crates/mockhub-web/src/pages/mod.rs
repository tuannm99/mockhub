mod dns;
mod mail;

use dioxus::prelude::*;

pub use dns::DnsPage;
pub use mail::MailPage;

use crate::api;
use crate::app::Route;
use crate::components::{Badge, Card, PageHeader, Stat, Table, Tone};
use crate::live::use_live;

/// `:1025`, hoặc `tắt`.
fn port(number: u16) -> String {
    if number == 0 {
        "tắt".to_owned()
    } else {
        format!(":{number}")
    }
}

#[component]
pub fn HomePage() -> Element {
    let live = use_live(api::overview)?;
    let o = (live.data)().unwrap_or_default();
    let mail_ports = format!(
        "SMTP {} · IMAP {} · POP3 {}",
        port(o.ports.smtp),
        port(o.ports.imap),
        port(o.ports.pop3)
    );
    let dns_ports = format!("DNS {} · {} lỗi giả lập", port(o.ports.dns), o.dns_faults);
    rsx! {
        PageHeader { title: "MockHub", subtitle: "Mock mail, DNS, SMS cho dev/test." }
        div { class: "grid grid-cols-[repeat(auto-fit,minmax(15rem,1fr))] gap-4",
            Stat { title: "Mail", value: "{o.mails}", desc: mail_ports,
                Link { class: "link link-primary", to: Route::MailPage {}, "Mở hộp thư →" }
            }
            Stat { title: "DNS record", value: "{o.dns_records}", desc: dns_ports,
                Link { class: "link link-primary", to: Route::DnsPage {}, "Quản lý record →" }
            }
            Stat { title: "SMS", value: "-", desc: "chưa làm (trang là dữ liệu mẫu)",
                Link { class: "link link-primary", to: Route::SmsPage {}, "Xem mẫu →" }
            }
            Stat { title: "API Mock", value: "-", desc: "chưa làm (trang là dữ liệu mẫu)",
                Link { class: "link link-primary", to: Route::ApiMockPage {}, "Xem mẫu →" }
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
        (
            "GET",
            "/api/users",
            "200",
            "Danh sách người dùng",
            Tone::Success,
        ),
        ("POST", "/api/orders", "201", "Tạo đơn hàng", Tone::Success),
        (
            "GET",
            "/api/orders/1024",
            "404",
            "Không tìm thấy đơn",
            Tone::Error,
        ),
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
