//! The browser app's pure parts, built for the host too so `cargo test`
//! covers them. The wasm shell in `main.rs` drives them.

pub mod draw;
pub mod gait;
pub mod input;
pub mod probe;
pub mod ui;
