use clap::Parser;

fn main() {
    // WorkerGuard 与进程同生命周期；错误同时落 ~/.tokenscope/logs/ 便于排障。
    let _log_guard = tokenscope::logging::init("cli");
    let cli = tokenscope::cli::Cli::parse();
    if let Err(e) = tokenscope::cli::run(cli) {
        tokenscope::logging::log_error("命令执行失败", &e);
        eprintln!("错误: {e:#}");
        std::process::exit(1);
    }
}
