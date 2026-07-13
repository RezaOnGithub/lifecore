use clap::Parser;

#[derive(Parser)]
pub enum Cli {}

impl Cli {
    pub fn run() -> anyhow::Result<()> {
        let cli = Self::try_parse()?;

        Ok(())
    }
}
