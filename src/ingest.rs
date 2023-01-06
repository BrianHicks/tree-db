use color_eyre::eyre::{bail, Result, WrapErr};
use cozo::NamedRows;
use std::collections::HashMap;
use std::fmt::Debug;
use std::path::Path;
use std::path::PathBuf;
use tracing::instrument;
use tree_sitter::Node;
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

impl IngestorConfig {
    #[instrument]
    pub fn run(&self) -> Result<()> {
        let language = self
            .language_for(&self.language)
            .wrap_err("could not find language")?;

        let mut ingestor = Ingestor::new(language);

        for path in &self.file {
            ingestor
                .ingest(&path)
                .wrap_err_with(|| format!("could not process `{}`", path.display()))?;
        }

        tracing::info!(
            nodes = ingestor.nodes.len(),
            edges = ingestor.edges.len(),
            "parsed all files"
        );

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

        for path in &self.include {
            let candidate = path.join(&search_name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }

        bail!("could not find {search_name:?} in any included path")
    }
}

#[derive(Debug)]
pub struct Ingestor<'path> {
    language: Language,
    nodes: Vec<IngestableNode<'path>>,
    edges: Vec<IngestableEdge<'path>>,
}

impl<'path> Ingestor<'path> {
    fn new(language: Language) -> Self {
        Self {
            language,
            // TODO: these capacities are really a shot in the dark. It's
            // probably worth measuring what's typical and then adjusting them.
            nodes: Vec::with_capacity(2 ^ 10),
            edges: Vec::with_capacity(2 ^ 10),
        }
    }

    fn ingest(&mut self, path: &'path Path) -> Result<()> {
        let bytes = std::fs::read(&path)
            .wrap_err_with(|| format!("could not read `{}`", path.display()))?;

        let mut parser = Parser::new();
        parser
            .set_language(self.language)
            .wrap_err("could not set parser language")?;

        let tree = match parser.parse(&bytes, None) {
            Some(tree) => tree,
            None => bail!("internal error: parser did not return a tree"),
        };

        let mut cursor = tree.walk();
        let mut todo = vec![tree.root_node()];

        while let Some(node) = todo.pop() {
            if node.is_error() {
                let range = node.range();
                tracing::warn!(
                    "`{}` contains an error at {}:{}",
                    path.display(),
                    range.start_point.row,
                    range.start_point.column,
                )
            }

            self.nodes.push(
                IngestableNode::from(&path, &node, &bytes)
                    .wrap_err("could not ingest a syntax node")?,
            );

            for (i, child) in node.children(&mut cursor).enumerate() {
                todo.push(child);

                self.edges.push(IngestableEdge {
                    path,
                    parent: node.id(),
                    child: node.id(),
                    field: node.field_name_for_child(i as u32),
                })
            }
        }

        Ok(())
    }

    fn ingest_node(&mut self, path: &Path, node: &Node) -> Result<()> {
        Ok(())
    }
}

struct IngestableNode<'path> {
    path: &'path Path,
    id: usize,
    kind: &'static str,
    is_error: bool,
    parent: Option<usize>,
    source: Option<Vec<u8>>,

    // location
    start_byte: usize,
    start_row: usize,
    start_column: usize,
    end_byte: usize,
    end_row: usize,
    end_column: usize,
}

impl<'path> IngestableNode<'path> {
    fn from(path: &'path Path, node: &Node, all_source: &[u8]) -> Result<Self> {
        let range = node.range();
        let source = if node.is_named() {
            Some(match all_source.get(range.start_byte..range.end_byte) {
                Some(source) => source.to_vec(),
                None => bail!(
                    "didn't have enough bytes ({}) for the source range ({}–{})",
                    all_source.len(),
                    range.start_byte,
                    range.end_byte,
                ),
            })
        } else {
            None
        };

        Ok(Self {
            path,
            id: node.id(),
            kind: node.kind(),
            is_error: node.is_error(),
            parent: node.parent().map(|node| node.id()),
            source,

            // location
            start_byte: range.start_byte,
            start_row: range.start_point.row,
            start_column: range.start_point.column,
            end_byte: range.end_byte,
            end_row: range.end_point.row,
            end_column: range.end_point.column,
        })
    }
}

impl Debug for IngestableNode<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut builder = f.debug_struct("IngestableNode");

        builder
            .field("path", &self.path)
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("is_error", &self.is_error);

        if let Some(parent) = &self.parent {
            builder.field("parent", parent);
        }

        if let Some(source) = &self.source {
            builder.field(
                "source",
                &core::str::from_utf8(source).unwrap_or("<invalid utf-8>"),
            );
        }

        builder
            .field("start", &(&self.start_row, &self.start_column))
            .field("end", &(&self.end_row, &self.end_column));

        builder.finish()
    }
}

#[derive(Debug)]
struct IngestableEdge<'path> {
    path: &'path Path,
    parent: usize,
    child: usize,
    field: Option<&'static str>,
}
