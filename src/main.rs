use clap::Parser;
use color_eyre::eyre::Result;
use tracing_error::ErrorLayer;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

mod compile_grammar;
mod ingest;

#[derive(Debug, Parser)]
enum Command {
    /// Turn a source tree into a Cozo database file
    Ingest(ingest::Ingest),

    /// Compile a tree-sitter grammar to a shared library for future use
    CompileGrammar(compile_grammar::CompileGrammar),
}

impl Command {
    fn run(&self) -> Result<()> {
        match self {
            Self::CompileGrammar(cg) => cg.run(),
            Self::Ingest(i) => i.run(),
        }
    }
}

fn main() {
    let subscriber = tracing_subscriber::Registry::default()
        .with(ErrorLayer::default())
        .with(tracing_subscriber::fmt::layer().with_span_events(FmtSpan::NEW))
        .with(
            EnvFilter::try_from_default_env()
                // TODO: default to `info` eventually
                .or_else(|_| EnvFilter::try_new("trace"))
                .unwrap(),
        );

    tracing::subscriber::set_global_default(subscriber)
        .expect("could not initialize tracing subscribers");

    color_eyre::install().expect("could not initialize error handling");

    let opts = Command::parse();

    if let Err(err) = opts.run() {
        eprintln!("{err:?}");
        std::process::exit(1);
    }
}
