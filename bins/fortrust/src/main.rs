use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    let _datadir = parse_arg(&args, "--datadir").or_else(|| parse_arg(&args, "-d"));
    let _no_sandbox = args.iter().any(|a| a == "--no-sandbox");
    let headless = args.iter().any(|a| a == "--headless");

    if args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!("Fortrust Browser v{}", env!("CARGO_PKG_VERSION"));
        eprintln!("Usage: fortrust [OPTIONS]");
        eprintln!();
        eprintln!("Options:");
        eprintln!("  --datadir, -d <path>   Set user data directory (default: platform default)");
        eprintln!("  --no-sandbox           Disable subprocess sandboxing (not recommended)");
        eprintln!("  --headless             Run without GUI window (for testing)");
        eprintln!("  --help, -h             Print this help message");
        eprintln!("  --version, -v          Print version information");
        return;
    }

    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("Fortrust v{}", env!("CARGO_PKG_VERSION"));
        return;
    }

    if headless {
        eprintln!("Headless mode not yet implemented");
        std::process::exit(0);
    }

    if let Err(error) = fortrust_chrome::run() {
        eprintln!("Fortrust failed to start: {error}");
        std::process::exit(1);
    }
}

fn parse_arg(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find_map(|pair| {
        if pair[0] == name {
            Some(pair[1].clone())
        } else {
            None
        }
    })
}
