fn main() {
    if let Err(error) = pup_cli_start_rust::run(std::env::args().collect()) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
