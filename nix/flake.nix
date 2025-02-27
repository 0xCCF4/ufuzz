{
  description = "Build fuzzer setup";
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-24.11";
    nixos-hardware.url = "github:nixos/nixos-hardware";
    crate2nix.url = "github:nix-community/crate2nix";
  };
  outputs = { self, nixpkgs, nixos-hardware, crate2nix }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      pkgsFor = nixpkgs.legacyPackages;
    in
    rec {
      nixosConfigurations.node = nixpkgs.lib.nixosSystem rec {
        system = "aarch64-linux";
        modules = [
          nixos-hardware.nixosModules.raspberry-pi-4
          "${nixpkgs}/nixos/modules/installer/sd-card/sd-image-aarch64.nix"
          ./fuzzer_node/system.nix
          ./system.nix
        ];
        specialArgs = {
          settings.hostName = "fuzzer-node";
          settings.ip = "10";
          inherit packages system;
        };
      };
      images.node = nixosConfigurations.node.config.system.build.sdImage;

      nixosConfigurations.master = nixpkgs.lib.nixosSystem rec {
        system = "aarch64-linux";
        modules = [
          nixos-hardware.nixosModules.raspberry-pi-4
          "${nixpkgs}/nixos/modules/installer/sd-card/sd-image-aarch64.nix"
          ./fuzzer_master/system.nix
          ./system.nix
        ];
        specialArgs = {
          settings.hostName = "fuzzer-master";
          settings.ip = "11";
          inherit packages system;
        };
      };
      images.master = nixosConfigurations.master.config.system.build.sdImage;

      packages = forAllSystems (system: {
        fuzzer_node = pkgsFor.${system}.callPackage ./fuzzer_node/package.nix { inherit nixpkgs system crate2nix; };
        fuzzer_master = pkgsFor.${system}.callPackage ./fuzzer_master/package.nix { inherit nixpkgs system crate2nix; };
      });

      formatter.x86_64-linux = nixpkgs.legacyPackages.x86_64-linux.nixpkgs-fmt;
    };
}
