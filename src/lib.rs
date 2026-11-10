//! Eginbeharraren deia: núcleo del juego (sin gráficos). Lo comparten el juego
//! (`main.rs`) y el servidor dedicado (`bin/eginbeharraren-deia-server.rs`).

pub mod client;
pub mod config;
pub mod geo;
pub mod math;
pub mod net;
pub mod server;
pub mod sfx;
pub mod sim;
pub mod world;
