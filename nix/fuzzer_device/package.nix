{ pkgs, rust-overlay, nixpkgs, system }:
let
  manifest = (pkgs.lib.importTOML ../../fuzzer_device/Cargo.toml).package;

  uasm_patch = ./../../ucode_compiler_bridge/uasm.py.patch;

  uasm = pkgs.stdenv.mkDerivation rec {
    pname = "fuzzer_device source env";
    version = manifest.version;
    src =
      pkgs.fetchFromGitHub {
        owner = "pietroborrello";
        repo = "CustomProcessingUnit";
        rev = "4237524fe7545c66e42dd986113f220662c06f6a";
        hash = "sha256-Bd2Q47ZksoEzTesbtAbW4p8qCHUMYg/zXzjKQt4IzLo=";
        name = "CustomProcessingUnit";
      };

    dontUnpack = true;

    installPhase = ''
      mkdir -p $out

      mkdir -p wkd
      cp -r $src/* wkd/
      chmod +wx wkd/uasm-lib/
      install -Dm755 $src/uasm-lib/uasm.py wkd/uasm-lib/uasm.py

      # apply patch
      patch wkd/uasm-lib/uasm.py < ${uasm_patch}

      chmod -w wkd/uasm-lib/

      cp -r wkd/* $out/
    '';

    passthru = {
      uasm_path = "${uasm}/uasm-lib/uasm.py";
    };

    propagatedBuildInputs = [
      (pkgs.python314.withPackages (pythonPackages: with pythonPackages; [
        click
      ]))
    ];
  };

  rustPkgs = import nixpkgs {
    inherit system;
    overlays = [ (import rust-overlay) ];
  };

  rustWithUefiTarget = rustPkgs.rust-bin.nightly."2024-09-06".default.override {
    targets = [ "x86_64-unknown-uefi" ];
  };

  rustPlatformUefi = rustPkgs.makeRustPlatform {
    cargo = rustWithUefiTarget;
    rustc = rustWithUefiTarget;
  };

in
rustPlatformUefi.buildRustPackage
rec {
  pname = manifest.name;
  version = manifest.version;
  cargoLock.lockFile = ../../Cargo.lock;
  srcs = pkgs.lib.cleanSource ./../..;

  buildPhase = ''
    CC=clang cargo build --release -p fuzzer_device --target x86_64-unknown-uefi --bins
  '';

  installPhase = ''
    mkdir -p $out/bin
    cp target/x86_64-unknown-uefi/release/fuzzer_device $out/bin/fuzzer_device
  '';

  UASM = uasm.uasm_path;
}
