fn main() {
    // Go passes arguments that are not UTF-8 through as bytes; here they get U+FFFD (rust/diff/cli.md)
    let args: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    let code = docc::cli::run(&args, &mut std::io::stdout(), &mut std::io::stderr());
    std::process::exit(code);
}
