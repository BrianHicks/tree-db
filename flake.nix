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
      in
      rec {
        formatter = pkgs.nixpkgs-fmt;

        packages.tree-db = tree-db;

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
