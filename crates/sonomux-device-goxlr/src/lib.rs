//! TC-Helicon GoXLR and GoXLR Mini device driver for Sonomux.

pub mod client;
pub mod types;

pub use client::{GoXLRClient, DEFAULT_GOXLR_SOCKET};
pub use sonomux_core::traits::*;
pub use types::*;
