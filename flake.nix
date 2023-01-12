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
          ln -s ${inputs.tree-sitter-rust} vendor/tree-sitter-rust
        '';

        naersk = pkgs.callPackage inputs.naersk { };

        tree-db = naersk.buildPackage {
          src = ./.;
        };

        grammar = name: src: pkgs.stdenv.mkDerivation {
          name = "tree-sitter-${name}";
          src = src;

          buildPhase = ''
            mkdir -p $out/lib/tree-db
            ${tree-db}/bin/tree-db compile-grammar --out-dir $out/lib/tree-db tree-sitter-${name} .
          '';
          installPhase = "true";
        };
      in
      rec {
        formatter = pkgs.nixpkgs-fmt;

        packages.tree-db = tree-db;

        # grammars
        # packages.tree-sitter-c = grammar "c" "${inputs.tree-sitter-c}/src";
        packages.tree-sitter-cpp = grammar "cpp" "${inputs.tree-sitter-cpp}/src";
        packages.tree-sitter-nix = grammar "nix" "${inputs.tree-sitter-nix}/src";
        packages.tree-sitter-elixir = grammar "elixir" "${inputs.tree-sitter-elixir}/src";
        packages.tree-sitter-elm = grammar "elm" "${inputs.tree-sitter-elm}/src";
        packages.tree-sitter-haskell = grammar "haskell" "${inputs.tree-sitter-haskell}/src";
        packages.tree-sitter-javascript = grammar "javascript" "${inputs.tree-sitter-javascript}/src";
        packages.tree-sitter-markdown = grammar "markdown" "${inputs.tree-sitter-markdown}/src";
        packages.tree-sitter-php = grammar "php" "${inputs.tree-sitter-php}/src";
        packages.tree-sitter-python = grammar "python" "${inputs.tree-sitter-python}/src";
        packages.tree-sitter-ruby = grammar "ruby" "${inputs.tree-sitter-ruby}/src";
        packages.tree-sitter-rust = grammar "rust" "${inputs.tree-sitter-rust}/src";
        # packages.tree-sitter-typescript = grammar "typescript" "${inputs.tree-sitter-typescript}/typescript/src";

        packages.tree-db-full = pkgs.symlinkJoin {
          name = "tree-db";
          paths = [
            packages.tree-db

            # grammars
            # packages.tree-sitter-c
            packages.tree-sitter-cpp
            packages.tree-sitter-nix
            packages.tree-sitter-elixir
            packages.tree-sitter-elm
            packages.tree-sitter-haskell
            packages.tree-sitter-javascript
            packages.tree-sitter-markdown
            packages.tree-sitter-php
            packages.tree-sitter-python
            packages.tree-sitter-ruby
            packages.tree-sitter-rust
            # packages.tree-sitter-typescript
          ];
        };

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
