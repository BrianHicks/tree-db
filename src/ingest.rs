use color_eyre::eyre::{bail, Result, WrapErr};
use cozo::NamedRows;
use serde_json::json;
use serde_json::value::Value;
use std::collections::BTreeMap;
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

        // TODO: this could be in parallel pretty easily. Buncha threads, each
        // with an ingestor. Make a way to combine ingestors (appending the
        // interior lists should be fine) and we're good to go.
        for path in &self.file {
            ingestor
                .ingest(path)
                .wrap_err_with(|| format!("could not process `{}`", path.display()))?;
        }

        tracing::info!(
            nodes = ingestor.nodes.len(),
            edges = ingestor.edges.len(),
            "parsed all files"
        );

        let db = self.empty_db().wrap_err("could not set up empty Cozo DB")?;

        if let Err(err) = db.import_relations(ingestor.into()) {
            bail!("{err:#?}");
        };

        // TODO: how do we want output?
        match db.export_relations(vec!["nodes", "edges"].drain(..)) {
            Ok(relations) => println!("{relations:?}"),
            Err(err) => bail!("{err:#?}"),
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

        for path in &self.include {
            let candidate = path.join(&search_name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }

        bail!("could not find {search_name:?} in any included path")
    }

    fn empty_db(&self) -> Result<cozo::Db<cozo::MemStorage>> {
        let db = match cozo::new_cozo_mem() {
            Ok(db) => db,
            // Cozo uses miette for error handling. It looks pretty nice, but
            // it can't be used with color_eyre. Might be worth switching over;
            // they both seem fine and I don't intend tree-db to ever be used
            // as a library (if I did, I'd be doing things in this_error or
            // something similar already.)
            Err(err) => bail!("{err:#?}"),
        };

        if let Err(err) = db.run_script(":create nodes {path: String, id: Int => kind: String, is_error: Bool, parent: Int?, source: String?, start_byte: Int, start_row: Int, start_column: Int, end_byte: Int, end_row: Int, end_column: Int}", BTreeMap::new()) {
            bail!("{err:#?}")
        }

        if let Err(err) = db.run_script(
            ":create edges { path: String, parent: Int, child: Int, field: String? }",
            BTreeMap::new(),
        ) {
            bail!("{err:#?}")
        }

        Ok(db)
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

    #[instrument(skip(self))]
    fn ingest(&mut self, path: &'path Path) -> Result<()> {
        let source = std::fs::read_to_string(path)
            .wrap_err_with(|| format!("could not read `{}`", path.display()))?;

        let mut parser = Parser::new();
        parser
            .set_language(self.language)
            .wrap_err("could not set parser language")?;

        let tree = match parser.parse(&source, None) {
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
                IngestableNode::from(path, &node, &source)
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
}

impl From<Ingestor<'_>> for BTreeMap<String, NamedRows> {
    #[instrument(skip(ingestor))]
    fn from(ingestor: Ingestor<'_>) -> Self {
        Self::from([
            (
                "nodes".into(),
                NamedRows {
                    headers: vec![
                        "path".into(),
                        "id".into(),
                        "kind".into(),
                        "is_error".into(),
                        "parent".into(),
                        "source".into(),
                        "start_byte".into(),
                        "start_row".into(),
                        "start_column".into(),
                        "end_byte".into(),
                        "end_row".into(),
                        "end_column".into(),
                    ],
                    rows: ingestor.nodes.iter().map(|node| node.to_vec()).collect(),
                },
            ),
            (
                "edges".into(),
                NamedRows {
                    headers: vec![
                        "path".into(),
                        "parent".into(),
                        "child".into(),
                        "field".into(),
                    ],
                    rows: ingestor.edges.iter().map(|edge| edge.to_vec()).collect(),
                },
            ),
        ])
    }
}

struct IngestableNode<'path> {
    path: &'path Path,
    id: usize,
    kind: &'static str,
    is_error: bool,
    parent: Option<usize>,
    source: Option<String>,

    // location
    start_byte: usize,
    start_row: usize,
    start_column: usize,
    end_byte: usize,
    end_row: usize,
    end_column: usize,
}

impl<'path> IngestableNode<'path> {
    fn from(path: &'path Path, node: &Node, all_source: &str) -> Result<Self> {
        let range = node.range();
        let source = if node.is_named() {
            Some(match all_source.get(range.start_byte..range.end_byte) {
                Some(source) => source.to_string(),
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

    fn to_vec(&self) -> Vec<Value> {
        vec![
            json!(self.path),
            json!(self.id),
            json!(self.kind),
            json!(self.is_error),
            json!(self.parent),
            json!(self.source),
            json!(self.start_byte),
            json!(self.start_row),
            json!(self.start_column),
            json!(self.end_byte),
            json!(self.end_row),
            json!(self.end_column),
        ]
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
            builder.field("source", source);
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

impl IngestableEdge<'_> {
    fn to_vec(&self) -> Vec<Value> {
        vec![
            json!(self.path),
            json!(self.parent),
            json!(self.child),
            json!(self.field),
        ]
    }
}
