{
  description = "Build fuzzer setup";
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-24.11";
    nixos-hardware.url = "github:nixos/nixos-hardware";
    crate2nix.url = "github:nix-community/crate2nix";
    deploy-rs.url = "github:serokell/deploy-rs";
    deploy-rs.inputs.nixpkgs.follows = "nixpkgs";
  };
  outputs = { self, nixpkgs, nixos-hardware, crate2nix, deploy-rs }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      pkgsFor = nixpkgs.legacyPackages;
    in
    rec {
      # Build fuzzer node system
      # Use nix build .#images.node to build the initial SD card image
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

      # Build fuzzer master system
      # Use nix build .#images.master to build the initial SD card image
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

      # Export fuzzer master and node executable
      packages = forAllSystems (system:
        {
          fuzzer_node = pkgsFor.${system}.callPackage ./fuzzer_node/package.nix { inherit nixpkgs system crate2nix; };
          fuzzer_master = pkgsFor.${system}.callPackage ./fuzzer_master/package.nix { inherit nixpkgs system crate2nix; };
        });

      # provide deloy-rs executable in dev shells
      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
          };
          deploy = deploy-rs.packages.${system}.deploy-rs;
        in
        {
          default = pkgs.mkShell {
            packages = [ deploy ];
          };
        });

      # Nix file formatter
      formatter.x86_64-linux = nixpkgs.legacyPackages.x86_64-linux.nixpkgs-fmt;

      # Deploy checks
      checks = builtins.mapAttrs (system: deployLib: deployLib.deployChecks self.deploy) deploy-rs.lib;

      # System deploy setup
      # use "nix develop" + "deploy" to deploy systems
      deploy.nodes.master = {
        hostname = "10.0.0.11";
        profiles.system = {
          sshUser = "thesis";
          user = "root";
          path = deploy-rs.lib.aarch64-linux.activate.nixos self.nixosConfigurations.master;
        };
      };
      deploy.nodes.node = {
        hostname = "10.0.0.10";
        profiles.system = {
          sshUser = "thesis";
          user = "root";
          path = deploy-rs.lib.aarch64-linux.activate.nixos self.nixosConfigurations.node;
        };
      };
    };
}
