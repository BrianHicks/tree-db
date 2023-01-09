use clap::Parser;
use color_eyre::eyre::{bail, Result, WrapErr};
use std::path::PathBuf;
use tracing::instrument;

#[derive(Debug, Parser)]
pub struct CompileGrammar {
    /// The name of the language
    name: String,

    /// The path to the source
    path: PathBuf,

    /// Where to place the shared library
    #[arg(long, default_value("."))]
    out_dir: PathBuf,

    /// Target system to build for
    #[arg(long, default_value(guess_host_triple::guess_host_triple()))]
    target: String,

    /// Host system to build from
    #[arg(long, default_value(guess_host_triple::guess_host_triple()))]
    host: String,

    /// If present, include debugging info in built library
    #[arg(long)]
    debug: bool,

    /// Optimization level. Corresponds roughly to clang's `-O` flag
    /// <https://clang.llvm.org/docs/CommandGuide/clang.html#cmdoption-o0>
    #[arg(short('O'), long, default_value = "2")]
    opt_level: u32,
}

// TODO: Windows support should be possible, but I'm not sure how to do it right now
#[cfg(all(unix, not(target_os = "macos")))]
pub static DYLIB_EXTENSION: &str = "so";

#[cfg(target_os = "macos")]
pub static DYLIB_EXTENSION: &str = "dylib";

impl CompileGrammar {
    #[instrument]
    pub fn run(&self) -> Result<()> {
        let mut builder = cc::Build::new();

        builder
            .opt_level(2)
            .cargo_metadata(false)
            .debug(self.debug)
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
            tracing::info!("enabling C++ compilation");
        }
        tracing::debug!(?scanner_path, "found scanner");

        let mut command = builder
            .try_get_compiler()
            .wrap_err("could not get compiler")?
            .to_command();

        if cfg!(unix) {
            //the `cc` crate will try to compile one of these files at once,
            // but we can compile both in one command. This is necessary in
            // situations where the source is read-only, and is more efficient
            // anyway.
            command
                .arg(&parser_path)
                .arg(&scanner_path)
                .arg("-o")
                .arg(format!("{}.{}", self.name, DYLIB_EXTENSION));

            tracing::info!(?command, "executing");

            let status = command
                .status()
                .wrap_err_with(|| format!("could not execute {:?}", command.get_program()))?;

            match status.code() {
                Some(0) => Ok(()),
                Some(other) => bail!("compilation command exited with status {}", other),
                None => bail!("command was terminated by a signal"),
            }
        } else {
            bail!("grammar compilation for this platform is probably possible, but the author doesn't have a machine to test on. Get in touch!")
        }
    }
}
