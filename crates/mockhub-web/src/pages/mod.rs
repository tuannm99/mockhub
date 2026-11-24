use leptos::prelude::*;

#[component]
pub fn HomePage() -> impl IntoView {
    view! { <h1>"MockHub"</h1><p>"Mock mail, DNS, SMS cho dev/test."</p> }
}

#[component]
pub fn MailPage() -> impl IntoView {
    view! { <h1>"Mail"</h1> }
}

#[component]
pub fn DnsPage() -> impl IntoView {
    view! { <h1>"DNS"</h1> }
}

#[component]
pub fn SmsPage() -> impl IntoView {
    view! { <h1>"SMS"</h1> }
}
