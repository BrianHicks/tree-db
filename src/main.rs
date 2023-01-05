use clap::Parser;

#[derive(Debug, Parser)]
struct Opts {}

fn main() {
    let opts = Opts::parse();

    println!("{opts:#?}");
}
