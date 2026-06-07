mod headless;

use std::env;
use std::path::Path;

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
        eprintln!("  --datadir, -d <path>    Set user data directory (default: platform default)");
        eprintln!("  --no-sandbox            Disable subprocess sandboxing (not recommended)");
        eprintln!("  --headless              Run without GUI window (for testing)");
        eprintln!("    --url <URL>           Page to render (default: https://example.com)");
        eprintln!("    --output <PATH>       PNG output path (default: screenshot.png)");
        eprintln!("    --width <PX>          Viewport width in pixels (default: 1024)");
        eprintln!("    --height <PX>         Viewport height in pixels (default: 768)");
        eprintln!("  --help, -h              Print this help message");
        eprintln!("  --version, -v           Print version information");
        return;
    }

    if args.iter().any(|a| a == "--version" || a == "-v") {
        println!("Fortrust v{}", env!("CARGO_PKG_VERSION"));
        return;
    }

    if headless {
        tracing_subscriber::fmt().with_env_filter("info").init();

        let url = parse_arg(&args, "--url").unwrap_or_else(|| "https://example.com".into());
        let output = parse_arg(&args, "--output").unwrap_or_else(|| "screenshot.png".into());
        let width: u32 = parse_arg(&args, "--width").and_then(|v| v.parse().ok()).unwrap_or(1024);
        let height: u32 = parse_arg(&args, "--height").and_then(|v| v.parse().ok()).unwrap_or(768);

        if let Err(e) = headless::run(&url, Path::new(&output), width, height) {
            eprintln!("Headless mode failed: {e}");
            std::process::exit(1);
        }
        return;
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
