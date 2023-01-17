use color_eyre::eyre::{bail, Result, WrapErr};
use std::collections::HashMap;
use std::path::PathBuf;
use tree_sitter::Language;

// TODO: Windows support should be possible, but I'm not sure how to do it right now
#[cfg(all(unix, not(target_os = "macos")))]
pub static DYLIB_EXTENSION: &str = "so";

#[cfg(target_os = "macos")]
pub static DYLIB_EXTENSION: &str = "dylib";

#[derive(Debug)]
pub struct Loader {
    include: Vec<PathBuf>,
    grammars: HashMap<String, libloading::Library>,
    languages: HashMap<String, Language>,
}

impl Loader {
    pub fn with_capacity(include: Vec<PathBuf>, size: usize) -> Self {
        Self {
            include,
            grammars: HashMap::with_capacity(size),
            languages: HashMap::with_capacity(size),
        }
    }

    pub fn preload(&mut self, language_name: String) -> Result<()> {
        let symbol_name = format!("tree_sitter_{language_name}");

        let lib = match self.grammars.get(&language_name) {
            Some(grammar) => grammar,
            None => {
                let grammar_path = self
                    .find_grammar(&language_name)
                    .wrap_err("could not find grammar")?;

                let lib =
                    unsafe { libloading::Library::new(&grammar_path) }.wrap_err_with(|| {
                        format!(
                            "could not open shared library ({}) for grammar",
                            grammar_path.display()
                        )
                    })?;
                self.grammars.insert(language_name.clone(), lib);
                self.grammars.get(&language_name).unwrap()
            }
        };

        if !self.languages.contains_key(&language_name) {
            let language = unsafe {
                let lang_fn: libloading::Symbol<unsafe extern "C" fn() -> Language> =
                    lib.get(symbol_name.as_bytes()).wrap_err_with(|| {
                        format!("could not load language function `{}`", symbol_name)
                    })?;

                lang_fn()
            };
            self.languages.insert(language_name, language);
        }

        Ok(())
    }

    pub fn get(&self, language_name: &str) -> Option<Language> {
        self.languages
            .get(language_name)
            .map(|language| language.clone())
    }

    fn find_grammar(&self, name: &str) -> Result<PathBuf> {
        let search_name = PathBuf::from(format!("tree-sitter-{}.{}", name, DYLIB_EXTENSION));

        for path in &self.include {
            let candidate = path.join(&search_name);
            tracing::debug!(name, ?candidate, "looking for grammar");
            if candidate.exists() {
                return Ok(candidate);
            }
        }

        bail!("could not find {search_name:?} in any included path")
    }
}
