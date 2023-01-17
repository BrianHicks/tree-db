use crate::loader::Loader;
use color_eyre::eyre::{bail, eyre, Result, WrapErr};
use cozo::NamedRows;
use rayon::prelude::*;
use serde_json::json;
use serde_json::value::Value;
use std::collections::{BTreeMap, HashSet};
use std::fmt::Debug;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tracing::instrument;
use tree_sitter::{Language, Node, Parser};

#[derive(Debug, clap::Parser)]
pub struct ExporterConfig {
    /// What format do you want the output in?
    output: Output,

    /// Which languages should we include? (Defaults to all languages whose extensions we know.)
    #[arg(short('l'), long)]
    language: Vec<String>,

    /// Which languages should we avoid including?
    #[arg(short('L'), long)]
    no_language: Vec<String>,

    /// Define a custom language in the format `{name}:{glob}`. You can separate
    /// multiple globs with a comma, like `ruby:*.rb,*.rake`.
    #[arg(long)]
    custom_language: Vec<String>,

    /// Paths to look for language libraries. Use `tree-db compile-grammar` to
    /// make these.
    #[arg(
        long,
        short('i'),
        default_value = ".",
        env = "TREE_DB_LANGUAGE_SEARCH_PATH"
    )]
    include: Vec<PathBuf>,

    #[arg(
        long,
        short('o'),
        required_if_eq("output", "cozo-sqlite"),
        required_if_eq("output", "csv")
    )]
    output_path: Option<PathBuf>,

    /// Where to search for files. These can either be directories or files.
    #[arg(default_value = ".")]
    file: Vec<PathBuf>,

    /// Include hidden files
    #[arg(long)]
    no_hidden: bool,

    /// Parse and use `.ignore` files
    #[arg(long)]
    no_ignore: bool,

    /// Parse and use ignore information from git
    #[arg(long)]
    no_git_ignore: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, clap::ValueEnum)]
pub enum Output {
    /// Cozo relations, as JSON
    CozoJson,

    /// The Cozo schema that we're assuming as a query you can run to start
    /// your own Cozo database.
    CozoSchema,

    /// A SQLite database, as a file
    CozoSqlite,

