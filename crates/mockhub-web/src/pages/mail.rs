use dioxus::prelude::*;

use super::port;
use crate::api::{self, MailDetail, MailRow};
use crate::components::{Badge, Button, Card, PageHeader, Tone};
use crate::live::{confirm, copy_to_clipboard, use_live};

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Text,
    Html,
    Raw,
    Headers,
}

/// `2026-11-27T10:05:09.123Z` thành `2026-11-27 10:05:09`.
fn when(rfc3339: &str) -> String {
    rfc3339.get(..19).unwrap_or(rfc3339).replace('T', " ")
}

#[component]
pub fn MailPage() -> Element {
    let mut filter = use_signal(String::new);
    let list = use_live(move || api::list_mails(filter.peek().clone()))?;
    let overview = use_live(api::overview)?;
    let mut selected = use_signal(|| None::<MailDetail>);
    let mut failed = use_signal(|| None::<String>);

    let open = use_callback(move |id: i64| {
        spawn(async move {
            match api::open_mail(id).await {
                Ok(mail) => {
                    selected.set(mail);
                    failed.set(None);
                    list.refresh.call(());
                }
                Err(e) => failed.set(Some(e.to_string())),
            }
        });
    });

    let ports = (overview.data)().unwrap_or_default().ports;
    let subtitle = format!(
        "Thư app gửi tới SMTP {}. Đọc bằng mail client: IMAP {} / POP3 {}, đăng nhập bằng địa chỉ nhận (hoặc * để đọc mọi thư).",
        port(ports.smtp),
        port(ports.imap),
        port(ports.pop3)
    );
    let rows = (list.data)().unwrap_or_default();
    let selected_id = selected.read().as_ref().map(|m| m.id);
    rsx! {
        PageHeader { title: "Mail", subtitle }
        if let Some(text) = failed().or_else(|| (list.error)()) {
            div { role: "alert", class: "alert alert-error", "Lỗi: {text}" }
        }
        div { class: "flex flex-wrap items-center gap-2",
            input {
                class: "input input-bordered grow",
                placeholder: "Lọc theo người gửi, người nhận, tiêu đề, nội dung…",
                value: "{filter}",
                oninput: move |e| {
                    filter.set(e.value());
                    list.refresh.call(());
                },
            }
            Button { ghost: true,
                onclick: move |_| async move {
                    if !confirm("Xóa toàn bộ thư trong mock?").await {
                        return;
                    }
                    match api::clear_mails().await {
                        Ok(()) => {
                            selected.set(None);
                            list.refresh.call(());
                        }
                        Err(e) => failed.set(Some(e.to_string())),
                    }
                },
                "Xóa hết"
            }
        }
        div { class: "grid items-start gap-4 lg:grid-cols-[22rem_minmax(0,1fr)]",
            Card {
                if rows.is_empty() {
                    p { class: "opacity-60", if filter().is_empty() { "Chưa có thư nào." } else { "Không có thư nào khớp." } }
                } else {
                    ul { class: "menu max-h-[70vh] w-full flex-nowrap overflow-y-auto p-0",
                        for row in rows {
                            Row { key: "{row.id}", active: selected_id == Some(row.id), row, onopen: open }
                        }
                    }
                }
            }
            Card {
                match selected() {
                    Some(mail) => rsx! { Message { key: "{mail.id}", mail } },
                    None => rsx! { p { class: "opacity-60", "Chọn một thư để đọc." } },
                }
            }
        }
    }
}

