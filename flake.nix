{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";

    naersk.url = "github:nix-community/naersk";
    naersk.inputs.nixpkgs.follows = "nixpkgs";

    # grammars
    tree-sitter-rust = {
      url = "github:tree-sitter/tree-sitter-rust";
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
        packages.tree-sitter-rust = grammar "rust" "${inputs.tree-sitter-rust}/src";

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
