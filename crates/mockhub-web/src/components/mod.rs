//! Bộ component mỏng bọc class của DaisyUI (Tailwind + plugin DaisyUI, build từ `styles/input.css`).
//! Muốn thêm: tra class tại daisyui.com/components rồi bọc lại ở đây cho gọn.
use dioxus::prelude::*;

#[component]
pub fn PageHeader(title: String, subtitle: String) -> Element {
    rsx! {
        div {
            h1 { class: "text-2xl font-bold", "{title}" }
            p { class: "opacity-60", "{subtitle}" }
        }
    }
}

#[component]
pub fn Card(title: Option<String>, children: Element) -> Element {
    rsx! {
        div { class: "card bg-base-100 shadow-sm",
            div { class: "card-body",
                if let Some(t) = title { h2 { class: "card-title", "{t}" } }
                {children}
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Default)]
pub enum Tone {
    #[default]
    Neutral,
    Primary,
    Success,
    Warning,
    Error,
}

impl Tone {
    fn badge(self) -> &'static str {
        match self {
            Tone::Neutral => "badge-ghost",
            Tone::Primary => "badge-primary",
            Tone::Success => "badge-success",
            Tone::Warning => "badge-warning",
            Tone::Error => "badge-error",
        }
    }
}

#[component]
pub fn Badge(#[props(default)] tone: Tone, #[props(default)] outline: bool, children: Element) -> Element {
    let outline = if outline { "badge-outline" } else { "" };
    rsx! { span { class: "badge {tone.badge()} {outline}", {children} } }
}

#[component]
pub fn Button(
    #[props(default)] ghost: bool,
    #[props(default)] small: bool,
    #[props(default = "button")] r#type: &'static str,
    onclick: Option<EventHandler<MouseEvent>>,
    children: Element,
) -> Element {
    let variant = if ghost { "btn-ghost" } else { "btn-primary" };
    let size = if small { "btn-sm" } else { "" };
    rsx! {
        button {
            class: "btn {variant} {size}",
            r#type,
            onclick: move |e| if let Some(h) = onclick { h.call(e) },
            {children}
        }
    }
}

#[component]
pub fn TextInput(placeholder: String, value: String, oninput: EventHandler<String>) -> Element {
    rsx! {
        input {
            class: "input input-bordered",
            placeholder,
            value,
            oninput: move |e| oninput.call(e.value()),
        }
    }
}

#[component]
pub fn Table(head: Vec<&'static str>, children: Element) -> Element {
    rsx! {
        div { class: "overflow-x-auto",
            table { class: "table",
                thead { tr { for h in head { th { "{h}" } } } }
                tbody { {children} }
            }
        }
    }
}

#[component]
pub fn Stat(title: String, value: String, desc: String, children: Element) -> Element {
    rsx! {
        div { class: "card bg-base-100 shadow-sm",
            div { class: "card-body",
                div { class: "stat-title opacity-60", "{title}" }
                div { class: "stat-value", "{value}" }
                div { class: "stat-desc", "{desc}" }
                div { class: "card-actions", {children} }
            }
        }
    }
}
