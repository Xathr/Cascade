//! cascade: NPCs and displays for Pumpkin.

#![allow(non_snake_case)] // the library is named Cascade so the plugin file is Cascade.wasm

pub mod commands;
pub mod model;
pub mod names;

#[cfg(target_arch = "wasm32")]
mod plugin;
