use clap::Parser;
use color_eyre::eyre::Result;

#[derive(Debug, Parser)]
struct TreeDB {}

impl TreeDB {
    fn run(&self) -> Result<()> {
        println!("{self:#?}");

        Ok(())
    }
}

fn main() {
    color_eyre::install().expect("could not install error handlers");

    let opts = TreeDB::parse();
    if let Err(err) = opts.run() {
        println!("{err:?}");
        std::process::exit(1);
    }
}