    /// A set of CSVs. When using this, the path specified in -o/--output-path
    /// must be a directory.
    Csv,
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

struct LanguagesAndPaths {
    languages: HashSet<String>,
    paths: Vec<LanguageAndPath>,
}

struct LanguageAndPath {
    language: String,
    path: PathBuf,
}

impl ExporterConfig {
    #[instrument]
    pub fn run(&self) -> Result<()> {
        match self.output {
            Output::CozoJson => {
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
            Output::Csv => {
                let output_path = self
                    .output_path
                    .as_ref()
                    .ok_or_else(|| eyre!("output_path is required, but should have been validated by clap. Is there a misconfiguration or bug?"))?;

                if !output_path
                    .metadata()
                    .wrap_err_with(|| {
                        format!("could not get metadata for `{}`", output_path.display())
                    })?
                    .file_type()
                    .is_dir()
                {
                    bail!(
                        "For CSV output, we need the output path (`{}`) to be a directory.",
                        output_path.display()
                    );
                }

                // TODO: we wouldn't necessarily have to use cozo for this!
                let db = self
                    .slurp_all()
                    .wrap_err("could not load source files to database")?;

                let relations =
                    match db.export_relations(vec!["nodes", "node_locations", "edges"].drain(..)) {
                        Ok(relations) => relations,
                        Err(err) => bail!("{err:#?}"),
                    };

                Self::write_csv(
                    &output_path.join("nodes.csv"),
                    relations
                        .get("nodes")
                        .expect("nodes should be present in the export above"),
                )
                .wrap_err("could not export `nodes.csv`")?;

                Self::write_csv(
                    &output_path.join("node_locations.csv"),
                    relations
                        .get("node_locations")
                        .expect("node_locations should be present in the export above"),
                )
                .wrap_err("could not export `node_locations.csv`")?;

                Self::write_csv(
                    &output_path.join("edges.csv"),
                    relations
                        .get("edges")
                        .expect("edges should be present in the export above"),
                )
                .wrap_err("could not export `edges.csv`")
            }
        }
    }

    #[instrument]
    fn files(&self) -> Result<LanguagesAndPaths> {
        let mut types_builder = ignore::types::TypesBuilder::new();
        types_builder.add_defaults();
        if self.language.is_empty() {
            types_builder.select("all");
        } else {
            for language in &self.language {
                types_builder.select(language);
            }
        }
        for language in &self.no_language {
            types_builder.negate(language);
        }
        for language in &self.custom_language {
            types_builder
                .add_def(language)
                .wrap_err("could not define custom language")?;
        }

        let types = types_builder
            .build()
            .wrap_err("could not build filetype matcher")?;

        let mut builder = ignore::WalkBuilder::new(match self.file.get(0) {
            Some(path) => path,
            None => bail!("expected at least one path to search"),
        });
        self.file.iter().skip(1).for_each(|path| {
            builder.add(path);
        });
        builder
            .types(types.clone())
            .hidden(!self.no_hidden)
            .ignore(!self.no_ignore)
            .git_ignore(!self.no_git_ignore)
            .git_global(!self.no_git_ignore)
            .git_exclude(!self.no_git_ignore);

        let mut languages = HashSet::with_capacity(self.language.len().max(1));
        let mut paths = Vec::with_capacity(self.file.len());

        for entry_res in builder.build() {
            let entry = entry_res?;

            if let Some(ft) = entry.file_type() {
                if !ft.is_file() {
                    continue;
                }
            }

            if let ignore::Match::Whitelist(glob) = types.matched(entry.path(), false) {
                let file_type = match glob.file_type_def() {
                    Some(ft) => ft,
                    None => bail!("there's always supposed to be a file type def when the types matched a file path"),
                };

                languages.insert(file_type.name().to_string());
                paths.push(LanguageAndPath {
                    language: file_type.name().to_string(),
                    path: entry.into_path(),
                });
            } else {
                bail!("got an entry which wasn't a directory and also didn't match any supplied file types. Is this a misconfiguration or a bug?")
            }
        }

        Ok(LanguagesAndPaths { languages, paths })
    }

    #[instrument]
    fn slurp_all(&self) -> Result<cozo::Db<cozo::MemStorage>> {
        let LanguagesAndPaths {
            mut languages,
            paths,
        } = self.files().wrap_err("could not get files")?;

        let mut loader = Loader::with_capacity(self.include.clone(), languages.len());
        for language in languages.drain() {
            loader
                .preload(language)
                .wrap_err("could not load language")?;
        }

        let mut exporters = paths
            .par_iter()
            .map(|LanguageAndPath { language: language_name, path }| {
                let language = match loader.get(language_name) {
                    Some(language) => language,
                    None => bail!("could not get a language definition for `{language_name}`. Was it preloaded?"),
                };

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

    #[instrument(skip(data))]
    fn write_csv(path: &Path, data: &NamedRows) -> Result<()> {
        let nodes_file = std::fs::File::create(path)?;

        let mut csv_writer = csv::Writer::from_writer(nodes_file);
        csv_writer
            .write_record(&data.headers)
            .wrap_err("could not write header")?;

        for row in &data.rows {
            csv_writer.serialize(row).wrap_err("could not write row")?;
        }

        Ok(())
    }

    #[instrument(skip(data))]
    fn write(&self, data: &str) -> Result<()> {
        match &self.output_path {
            None => std::io::stdout()
                .write(data.as_bytes())
                .map(|_| ())
                .wrap_err("could not write to stdout"),
            Some(path) => std::fs::write(path, data).wrap_err("could not write to output file"),
        }
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

    #[instrument(skip(self), fields(path = ?self.path))]
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
        let mut file = std::fs::File::open(self.path)
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
        let source_bytes = if node.is_named() && node.child_count() == 0 {
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
