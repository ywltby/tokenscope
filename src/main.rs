use clap::Parser;

fn main() {
    let cli = tokenscope::cli::Cli::parse();
    if let Err(e) = tokenscope::cli::run(cli) {
        eprintln!("错误: {e:#}");
        std::process::exit(1);
    }
}
