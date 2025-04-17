{
  description = "Generate initial fuzzing corpus.";
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-24.11";
  };
  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in
    rec {
      # Export fuzzer master and node executable
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            system = "x86_64-linux"; # always build the executables for x86_64 arch !
          };

          # This are the software libraries from which the fuzzing corpus is generated
          libs = with pkgs; [ libcxx libz libzip ];
        in
        {
          default = pkgs.stdenv.mkDerivation {
            name = "libraries";
            src = ./.;
            propagatedBuildInputs = libs;
            installPhase = ''
              mkdir -p $out/lib
              for pkgs in ${pkgs.lib.concatStringsSep " " (map (lib: "${lib}") libs)}; do
                echo "Extracting from $pkgs"
                find $pkgs -type f -name "*.a" -exec ln -s {} $out/lib/ \;
                find $pkgs -type f -name "*.so*" -exec ln -s {} $out/lib/ \;
              done
            '';
          };
        }
      );

      formatter.x86_64-linux = nixpkgs.legacyPackages.x86_64-linux.nixpkgs-fmt;
    };
}
