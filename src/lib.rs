//! cascade: NPCs and displays for Pumpkin.

#![allow(non_snake_case)] // the library is named Cascade so the plugin file is Cascade.wasm

pub mod banner;
pub mod color;
pub mod commands;
pub mod http;
pub mod model;
pub mod names;

#[cfg(target_arch = "wasm32")]
mod net;
#[cfg(target_arch = "wasm32")]
mod plugin;
#[cfg(target_arch = "wasm32")]
mod web;
