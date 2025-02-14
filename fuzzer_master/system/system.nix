{ config, pkgs, lib, ... }:

let
  user = "thesis";
  password = "thesis";
  SSID = "<ssid>";
  SSIDpassword = "<pass>";
  interface = "wlan0";
  hostname = "thesis";
in
{
  config = {
    nixpkgs.overlays = [
      (final: super: {
        makeModulesClosure = x:
          super.makeModulesClosure (x // { allowMissing = true; });
      })
    ];

    boot = {
      kernelPackages = pkgs.linuxKernel.packages.linux_rpi4;
      initrd.availableKernelModules = [ "xhci_pci" "usbhid" "usb_storage" ];
      loader = {
        grub.enable = false;
        generic-extlinux-compatible.enable = true;
      };
    };

    hardware.raspberry-pi."4".dwc2.enable = true;
    hardware.bluetooth.powerOnBoot = false;

    fileSystems = {
      "/" = {
        device = "/dev/disk/by-label/NIXOS_SD";
        fsType = "ext4";
        options = [ "noatime" ];
      };
    };

    networking = {
      hostName = hostname;
      wireless = {
        enable = false;
        #networks."${SSID}".psk = SSIDpassword;
        #interfaces = [ interface ];
      };
    };

    environment.systemPackages = with pkgs; [ rustup helix killall htop dig lsof file coreutils openssl wget bat eza fd fzf ripgrep age tldr nh nix-output-monitor nvd git ];

    services.openssh.enable = true;

    users = {
      mutableUsers = false;
      users."${user}" = {
        isNormalUser = true;
        password = password;
        extraGroups = [ "wheel" ];
      };
    };

    time.timeZone = "Europe/Berlin";
    i18n.defaultLocale = "de_DE.UTF-8";
    console.keyMap = "de";

    sdImage = {
        imageBaseName = "thesis-system";
        expandOnBoot = true;
    };

    hardware.enableRedistributableFirmware = true;
    system.stateVersion = "23.11";
  };
}
