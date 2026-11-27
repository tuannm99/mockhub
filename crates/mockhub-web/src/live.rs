//! Dữ liệu đọc từ server rồi tự làm mới.
use std::{future::Future, rc::Rc};

use dioxus::fullstack::serde::{Serialize, de::DeserializeOwned};
use dioxus::prelude::*;

const EVERY_MS: u32 = 2000;

pub struct Live<T: 'static> {
    pub data: Signal<Option<T>>,
    pub error: Signal<Option<String>>,
    /// Đọc lại ngay (sau khi ghi, hoặc khi đổi bộ lọc).
    pub refresh: Callback<()>,
}

impl<T> Clone for Live<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Live<T> {}

/// Lần đầu chạy khi SSR (client hydrate lại, không fetch lần 2), sau đó gọi lại mỗi 2 giây.
///
/// Không dùng `Resource::restart()` để làm mới: nó đưa component về trạng thái suspend, trang chớp
/// trắng mỗi lần.
pub fn use_live<T, F, Fut>(fetch: F) -> std::result::Result<Live<T>, RenderError>
where
    T: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
    F: Fn() -> Fut + 'static,
    Fut: Future<Output = Result<T>> + 'static,
{
    let fetch = use_hook(|| Rc::new(fetch));
    let first = {
        let fetch = fetch.clone();
        use_server_future(move || {
            // `peek` bên trong `fetch`: lần tải đầu không được chạy lại theo signal.
            let request = fetch();
            async move { request.await.map_err(|e| e.to_string()) }
        })?
    };
    let mut data = use_signal(|| match &*first.peek() {
        Some(Ok(value)) => Some(value.clone()),
        _ => None,
    });
    let mut error = use_signal(|| match &*first.peek() {
        Some(Err(e)) => Some(e.clone()),
        _ => None,
    });
    let refresh = use_callback(move |()| {
        let request = fetch();
        spawn(async move {
            match request.await {
                Ok(value) => {
                    if data.peek().as_ref() != Some(&value) {
                        data.set(Some(value));
                    }
                    if error.peek().is_some() {
                        error.set(None);
                    }
                }
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    });
    // `use_effect` chỉ chạy ở client, nên SSR không bị vòng lặp này giữ lại.
    use_effect(move || {
        spawn(async move {
            let wait = format!("await new Promise(r => setTimeout(r, {EVERY_MS})); return true;");
            while document::eval(&wait).await.is_ok() {
                refresh.call(());
            }
        });
    });
    Ok(Live {
        data,
        error,
        refresh,
    })
}

/// Chép vào clipboard (cần HTTPS hoặc localhost).
pub fn copy_to_clipboard(text: String) {
    let eval =
        document::eval("const t = await dioxus.recv(); await navigator.clipboard.writeText(t);");
    let _ = eval.send(text);
}

/// Hộp xác nhận của trình duyệt.
pub async fn confirm(question: &str) -> bool {
    let mut eval = document::eval("const q = await dioxus.recv(); dioxus.send(window.confirm(q));");
    if eval.send(question).is_err() {
        return false;
    }
    eval.recv::<bool>().await.unwrap_or(false)
}
