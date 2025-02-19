{ pkgs ? import <nixpkgs> }:

let
  version = "master";
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
    sha256 = "sha256-rgBnBRo4KJI0qE9pLpGLraGL+cqWzm4rbNmOzM/wOwU=";
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
