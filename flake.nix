{
  description = "A simple OS built in Rust";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    {
      self,
      flake-utils,
      rust-overlay,
      nixpkgs,
    }:
    (flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        nightlyToolChain = pkgs.rust-bin.nightly.latest.default.override {
          extensions = [
            "rust-src"
            "rust-analyzer"
            "llvm-tools-preview"
            "clippy"
          ];
          targets = [
            "thumbv7em-none-eabihf"
          ];
        };
      in
      {
        checks.fmt = pkgs.runCommand "cargo-fmt-check" {
          nativeBuildInputs = [ nightlyToolChain ];
        } ''
          cd ${./os}
          cargo fmt --check
          touch $out
        '';

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            nightlyToolChain
            cargo-show-asm
            cargo-expand
            cargo-bootimage
            qemu
            just
          ];
          env = {
            RUST_SRC_PATH = "${nightlyToolChain}/lib/rustlib/src/library";
          };
        };
      }
    ));
}
