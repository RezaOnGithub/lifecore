use lifecore_cli::Cli;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("trace")).init();
    Cli::run()
}
