use color_eyre::eyre::{bail, Result, WrapErr};
use cozo::NamedRows;
use rayon::prelude::*;
use serde_json::json;
use serde_json::value::Value;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use tracing::instrument;
use tree_sitter::Node;
use tree_sitter::{Language, Parser};

#[derive(Debug, clap::Parser)]
pub struct ExporterConfig {
    /// What format do you want the output in?
    output: Output,

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

    #[arg(long, short('o'), required_if_eq("output", "cozo-sqlite"))]
    output_path: Option<PathBuf>,

    /// Where to search for files. These can either be directories or files.
    #[arg(default_value = ".")]
    file: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, clap::ValueEnum)]
pub enum Output {
    /// Cozo relations, as JSON
    Cozo,

    /// The Cozo schema that we're assuming, as a query you can run to start
    /// your own Cozo database.
    CozoSchema,

    /// A SQLite database, as a file
    CozoSqlite,
}

static SCHEMA: &str = indoc::indoc! {"
    {:create nodes {
        path: String,
        id: Int,
        =>
        kind: String,
        is_error: Bool,
        source: String?,
    }}

    {:create node_locations {
        path: String,
        id: Int,
        =>
        start_byte: Int,
        start_row: Int,
        start_column: Int,
        end_byte: Int,
        end_row: Int,
        end_column: Int,
    }}

    {:create edges {
        path: String,
        parent: Int,
        child: Int,
        field: String?,
    }}

"};

impl ExporterConfig {
    #[instrument]
    pub fn run(&self) -> Result<()> {
        match self.output {
            Output::Cozo => {
                let db = self.slurp_all().wrap_err("failed to create database")?;

                match db.export_relations(vec!["nodes", "node_locations", "edges"].drain(..)) {
                    Ok(relations) => {
                        let json = serde_json::to_string(&relations)
                            .wrap_err("could not export relations")?;
                        self.write(&json).wrap_err("could not write output")
                    }
                    Err(err) => bail!("{err:#?}"),
                }
            }
            Output::CozoSchema => self.write(SCHEMA).context("could not write schema"),
            Output::CozoSqlite => match self
                .slurp_all()
                .wrap_err("failed to create database")?
                .backup_db(
                self.output_path
                    .as_ref()
                    .expect(
                        "if output is sqlite, output path should have been required as an argument",
                    )
                    // hmm, it's a little weird that the Cozo API doesn't take a PathBuf...
                    .display()
                    .to_string(),
            ) {
                Ok(()) => Ok(()),
                Err(err) => bail!("{err:#?}"),
            },
        }
    }

    fn files(&self) -> Result<Vec<PathBuf>> {
        let mut builder = ignore::WalkBuilder::new(match self.file.get(0) {
            Some(path) => path,
            None => bail!("expected at least one path to search"),
        });
        self.file.iter().skip(1).for_each(|path| {
            builder.add(path);
        });

        let mut out = Vec::with_capacity(self.file.len());
        for entry_res in builder.build() {
            let entry = entry_res?;

            if let Some(ft) = entry.file_type() {
                if ft.is_file() {
                    out.push(entry.into_path())
                }
            }
        }

        Ok(out)
    }

    fn slurp_all(&self) -> Result<cozo::Db<cozo::MemStorage>> {
        let language = self
            .language_for(&self.language)
            .wrap_err("could not find language")?;

        let files = self.files().wrap_err("could not get files")?;

        let mut exporters = files
            .par_iter()
            .map(|path| {
                let mut exporter = FileExporter::new(language, path);
                exporter
                    .slurp()
                    .wrap_err_with(|| format!("could not export from `{}`", path.display()))?;
                Ok(exporter)
            })
            .collect::<Result<Vec<FileExporter<'_>>>>()
            .wrap_err("failed to parse files")?;

        let db = self.empty_db().wrap_err("could not set up empty Cozo DB")?;

        for exporter in exporters.drain(..) {
            if let Err(err) = db.import_relations(exporter.into()) {
                bail!("{err:#?}");
            };
        }

        Ok(db)
    }

    fn write(&self, data: &str) -> Result<()> {
        match &self.output_path {
            None => std::io::stdout()
                .write(data.as_bytes())
                .map(|_| ())
                .wrap_err("could not write to stdout"),
            Some(path) => std::fs::write(path, data).wrap_err("could not write to output file"),
        }
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

        if let Err(err) = db.run_script(SCHEMA, BTreeMap::new()) {
            bail!("{err:#?}")
        }

        Ok(db)
    }
}

#[derive(Debug)]
pub struct FileExporter<'path> {
    language: Language,

    path: &'path Path,
    source: String,

    nodes: Vec<ExportableNode<'path>>,
    locations: Vec<ExportableNodeLocation<'path>>,
    edges: Vec<ExportableEdge<'path>>,
}

