use dioxus::prelude::*;

use super::port;
use crate::api;
use crate::components::{Badge, Button, Card, PageHeader, Table, TextInput, Tone};
use crate::live::use_live;

const KINDS: [&str; 10] = [
    "A", "AAAA", "CNAME", "TXT", "MX", "NS", "PTR", "SRV", "CAA", "SOA",
];
const ACTIONS: [&str; 5] = ["servfail", "refused", "nxdomain", "timeout", "delay"];

fn rcode_tone(rcode: &str) -> Tone {
    match rcode {
        "NOERROR" => Tone::Success,
        "NXDOMAIN" => Tone::Warning,
        _ => Tone::Error,
    }
}

#[component]
fn Select(
    label: &'static str,
    value: String,
    options: Vec<&'static str>,
    onchange: EventHandler<String>,
) -> Element {
    rsx! {
        select {
            class: "select select-bordered w-auto",
            aria_label: label,
            onchange: move |e| onchange.call(e.value()),
            for option in options {
                option { value: option, selected: value == option, "{option}" }
            }
        }
    }
}

#[component]
fn Failure(message: Signal<Option<String>>) -> Element {
    rsx! {
        if let Some(text) = message() {
            div { role: "alert", class: "alert alert-error", "Lỗi: {text}" }
        }
    }
}

