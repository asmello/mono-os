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
            "rustfmt"
          ];
          targets = [
            "thumbv7m-none-eabi"
          ];
        };
      in
      {
        checks.fmt =
          pkgs.runCommand "cargo-fmt-check"
            {
              nativeBuildInputs = [ nightlyToolChain ];
            }
            ''
              cd ${./os}
              cargo fmt --check
              touch $out
            '';

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            nightlyToolChain
            # `cargo objdump` / `objcopy` / `size` / `nm`. Resolves the
            # llvm tools via `llvm-tools-preview` in the toolchain above.
            cargo-binutils
            qemu
            probe-rs-tools
            tio
          ];
          env = {
            RUST_SRC_PATH = "${nightlyToolChain}/lib/rustlib/src/library";
          };
        };
      }
    ));
}
