# tree-db

Transforms a project's source AST into a database you can query!

## Installation

Use nix to install:

```
nix-env -if https://github.com/BrianHicks/tree-grepper/archive/refs/heads/main.tar.gz
```

If you have a Rust toolchain set up, you can also clone this repo and run `cargo build`.

## Schema

`tree-db` can emit a [Cozo](https://www.cozodb.org/) database or SQLite backup, depending on the command (run `tree-db help export` for full documentation or to export this schema.)

```
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
```

The schema in this file is only provided for convenience and understanding, though.
See `tree-db export cozo-schema` for the schema that your installed version of `tree-db` actually works with.

## Stability

`tree-db` is pre-1.0.0 software, and not yet completely stabilized.
Here are the big things that might change:

1. The schema that `tree-db` generates might change once I've used it in a couple of places.
1. I'm not sure where it should live (GitHub might only be temporary)
1. It might not make sense to have a `compile-grammar` subcommand.
   It might be more of a packaging task than a runtime one, but `tree-db` does require building shared libraries, so it might be good to include?
   Not sure yet.

## Contributing

This is a personal project right now.
I've got some ideas about how to make it work well, but this is more-or-less an experiment that I'm doing in public.
Feel free to open issues if you give this a try, but PRs probably don't make sense yet (unless we talk first.)

## License

MIT.
See `LICENSE`.