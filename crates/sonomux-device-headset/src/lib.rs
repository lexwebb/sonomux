//! Wireless headset and USB peripheral monitor for Sonomux.

pub mod headset;

pub use headset::{HeadsetMonitor, HeadsetTelemetry};
pub use sonomux_core::traits::*;
