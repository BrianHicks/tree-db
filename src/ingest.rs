use color_eyre::eyre::{bail, Result, WrapErr};
use cozo::NamedRows;
use std::collections::BTreeMap;
use std::path::PathBuf;
use tracing::instrument;
use tree_sitter::{Language, Parser};

#[derive(Debug, clap::Parser)]
pub struct IngestorConfig {
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

    /// The files to ingest
    file: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct Ingestor {
    config: IngestorConfig,
    relations: BTreeMap<String, NamedRows>,
}

impl From<IngestorConfig> for Ingestor {
    fn from(config: IngestorConfig) -> Self {
        Self {
            config,
            relations: BTreeMap::new(),
        }
    }
}

impl Ingestor {
    #[instrument]
    pub fn run(&self) -> Result<()> {
        let language = self
            .language_for(&self.config.language)
            .wrap_err("could not find language")?;

        println!("{:#?}", language);

        for path in &self.config.file {
            println!("{path:#?}");
        }

        Ok(())
    }

    fn language_for(&self, language_name: &str) -> Result<Language> {
        let grammar_path = self
            .find_grammar(language_name)
            .wrap_err("could not find grammar")?;

        let symbol_name = format!("tree_sitter_{language_name}");

        let lib = unsafe { libloading::Library::new(&grammar_path) }.wrap_err_with(|| {
            format!(
                "could not open shared library ({}) for grammar",
                grammar_path.display()
            )
        })?;

        let language = unsafe {
            let lang_fn: libloading::Symbol<unsafe extern "C" fn() -> Language> = lib
                .get(symbol_name.as_bytes())
                .wrap_err_with(|| format!("could not load language function `{}`", symbol_name))?;

            lang_fn()
        };

        // HACK: this keeps the library's memory allocated for the duration of
        // the program. This is necessary, since we've just called `lang` to get
        // a reference to the grammar, and if the library gets unloaded before
        // we parse then we'll get segfaults. An alternative eventually be to
        // keep a mapping of language name to `libloading::Library` around.
        //
        // The docs for `std::mem::forget` say that a reference into the memory
        // passed to it will not always be valid, but it looks Helix does this
        // and it works fine. Diffsitter prefers to use `Box::leak` instead.
        // We'll see what we see, I guess.
        std::mem::forget(lib);

        Ok(language)
    }

    fn find_grammar(&self, name: &str) -> Result<PathBuf> {
        let search_name = PathBuf::from(format!(
            "tree-sitter-{}.{}",
            name,
            crate::compile_grammar::DYLIB_EXTENSION
        ));

        for path in &self.config.include {
            let candidate = path.join(&search_name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }

        bail!("could not find {search_name:?} in any included path")
    }
}
