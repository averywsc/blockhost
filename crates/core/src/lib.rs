//! BlockHost core: everything needed to turn a spare PC or old laptop into a 24/7 Minecraft server.
//! UI shells (Tauri desktop app, CLI, background service) sit on top of this crate; nothing here knows about UI.

pub mod backup;
pub mod config;
pub mod events;
pub mod hardware;
pub mod java;
pub mod manager;
pub mod net;
pub mod paper;
pub mod paths;
pub mod power;
pub mod server;
pub mod service;
pub mod setup;
pub mod supervisor;
pub mod tunnel;
