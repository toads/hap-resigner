#[cfg(feature = "agc")]
pub mod agc;
pub mod app;
pub mod cli;
pub mod device;
pub mod hap;
pub mod materials;

pub const APP_NAME: &str = "HAP Resigner";
