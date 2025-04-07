project_name:
{ pkgs, crate2nix, nixpkgs, system }:
let
  cargoNix = import ../Cargo.nix {
    inherit pkgs;
    inherit nixpkgs;
  };

in
cargoNix.workspaceMembers.${project_name}.build
