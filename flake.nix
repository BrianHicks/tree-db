{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    naersk.url = "github:nix-community/naersk";
    naersk.inputs.nixpkgs.follows = "nixpkgs";

    # grammars
    tree-sitter-c = {
      url = "github:tree-sitter/tree-sitter-c";
      flake = false;
    };

    tree-sitter-cpp = {
      url = "github:tree-sitter/tree-sitter-cpp";
      flake = false;
    };

    tree-sitter-nix = {
      url = "github:cstrahan/tree-sitter-nix";
      flake = false;
    };

    tree-sitter-elixir = {
      url = "github:elixir-lang/tree-sitter-elixir/main";
      flake = false;
    };

    tree-sitter-elm = {
      url = "github:elm-tooling/tree-sitter-elm/main";
      flake = false;
    };

    tree-sitter-haskell = {
      url = "github:tree-sitter/tree-sitter-haskell";
      flake = false;
    };

    tree-sitter-javascript = {
      url = "github:tree-sitter/tree-sitter-javascript";
      flake = false;
    };

    tree-sitter-markdown = {
      url = "github:ikatyang/tree-sitter-markdown";
      flake = false;
    };

    tree-sitter-php = {
      url = "github:tree-sitter/tree-sitter-php";
      flake = false;
    };

    tree-sitter-python = {
      url = "github:tree-sitter/tree-sitter-python";
      flake = false;
    };

    tree-sitter-ruby = {
      url = "github:tree-sitter/tree-sitter-ruby";
      flake = false;
    };

    tree-sitter-rust = {
      url = "github:tree-sitter/tree-sitter-rust";
      flake = false;
    };

    tree-sitter-typescript = {
      url = "github:tree-sitter/tree-sitter-typescript";
      flake = false;
    };
  };

  outputs = inputs:
    inputs.flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import inputs.nixpkgs { inherit system; };

        vendor-languages = pkgs.writeShellScriptBin "vendor-languages" ''
          rm -rf vendor
          mkdir vendor

          ln -s ${inputs.tree-sitter-c} vendor/tree-sitter-c
          ln -s ${inputs.tree-sitter-cpp} vendor/tree-sitter-cpp
          ln -s ${inputs.tree-sitter-nix} vendor/tree-sitter-nix
          ln -s ${inputs.tree-sitter-elixir} vendor/tree-sitter-elixir
          ln -s ${inputs.tree-sitter-elm} vendor/tree-sitter-elm
          ln -s ${inputs.tree-sitter-haskell} vendor/tree-sitter-haskell
          ln -s ${inputs.tree-sitter-javascript} vendor/tree-sitter-javascript
          ln -s ${inputs.tree-sitter-markdown} vendor/tree-sitter-markdown
          ln -s ${inputs.tree-sitter-php} vendor/tree-sitter-php
          ln -s ${inputs.tree-sitter-python} vendor/tree-sitter-python
          ln -s ${inputs.tree-sitter-ruby} vendor/tree-sitter-ruby
          ln -s ${inputs.tree-sitter-rust} vendor/tree-sitter-rust
          ln -s ${inputs.tree-sitter-typescript} vendor/tree-sitter-typescript
        '';

        naersk = pkgs.callPackage inputs.naersk { };

        tree-db = naersk.buildPackage {
          src = ./.;
        };

        grammar = { name, src, path ? "src" }: pkgs.stdenv.mkDerivation {
          name = "tree-sitter-${name}";
          inherit src;

          buildPhase = ''
            BINARY=${pkgs.clang}/bin/clang
            ARGS=(-O2 -ffunction-sections -fdata-sections -fPIC -shared -fno-exceptions -I ${path})

            if test -f ${path}/parser.c; then
              ARGS+=(${path}/parser.c)
            fi

            if test -f ${path}/scanner.c; then
              ARGS+=(${path}/scanner.c)
            elif test -f ${path}/scanner.cc; then
              ARGS+=(${path}/scanner.cc)
              BINARY=${pkgs.clang}/bin/clang++
            fi

            ARGS+=(-o $out/lib/tree-db/tree-sitter-${name}.${if pkgs.stdenv.isDarwin then "dylib" else "so"})

            mkdir -p $out/lib/tree-db
            $BINARY ''${ARGS[@]}
          '';

          installPhase = "true";
        };

        grammars = [
          (grammar {
            name = "c";
            src = inputs.tree-sitter-c;
          })
          (grammar {
            name = "cpp";
            src = inputs.tree-sitter-cpp;
          })
          (grammar {
            name = "nix";
            src = inputs.tree-sitter-nix;
          })
          (grammar {
            name = "elixir";
            src = inputs.tree-sitter-elixir;
          })
          (grammar {
            name = "elm";
            src = inputs.tree-sitter-elm;
          })
          (grammar {
            name = "haskell";
            src = inputs.tree-sitter-haskell;
          })
          (grammar {
            name = "javascript";
            src = inputs.tree-sitter-javascript;
          })
          (grammar {
            name = "markdown";
            src = inputs.tree-sitter-markdown;
          })
          (grammar {
            name = "php";
            src = inputs.tree-sitter-php;
          })
          (grammar {
            name = "python";
            src = inputs.tree-sitter-python;
          })
          (grammar {
            name = "ruby";
            src = inputs.tree-sitter-ruby;
          })
          (grammar {
            name = "rust";
            src = inputs.tree-sitter-rust;
          })
          (grammar {
            name = "typescript";
            src = inputs.tree-sitter-rust;
            path = "typescript/src";
          })
        ];
      in
      rec {
        formatter = pkgs.nixpkgs-fmt;

        lib.tree-db-with-grammars = { grammars, tree-db ? packages.tree-db, name ? "tree-db-custom" }: pkgs.stdenv.mkDerivation {
          inherit name;
          src = builtins.filterSource (_: _: false) ./.;

          buildInputs = [ pkgs.makeWrapper ];
          buildPhase = ''
            mkdir -p $out/bin

            makeWrapper ${tree-db}/bin/tree-db $out/bin/tree-db \
              --set TREE_DB_LANGUAGE_SEARCH_PATH ${pkgs.symlinkJoin { name = "${name}-grammars"; paths = grammars; }}/lib/tree-db
          '';

          installPhase = "true";
        };

        packages.tree-db = tree-db;

        packages.grammars =
          (builtins.listToAttrs (builtins.map (drv: { name = drv.name; value = drv; }) grammars));

        packages.tree-db-full = lib.tree-db-with-grammars {
          name = "tree-db-full";
          grammars = builtins.attrValues packages.grammars;
        };
        defaultPackage = packages.tree-db-full;

        devShell = pkgs.mkShell {
          packages = [
            pkgs.cargo
            pkgs.cargo-edit
            pkgs.rustc
            pkgs.libiconv
            pkgs.rustfmt
            pkgs.clippy
            pkgs.rust-analyzer

            vendor-languages
          ] ++ pkgs.lib.optional pkgs.stdenv.isDarwin [
            pkgs.darwin.apple_sdk.frameworks.Security
          ];
        };
      });
}
