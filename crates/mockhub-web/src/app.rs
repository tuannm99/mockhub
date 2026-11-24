use leptos::prelude::*;
use leptos_meta::{provide_meta_context, MetaTags, Title};
use leptos_router::components::{Route, Router, Routes};
use leptos_router::path;

use crate::layouts::MainLayout;
use crate::pages::{DnsPage, HomePage, MailPage, SmsPage};

/// HTML shell dùng cho SSR (không hydrate).
pub fn shell(_options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="vi">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <MetaTags/>
            </head>
            <body>
                <App/>
            </body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();
    view! {
        <Title text="MockHub"/>
        <Router>
            <MainLayout>
                <Routes fallback=|| "Not found.">
                    <Route path=path!("/") view=HomePage/>
                    <Route path=path!("/mail") view=MailPage/>
                    <Route path=path!("/dns") view=DnsPage/>
                    <Route path=path!("/sms") view=SmsPage/>
                </Routes>
            </MainLayout>
        </Router>
    }
}
