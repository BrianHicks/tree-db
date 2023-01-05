use clap::Parser;
use color::eyre::eyre::Result;

#[derive(Debug, Parser)]
struct Opts {}

fn main() {
    color_eyre::install()?;

    let opts = Opts::parse();

    println!("{opts:#?}");
}
