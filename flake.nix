{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";

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
      in
      {
        formatter = pkgs.nixpkgs-fmt;
        devShell = pkgs.mkShell {
          packages = [
            pkgs.cargo
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
