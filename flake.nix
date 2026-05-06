{
  description = "sorceress";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    crane,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = nixpkgs.legacyPackages.${system};
        craneLib = crane.mkLib pkgs;

        # build just the dependencies first
        # crane caches this aggressively. if you only change main.rs,
        # nix won't recompile niri_ipc or any other crates
        cargoArtifacts = craneLib.buildDepsOnly {
          src = craneLib.cleanCargoSource ./.;
        };

        rustToolchain = pkgs.symlinkJoin {
          name = "rust-toolchain";
          paths = [
            pkgs.rustc
            pkgs.cargo
            pkgs.rustfmt
            pkgs.rustPackages.clippy
            pkgs.rust-analyzer
            pkgs.gcc
          ];

          nativeBuildInputs = [ pkgs.makeWrapper ];

          postBuild = ''
            for bin in $out/bin/*; do
              wrapProgram "$bin" \
                --prefix PATH : "${pkgs.gcc}/bin"
            done
          '';
        };

        sorceress = craneLib.buildPackage {
          src = craneLib.cleanCargoSource ./.;
          inherit cargoArtifacts;

          buildInputs = with pkgs; [];
          nativeBuildInputs = with pkgs; [];
        };
      in {
        packages.default = sorceress;
        packages.sorceress = sorceress;

        devShells.default = craneLib.devShell {
          inputsFrom = [sorceress];

          packages = with pkgs; [
            rustToolchain
            cargo-watch
          ];

          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
        };
      }
    );
}
