{ config, pkgs, lib, packages, ... }:
{
  config =
    let
      configure_usb = pkgs.writeShellApplication {
        name = "configure_usb";

        text = builtins.readFile ./configure_usb.sh;
      };
      skip_bios = pkgs.writeShellApplication {
        name = "skip_bios";

        text = builtins.readFile ./skip_bios.sh;
      };
      redetect_usb = pkgs.writeShellApplication {
        name = "redetect_usb";

        text = builtins.readFile ./redetect_usb.sh;
      };
      reboot_child = pkgs.writeShellApplication {
        name = "reboot_child";

        text = builtins.readFile ./reboot_child.sh;
      };
    in
    {
      boot.kernelModules = [ "libcomposite" ];
      hardware.raspberry-pi."4".dwc2.enable = true;

      environment.systemPackages = with pkgs; [ reboot_child redetect_usb configure_usb skip_bios packages.aarch64-linux.fuzzer_node ];
    };
}
