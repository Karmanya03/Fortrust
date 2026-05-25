fn main() {
    if let Err(error) = fortrust_chrome::run() {
        eprintln!("Fortrust failed to start: {error}");
        std::process::exit(1);
    }
}
