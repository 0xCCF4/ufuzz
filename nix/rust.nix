project_name:
{ pkgs, crate2nix, nixpkgs, system }:
let
  cargoNix = crate2nix.tools.${system}.appliedCargoNix {
    name = "rustnix";
    src = ./..;
  };

in
cargoNix.workspaceMembers.${project_name}.build
