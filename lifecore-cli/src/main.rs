use anyhow;
use lifecore_cli::Cli;

pub fn main() -> anyhow::Result<()> {
    Cli::run()
}