#[component]
pub fn DnsPage() -> Element {
    let live = use_live(api::dns_dashboard)?;
    let overview = use_live(api::overview)?;
    let dns_port = port((overview.data)().map_or(0, |o| o.ports.dns));
    // Lỗi của thao tác ghi, tách khỏi lỗi tự làm mới để không bị xoá sau 2 giây.
    let mut failed = use_signal(|| None::<String>);
    let done = use_callback(move |result: Result<()>| match result {
        Ok(()) => {
            failed.set(None);
            live.refresh.call(());
        }
        Err(e) => failed.set(Some(e.to_string())),
    });

    let Some(data) = (live.data)() else {
        return rsx! {
            PageHeader { title: "DNS", subtitle: "Zone giả lập." }
            Failure { message: live.error }
        };
    };
    rsx! {
        PageHeader { title: "DNS", subtitle: "Zone giả lập: DNS {dns_port} (UDP/TCP), DoH /dns-query và /resolve ở cổng REST." }
        Failure { message: failed }
        Failure { message: live.error }

        Card { title: "Record",
            RecordForm { ondone: done }
            if data.records.is_empty() {
                p { class: "opacity-60", "Chưa có record nào. Thêm ở form phía trên." }
            } else {
                Table { head: vec!["Tên", "Loại", "Giá trị", "TTL", ""],
                    for r in data.records {
                        tr { key: "{r.id}",
                            td { class: "break-all", "{r.name}" }
                            td { Badge { outline: true, "{r.kind}" } }
                            td { class: "break-all font-mono", "{r.value}" }
                            td { "{r.ttl}" }
                            td {
                                Button { ghost: true, small: true,
                                    onclick: move |_| async move { done.call(api::delete_dns_record(r.id).await) },
                                    "Xóa"
                                }
                            }
                        }
                    }
                }
            }
        }

        Card { title: "Lỗi giả lập",
            FaultForm { ondone: done }
            if data.faults.is_empty() {
                p { class: "opacity-60", "Không có lỗi giả lập nào đang bật." }
            } else {
                Table { head: vec!["Tên", "Loại record", "Lỗi", "Trễ (ms)", ""],
                    for f in data.faults {
                        tr { key: "{f.id}",
                            td { class: "break-all", "{f.name}" }
                            td { if f.kind.is_empty() { "mọi loại" } else { "{f.kind}" } }
                            td { Badge { tone: Tone::Warning, "{f.action}" } }
                            td { if f.action == "delay" { "{f.delay_ms}" } }
                            td {
                                Button { ghost: true, small: true,
                                    onclick: move |_| async move { done.call(api::delete_dns_fault(f.id).await) },
                                    "Xóa"
                                }
                            }
                        }
                    }
                }
            }
        }

        Card { title: "Tra thử", LookupForm { onasked: move |()| live.refresh.call(()) } }

        Card { title: "Truy vấn gần đây",
            div {
                Button { ghost: true, small: true,
                    onclick: move |_| async move { done.call(api::clear_dns_queries().await) },
                    "Xóa log"
                }
            }
            if data.queries.is_empty() {
                p { class: "opacity-60", "Chưa có truy vấn nào." }
            } else {
                Table { head: vec!["Lúc (UTC)", "Giao thức", "Tên", "Loại", "Kết quả", "Answer", "Nguồn"],
                    for q in data.queries {
                        tr { key: "{q.id}",
                            td { class: "whitespace-nowrap font-mono", {q.time.get(11..19).unwrap_or(&q.time)} }
                            td { "{q.proto}" }
                            td { class: "break-all", "{q.name}" }
                            td { "{q.kind}" }
                            td { Badge { tone: rcode_tone(&q.rcode), "{q.rcode}" } }
                            td { "{q.answers}" }
                            td { "{q.source}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn RecordForm(ondone: Callback<Result<()>>) -> Element {
    let mut name = use_signal(String::new);
    let mut kind = use_signal(|| "A".to_owned());
    let mut value = use_signal(String::new);
    let mut ttl = use_signal(|| "60".to_owned());
    let mut replace = use_signal(|| false);
    rsx! {
        form {
            class: "flex flex-wrap items-center gap-2",
            onsubmit: move |e| async move {
                e.prevent_default();
                if name().is_empty() {
                    return;
                }
                let result = api::add_dns_record(name(), kind(), value(), ttl().trim().parse().ok(), replace()).await;
                if result.is_ok() {
                    value.set(String::new());
                }
                ondone.call(result);
            },
            TextInput { placeholder: "tên (hỗ trợ *.)", value: name(), oninput: move |v| name.set(v) }
            Select { label: "loại record", value: kind(), options: KINDS.to_vec(), onchange: move |v| kind.set(v) }
            TextInput { placeholder: "giá trị (TXT: chuỗi thô)", value: value(), oninput: move |v| value.set(v) }
            TextInput { placeholder: "TTL", value: ttl(), oninput: move |v| ttl.set(v) }
            label { class: "label cursor-pointer gap-2",
                input {
                    r#type: "checkbox",
                    class: "checkbox checkbox-sm",
                    checked: replace(),
                    onchange: move |e| replace.set(e.checked()),
                }
                "thay bản cùng tên và loại"
            }
            Button { r#type: "submit", "Thêm record" }
        }
    }
}

#[component]
fn FaultForm(ondone: Callback<Result<()>>) -> Element {
    let mut name = use_signal(String::new);
    let mut kind = use_signal(String::new);
    let mut action = use_signal(|| "servfail".to_owned());
    let mut delay = use_signal(|| "1000".to_owned());
    rsx! {
        form {
            class: "flex flex-wrap items-center gap-2",
            onsubmit: move |e| async move {
                e.prevent_default();
                if name().is_empty() {
                    return;
                }
                let delay_ms = if action() == "delay" { delay().trim().parse().unwrap_or(0) } else { 0 };
                ondone.call(api::add_dns_fault(name(), kind(), action(), delay_ms).await);
            },
            TextInput { placeholder: "tên (hỗ trợ *.)", value: name(), oninput: move |v| name.set(v) }
            TextInput { placeholder: "loại (trống = mọi loại)", value: kind(), oninput: move |v| kind.set(v) }
            Select { label: "loại lỗi", value: action(), options: ACTIONS.to_vec(), onchange: move |v| action.set(v) }
            if action() == "delay" {
                TextInput { placeholder: "trễ (ms)", value: delay(), oninput: move |v| delay.set(v) }
            }
            Button { r#type: "submit", "Thêm lỗi" }
        }
    }
}

#[component]
fn LookupForm(onasked: EventHandler<()>) -> Element {
    let mut name = use_signal(String::new);
    let mut kind = use_signal(|| "A".to_owned());
    let mut answer = use_signal(|| None::<String>);
    rsx! {
        form {
            class: "flex flex-wrap items-center gap-2",
            onsubmit: move |e| async move {
                e.prevent_default();
                if name().is_empty() {
                    return;
                }
                answer.set(Some("…".to_owned()));
                let text = match api::dns_lookup(name(), kind()).await {
                    Ok(text) => text,
                    Err(e) => format!("Lỗi: {e}"),
                };
                answer.set(Some(text));
                onasked.call(());
            },
            TextInput { placeholder: "tên cần tra", value: name(), oninput: move |v| name.set(v) }
            Select { label: "loại cần tra", value: kind(), options: KINDS.to_vec(), onchange: move |v| kind.set(v) }
            Button { r#type: "submit", "Tra" }
        }
        if let Some(text) = answer() {
            pre { class: "bg-base-200 rounded-box whitespace-pre-wrap break-all p-3 text-sm", "{text}" }
        }
    }
}
