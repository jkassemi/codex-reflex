fn main() {
    if let Err(err) = reflex::mcp::run_stdio(&mut std::io::stdin(), &mut std::io::stdout()) {
        eprintln!("reflex-mcp: {err}");
        std::process::exit(1);
    }
}
