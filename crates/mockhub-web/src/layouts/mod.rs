use leptos::prelude::*;

#[component]
pub fn MainLayout(children: Children) -> impl IntoView {
    view! {
        <nav>
            <a href="/">"Home"</a>" | "
            <a href="/mail">"Mail"</a>" | "
            <a href="/dns">"DNS"</a>" | "
            <a href="/sms">"SMS"</a>
        </nav>
        <main>{children()}</main>
    }
}
