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
#[cfg(unix)]
static DYLIB_EXTENSION: &str = "so";

impl CompileGrammar {
    #[instrument]
    fn run(&self) -> Result<()> {
        let temp = tempfile::TempDir::new()
            .wrap_err("could not create temporary directory for use in compilation")?;

        self.compile_scanner(&temp)
            .wrap_err("could not compile scanner")?;

        self.compile_parser(&temp)
            .wrap_err("could not compile grammar")?;

        // // For tomorrow: I think what I need to do here is compile the scanner.c (or scanner.cc) to a scanner.o and the include that in the compilation for parser.c
        // let mut builder = cc::Build::new();
        // builder
        //     .opt_level(2)
        //     .cargo_metadata(false)
        //     .shared_flag(true)
        //     .debug(true)
        //     .include(&self.path)
        //     .out_dir(&self.out_dir)
        //     .target(&self.target)
        //     .host(&self.target)
        //     .flag("-fno-exceptions");

        // let parser_path = self.path.join("parser.c");
        // if !parser_path.exists() {
        //     bail!("parser path (`{}`) does not exist", parser_path.display())
        // }
        // tracing::trace!(?parser_path, "calculated parser path");
        // builder.file(parser_path);

        // builder.compile(&format!("{}.{}", self.name, DYLIB_EXTENSION));

        Ok(())
    }

    #[instrument]
    fn compile_scanner(&self, out: &tempfile::TempDir) -> Result<()> {
        let mut builder = cc::Build::new();

        builder
            .opt_level(2)
            .cargo_metadata(false)
            .debug(true)
            .include(&self.path)
            .out_dir(&out)
            .target(&self.target)
            .host(&self.host)
            .warnings(false)
            .flag("-fno-exceptions");

        let mut file = self.path.join("scanner.c");
        if !file.exists() {
            tracing::debug!("scanner.c does not exist; trying scanner.cc");

            file.set_extension("cc");
            if !file.exists() {
                bail!("scanner does not exist at either scanner.c or scanner.cc");
            }
            builder.cpp(true);
        }

        tracing::info!(source = ?file, "building scanner.o");
        builder
            .file(&file)
            .try_compile("scanner")
            .wrap_err_with(|| format!("could not compile `{}`", file.display()))?;

        Ok(())
    }

    #[instrument]
    fn compile_parser(&self, temp: &tempfile::TempDir) -> Result<()> {
        let mut builder = cc::Build::new();

        builder
            .opt_level(2)
            .cargo_metadata(false)
            .debug(true)
            .include(&self.path)
            .include(&temp)
            .out_dir(&self.out_dir)
            .target(&self.target)
            .host(&self.host)
            .warnings(false)
            .flag("-fno-exceptions")
            // make this a shared library
            .shared_flag(true)
            .pic(true)
            // from this file
            .file(self.path.join("parser.c"))
            .try_compile(&format!("{}.dylib", self.name))
            .wrap_err("could not compile parser.c")
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
