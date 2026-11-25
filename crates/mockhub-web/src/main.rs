//! Entry point cho `dx serve` / `dx build`:
//! - platform web   -> client wasm, hydrate HTML do server render
//! - feature server -> server độc lập (dev). Production chạy qua `mockhub-server`.
#[cfg(feature = "server")]
fn main() {
    dioxus::server::serve(|| async {
        mockhub_web::server::init_from_env().await?;
        Ok(dioxus::server::router(mockhub_web::app::App))
    });
}

#[cfg(not(feature = "server"))]
fn main() {
    dioxus::launch(mockhub_web::app::App);
}
