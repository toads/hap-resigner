#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    run_default();
}

#[cfg(feature = "app")]
fn run_default() {
    if let Err(error) = hap_resigner::app::gui::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(not(feature = "app"))]
fn run_default() {
    println!("{} GUI requires --features app", hap_resigner::APP_NAME);
}
