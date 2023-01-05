use clap::Parser;
use color_eyre::eyre::Result;
use std::path::PathBuf;

#[derive(Debug, Parser)]
enum Command {
    /// Compile a tree-sitter grammar to a shared library for future use
    CompileGrammar(CompileGrammar),
}

impl Command {
    fn run(&self) -> Result<()> {
        match self {
            Self::CompileGrammar(cg) => cg.run()
        }
    }
}

#[derive(Debug, Parser)]
struct CompileGrammar {
    /// The name of the language
    name: String,

    /// The path to the source
    path: PathBuf,
}

impl CompileGrammar {
    fn run(&self) -> Result<()> {
        println!("{self:#?}");

        Ok(())
    }
}

fn main() {
    color_eyre::install().expect("could not install error handlers");

    let opts = Command::parse();

    if let Err(err) = opts.run() {
        println!("{err:?}");
        std::process::exit(1);
    }
}
