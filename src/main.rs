fn main() {
    if let Err(error) = pupkit::run(std::env::args().collect()) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
