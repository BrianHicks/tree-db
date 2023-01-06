use color_eyre::eyre::{bail, Result, WrapErr};
use std::path::PathBuf;
use tracing::instrument;
use tree_sitter::{Language, Parser};

#[derive(Debug, clap::Parser)]
pub struct Ingest {
    /// Which languages should we include?
    #[arg(short('l'), long)]
    language: String,

    /// Paths to look for language libraries. Use `tree-db compile-grammar` to
    /// make these.
    #[arg(
        long,
        short('i'),
        default_value = ".",
        env = "TREE_DB_LANGUAGE_SEARCH_PATH"
    )]
    include: Vec<PathBuf>,
}

impl Ingest {
    #[instrument]
    pub fn run(&self) -> Result<()> {
        let parser = self
            .parser_for(&self.language)
            .wrap_err("could not find language")?;

        Ok(())
    }

    fn parser_for(&self, language_name: &str) -> Result<Parser> {
        let grammar_path = self
            .find_grammar(language_name)
            .wrap_err("could not find grammar")?;

        let symbol_name = format!("tree_sitter_{language_name}");

        let mut parser = Parser::new();

        unsafe {
            let lib = libloading::Library::new(&grammar_path).wrap_err_with(|| {
                format!(
                    "could not open shared library ({}) for grammar",
                    grammar_path.display()
                )
            })?;

            let lang: libloading::Symbol<unsafe extern "C" fn() -> Language> = lib
                .get(symbol_name.as_bytes())
                .wrap_err_with(|| format!("could not load language function `{}`", symbol_name))?;

            parser
                .set_language(lang())
                .wrap_err("could not set language")?;
        }

        Ok(parser)
    }

    fn find_grammar(&self, name: &str) -> Result<PathBuf> {
        let search_name = PathBuf::from(format!(
            "tree-sitter-{}.{}",
            name,
            crate::compile_grammar::DYLIB_EXTENSION
        ));

        for path in &self.include {
            let candidate = path.join(&search_name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }

        bail!("could not find {search_name:?} in any included path")
    }
}
