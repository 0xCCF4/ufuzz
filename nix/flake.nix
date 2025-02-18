{
  description = "Build fuzzer setup";
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-24.11";
    nixos-hardware.url = "github:nixos/nixos-hardware";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };
  outputs = { self, nixpkgs, nixos-hardware, rust-overlay }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      pkgsFor = nixpkgs.legacyPackages;
    in
    rec {
      nixosConfigurations.rpi4 = nixpkgs.lib.nixosSystem {
        system = "aarch64-linux";
        modules = [
          nixos-hardware.nixosModules.raspberry-pi-4
          "${nixpkgs}/nixos/modules/installer/sd-card/sd-image-aarch64.nix"
          ./fuzzer_node/system.nix
        ];
      };
      images.rpi4 = nixosConfigurations.rpi4.config.system.build.sdImage;

      packages = forAllSystems (system: {
        fuzzer_device = pkgsFor.${system}.callPackage ./fuzzer_device/package.nix { inherit nixpkgs system rust-overlay; };
        #fuzzer_node = pkgsFor.${system}.callPackage ./fuzzer_node/package.nix {};
      });

      formatter.x86_64-linux = nixpkgs.legacyPackages.x86_64-linux.nixpkgs-fmt;
    };
}
