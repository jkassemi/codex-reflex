fn main() {
    if let Err(err) = reflex::cli::run(
        std::env::args().skip(1),
        &mut std::io::stdin(),
        &mut std::io::stdout(),
    ) {
        eprintln!("reflex: {err}");
        std::process::exit(1);
    }
}
