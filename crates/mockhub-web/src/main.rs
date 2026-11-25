//! Entry point cho `dx serve` / `dx build`:
//! - platform web  -> client wasm, hydrate HTML do server render
//! - feature server -> server độc lập (dev). Production chạy qua `mockhub-server`.
fn main() {
    dioxus::launch(mockhub_web::app::App);
}
