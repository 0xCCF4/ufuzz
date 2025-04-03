{ pkgs ? import <nixpkgs> }:

let
  version = "660bba39805585fb39d37d49b8268073932696bb";
  pname = "pinctrl";

  pkgConfig = pkgs.writeTextFile {
    name = "${pname}.pc";
    text = ''
      prefix=@out@
      exec_prefix=''${prefix}
      includedir=''${prefix}/include
      libdir=''${prefix}/lib

      Name: pinctrl
      Description: GPIO library for Raspberry Pi computers
      Version: ${version}
      Libs: -L''${libdir} -lpinctrl -lpthread -lm
      Cflags: -I''${includedir}
    '';
  };

in
pkgs.stdenv.mkDerivation rec {
  inherit pname version;

  src = pkgs.fetchFromGitHub {
    owner = "raspberrypi";
    repo = "utils";
    rev = version;
    sha256 = "sha256-ItknnAWicXK1pfAVbSgSBj1SAx7oxnhsAZsRyaqfCDM=";
  };

  nativeBuildInputs = [
    pkgs.cmake
  ];

  buildInputs = [
    pkgs.glibc
  ];

  patches = [
    ./pinctrl.patch
  ];

  meta = with pkgs.lib; {
    description = "GPIO library for the Raspberry Pi";
    homepage = "https://github.com/raspberrypi/utils/tree/master/pinctrl";
    license = licenses.unlicense;
    platforms = platforms.unix;
  };
}