impl<'path> FileExporter<'path> {
    fn new(language: Language, path: &'path Path) -> Self {
        Self {
            language,
            path,
            // TODO: these capacities are really a shot in the dark. It's
            // probably worth measuring what's typical and then adjusting them.
            source: String::with_capacity(2 ^ 10),
            nodes: Vec::with_capacity(2 ^ 10),
            locations: Vec::with_capacity(2 ^ 10),
            edges: Vec::with_capacity(2 ^ 10),
        }
    }

    #[instrument(skip(self))]
    fn slurp(&mut self) -> Result<()> {
        self.read_source().wrap_err("could not read source")?;

        let mut parser = Parser::new();
        parser
            .set_language(self.language)
            .wrap_err("could not set parser language")?;

        let tree = match parser.parse(&self.source, None) {
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
                    self.path.display(),
                    range.start_point.row,
                    range.start_point.column,
                )
            }

            self.nodes.push(ExportableNode::from(self.path, &node));
            self.locations
                .push(ExportableNodeLocation::from(self.path, &node));

            for (i, child) in node.children(&mut cursor).enumerate() {
                todo.push(child);

                self.edges.push(ExportableEdge {
                    path: self.path,
                    parent: node.id(),
                    child: node.id(),
                    field: node.field_name_for_child(i as u32),
                })
            }
        }

        Ok(())
    }

    fn read_source(&mut self) -> Result<()> {
        let mut file = std::fs::File::open(&self.path)
            .wrap_err_with(|| format!("could not open `{}`", self.path.display()))?;

        file.read_to_string(&mut self.source)
            .wrap_err_with(|| format!("could not read source file `{}`", self.path.display()))?;

        Ok(())
    }
}

impl From<FileExporter<'_>> for BTreeMap<String, NamedRows> {
    #[instrument(skip(exporter))]
    fn from(exporter: FileExporter<'_>) -> Self {
        Self::from([
            (
                "nodes".into(),
                NamedRows {
                    headers: vec![
                        "path".into(),
                        "id".into(),
                        "kind".into(),
                        "is_error".into(),
                        "source".into(),
                    ],
                    rows: exporter
                        .nodes
                        .iter()
                        .map(|node| node.to_vec(&exporter.source))
                        .collect(),
                },
            ),
            (
                "node_locations".into(),
                NamedRows {
                    headers: vec![
                        "path".into(),
                        "id".into(),
                        "start_byte".into(),
                        "start_row".into(),
                        "start_column".into(),
                        "end_byte".into(),
                        "end_row".into(),
                        "end_column".into(),
                    ],
                    rows: exporter.locations.iter().map(|loc| loc.to_vec()).collect(),
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
                    rows: exporter.edges.iter().map(|edge| edge.to_vec()).collect(),
                },
            ),
        ])
    }
}

#[derive(Debug)]
struct ExportableNode<'path> {
    path: &'path Path,
    id: usize,
    kind: &'static str,
    is_error: bool,
    source_bytes: Option<(usize, usize)>,
}

impl<'path> ExportableNode<'path> {
    fn from(path: &'path Path, node: &Node) -> Self {
        let range = node.range();
        let source_bytes = if node.is_named() {
            Some((range.start_byte, range.end_byte))
        } else {
            None
        };

        Self {
            path,
            id: node.id(),
            kind: node.kind(),
            is_error: node.is_error(),
            source_bytes,
        }
    }

    fn to_vec(&self, source: &str) -> Vec<Value> {
        vec![
            json!(self.path),
            json!(self.id),
            json!(self.kind),
            json!(self.is_error),
            json!(self
                .source_bytes
                .and_then(|(start, end)| source.get(start..end))),
        ]
    }
}

#[derive(Debug)]
struct ExportableNodeLocation<'path> {
    path: &'path Path,
    id: usize,
    start_byte: usize,
    start_row: usize,
    start_column: usize,
    end_byte: usize,
    end_row: usize,
    end_column: usize,
}

impl<'path> ExportableNodeLocation<'path> {
    fn from(path: &'path Path, node: &Node) -> Self {
        let range = node.range();

        Self {
            path,
            id: node.id(),
            start_byte: range.start_byte,
            start_row: range.start_point.row,
            start_column: range.start_point.column,
            end_byte: range.end_byte,
            end_row: range.end_point.row,
            end_column: range.end_point.column,
        }
    }

    fn to_vec(&self) -> Vec<Value> {
        vec![
            json!(self.path),
            json!(self.id),
            json!(self.start_byte),
            json!(self.start_row),
            json!(self.start_column),
            json!(self.end_byte),
            json!(self.end_row),
            json!(self.end_column),
        ]
    }
}

#[derive(Debug)]
struct ExportableEdge<'path> {
    path: &'path Path,
    parent: usize,
    child: usize,
    field: Option<&'static str>,
}

impl ExportableEdge<'_> {
    fn to_vec(&self) -> Vec<Value> {
        vec![
            json!(self.path),
            json!(self.parent),
            json!(self.child),
            json!(self.field),
        ]
    }
}
