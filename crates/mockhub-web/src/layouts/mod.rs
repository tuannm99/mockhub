use dioxus::prelude::*;

use crate::app::Route;

#[component]
pub fn MainLayout() -> Element {
    rsx! {
        div { class: "navbar bg-base-100 shadow-sm",
            span { class: "px-3 text-lg font-bold", "MockHub" }
            ul { class: "menu menu-horizontal",
                li { Link { to: Route::HomePage {}, active_class: "menu-active", "Home" } }
                li { Link { to: Route::MailPage {}, active_class: "menu-active", "Mail" } }
                li { Link { to: Route::DnsPage {}, active_class: "menu-active", "DNS" } }
                li { Link { to: Route::SmsPage {}, active_class: "menu-active", "SMS" } }
            }
        }
        main { class: "mx-auto flex max-w-5xl flex-col gap-5 px-6 py-8", Outlet::<Route> {} }
    }
}
