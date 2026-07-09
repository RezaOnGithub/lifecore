use clap::Parser;

mod graphviz;

// #[derive(Parser)]
// struct Cli {
//     #[subcommand]
//     cmd: Cmd,
// }

// #[derive(Subcommand)]
// enum Cmd {}

fn main() {
    println!("Hello, world!");
}

#[cfg(test)]
#[test]
fn validate_cli() {
    use clap::CommandFactory;
    // Cli::debug_assert();
}