#[component]
fn Row(row: MailRow, active: bool, onopen: Callback<i64>) -> Element {
    let weight = if row.read { "" } else { "font-bold" };
    let active = if active { "menu-active" } else { "" };
    let recipients = row.recipients.join(", ");
    let id = row.id;
    rsx! {
        li { class: "w-full",
            button {
                r#type: "button",
                class: "{active} flex flex-col items-start gap-0.5 text-left",
                onclick: move |_| onopen.call(id),
                span { class: "flex w-full items-center gap-2",
                    span { class: "{weight} grow truncate", if row.subject.is_empty() { "(không tiêu đề)" } else { "{row.subject}" } }
                    if !row.read { Badge { tone: Tone::Primary, "Mới" } }
                }
                span { class: "w-full truncate text-xs opacity-70", "{row.from} → {recipients}" }
                span { class: "w-full truncate text-xs opacity-50", "{row.snippet}" }
                span { class: "text-xs opacity-50", {when(&row.received_at)} " UTC" }
            }
        }
    }
}

#[component]
fn Addresses(label: &'static str, list: Vec<String>) -> Element {
    rsx! {
        if !list.is_empty() {
            dt { class: "opacity-60", "{label}" }
            dd { class: "flex flex-wrap gap-1",
                for address in list {
                    Badge { outline: true, "{address}" }
                }
            }
        }
    }
}

#[component]
fn Message(mail: MailDetail) -> Element {
    let has_html = mail.html.is_some();
    let mut tab = use_signal(move || if has_html { Tab::Html } else { Tab::Text });
    let tabs = [
        (Tab::Html, "HTML"),
        (Tab::Text, "Văn bản"),
        (Tab::Raw, "Bản gốc"),
        (Tab::Headers, "Header"),
    ];
    let preformatted = "bg-base-200 rounded-box whitespace-pre-wrap break-all p-3 text-sm";
    rsx! {
        h2 { class: "card-title break-all", if mail.subject.is_empty() { "(không tiêu đề)" } else { "{mail.subject}" } }
        dl { class: "grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1 text-sm",
            dt { class: "opacity-60", "Từ" }
            dd { class: "break-all", "{mail.from}" }
            Addresses { label: "Tới", list: mail.to.clone() }
            Addresses { label: "CC", list: mail.cc.clone() }
            Addresses { label: "BCC (từ phong bì SMTP)", list: mail.bcc.clone() }
            dt { class: "opacity-60", "Lúc" }
            dd { {when(&mail.received_at)} " UTC · #{mail.id}" }
        }
        if !mail.links.is_empty() {
            div { class: "bg-base-200 rounded-box flex flex-col gap-1 p-3",
                span { class: "text-xs uppercase opacity-60", "Liên kết trong thư" }
                for link in mail.links.clone() {
                    div { class: "flex items-center gap-2",
                        code { class: "grow truncate text-sm", title: "{link}", "{link}" }
                        Button { ghost: true, small: true,
                            onclick: {
                                let link = link.clone();
                                move |_| copy_to_clipboard(link.clone())
                            },
                            "Sao chép"
                        }
                        a { class: "btn btn-sm", href: "{link}", target: "_blank", rel: "noopener noreferrer", "Mở" }
                    }
                }
            }
        }
        div { role: "tablist", class: "tabs tabs-border",
            for (value, label) in tabs {
                if value != Tab::Html || has_html {
                    button {
                        r#type: "button",
                        role: "tab",
                        class: if tab() == value { "tab tab-active" } else { "tab" },
                        onclick: move |_| tab.set(value),
                        "{label}"
                    }
                }
            }
        }
        match tab() {
            // HTML của thư là nội dung không tin được: `sandbox` rỗng (không script, không form,
            // origin riêng), không bao giờ chèn vào DOM của UI.
            Tab::Html => rsx! {
                iframe {
                    class: "rounded-box min-h-[28rem] w-full bg-white",
                    title: "Nội dung HTML của thư",
                    "sandbox": "",
                    srcdoc: mail.html.clone().unwrap_or_default(),
                }
            },
            Tab::Text => rsx! { pre { class: preformatted, "{mail.text}" } },
            Tab::Raw => rsx! { pre { class: preformatted, "{mail.raw}" } },
            Tab::Headers => rsx! { pre { class: preformatted, "{mail.headers}" } },
        }
    }
}
