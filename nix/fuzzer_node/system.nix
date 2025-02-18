{ config, pkgs, lib, ... }:

let
  user = "thesis";
  SSID = "mxlan";
  interface = "wlan0";
  hostname = "thesis";
  trusted_nix_keys = [ "laptop:zhWq+p6//VSVJiSKFitrqdJfzrJ1ajvPsXPz+M2n2Ao=" ];
  ssh_keys = [ "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILouqEVZdQe9lSB5QC0XIU15poExO4BAQDlMLLNkDwFn thesis" ];
in
{
  config = let 
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
  in{
    boot.kernelModules = [ "libcomposite" ];

    nix.settings.trusted-public-keys = trusted_nix_keys;

    nix.settings.experimental-features = [
      "nix-command"
      "flakes"
    ];

    nix.settings.auto-optimise-store = true;

    nix.gc = {
      automatic = true;
      dates = "daily";
      options = "--delete-older-than 1w";
    };

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
        enable = true;
        networks."${SSID}".pskRaw = "ext:WIFI_${SSID}_PASS";
        interfaces = [ interface ];
        secretsFile = "/wifi.key";
      };
    };

    environment.systemPackages = with pkgs; [ redetect_usb configure_usb skip_bios rustup helix killall htop dig lsof file coreutils openssl wget bat eza fd fzf ripgrep age tldr nh nix-output-monitor nvd git ];

    services.openssh.enable = true;

    users = {
      mutableUsers = false;
      users."${user}" = {
        isNormalUser = true;
        extraGroups = [ "wheel" ];
        openssh.authorizedKeys.keys = ssh_keys;
      };
    };
    security.sudo.wheelNeedsPassword = false;

    time.timeZone = "Europe/Berlin";
    i18n.defaultLocale = "en_US.UTF-8";
    console.keyMap = "de";

    sdImage = {
      imageBaseName = "thesis-system";
      expandOnBoot = true;
    };

    hardware.enableRedistributableFirmware = true;
    system.stateVersion = "23.11";
  };
}
