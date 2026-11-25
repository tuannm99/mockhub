use dioxus::prelude::*;

use crate::layouts::MainLayout;
use crate::pages::{DnsPage, HomePage, MailPage, NotFound, SmsPage};

#[derive(Clone, Debug, PartialEq, Routable)]
pub enum Route {
    #[layout(MainLayout)]
    #[route("/")]
    HomePage {},
    #[route("/mail")]
    MailPage {},
    #[route("/dns")]
    DnsPage {},
    #[route("/sms")]
    SmsPage {},
    #[route("/:..segments")]
    NotFound { segments: Vec<String> },
}

#[component]
pub fn App() -> Element {
    rsx! {
        document::Title { "MockHub" }
        document::Link { rel: "stylesheet", href: "/app.css" }
        div { class: "min-h-screen bg-base-200 text-base-content", Router::<Route> {} }
    }
}
