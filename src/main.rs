use clap::Parser;
use color_eyre::eyre::{bail, Result, WrapErr};
use std::path::PathBuf;
use tracing::instrument;
use tracing_error::ErrorLayer;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
enum Command {
    /// Compile a tree-sitter grammar to a shared library for future use
    CompileGrammar(CompileGrammar),
}

impl Command {
    fn run(&self) -> Result<()> {
        match self {
            Self::CompileGrammar(cg) => cg.run(),
        }
    }
}

#[derive(Debug, Parser)]
struct CompileGrammar {
    /// The name of the language
    name: String,

    /// The path to the source
    path: PathBuf,

    /// Where to place the shared library
    #[arg(long, default_value("."))]
    out_dir: PathBuf,

    #[arg(long, default_value(guess_host_triple::guess_host_triple()))]
    target: String,

    #[arg(long, default_value(guess_host_triple::guess_host_triple()))]
    host: String,
}

// TODO: Windows support should be possible, but I'm not sure how to do it right now
#[cfg(all(unix, not(target_os = "macos")))]
static DYLIB_EXTENSION: &str = "so";

#[cfg(target_os = "macos")]
static DYLIB_EXTENSION: &str = "dylib";

impl CompileGrammar {
    #[instrument]
    fn run(&self) -> Result<()> {
        // working command: clang -I vendor/tree-sitter-rust/src vendor/tree-sitter-rust/src/parser.c vendor/tree-sitter-rust/src/scanner.c -o thingy.so -shared -fpic -o2
        let mut builder = cc::Build::new();

        builder
            .opt_level(2)
            .cargo_metadata(false)
            .debug(true)
            .include(&self.path)
            .out_dir(&self.out_dir)
            .target(&self.target)
            .host(&self.host)
            .warnings(false)
            .shared_flag(true)
            .pic(true)
            .flag("-fno-exceptions");

        let parser_path = self.path.join("parser.c");
        if !parser_path.exists() {
            bail!(
                "parser (should be `{}`) does not exist",
                parser_path.display(),
            );
        }
        builder.file(parser_path);

        let mut scanner_path = self.path.join("scanner.c");
        if !scanner_path.exists() {
            scanner_path.set_extension("cc");
            if !scanner_path.exists() {
                bail!(
                    "scanner (`scanner.c` or `scanner.cc` at `{}`) does not exist",
                    self.path.display()
                );
            }
            builder.cpp(true);
        }
        tracing::debug!(?scanner_path, "found scanner");
        builder.file(scanner_path);

        builder
            .try_compile(&format!("{}.{}", self.name, DYLIB_EXTENSION))
            .wrap_err("could not compile grammar")
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
