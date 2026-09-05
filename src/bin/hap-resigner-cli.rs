fn main() {
    match hap_resigner::cli::run_from_env() {
        Ok(true) => {}
        Ok(false) => {
            eprintln!("No command specified. Use --help.");
            std::process::exit(2);
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
