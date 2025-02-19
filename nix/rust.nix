project_name:
{ pkgs, rust-overlay, nixpkgs, system }:
let
  manifest = (pkgs.lib.importTOML ../${project_name}/Cargo.toml).package;

  rustPkgs = import nixpkgs {
    inherit system;
    overlays = [ (import rust-overlay) ];
  };

  target = if system == "x86_64-linux" then "x86_64-unknown-linux-gnu" else if system == "aarch64-linux" then "aarch64-unknown-linux-gnu" else throw "Unsupported system: ${system}";

  rustPlatformEnv = rustPkgs.rust-bin.nightly."2024-09-06".default.override {
    targets = [ target "x86_64-unknown-linux-gnu" ];
  };

  rustPlatform = rustPkgs.makeRustPlatform {
    cargo = rustPlatformEnv;
    rustc = rustPlatformEnv;
  };

in
rustPlatform.buildRustPackage
rec {
  pname = manifest.name;
  version = manifest.version;
  cargoLock.lockFile = ../Cargo.lock;
  srcs = pkgs.lib.cleanSource ./..;

  cargoBuildFlags = [
    "-p"
    "${project_name}"
    "--target"
    target
  ];

  checkPhase = "true";
}
