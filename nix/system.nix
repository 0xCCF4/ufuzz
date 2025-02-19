{ config, pkgs, lib, settings, ... }:

let
  user = "thesis";
  SSID = "mxlan";
  interface = "wlan0";
  trusted_nix_keys = [ "laptop:zhWq+p6//VSVJiSKFitrqdJfzrJ1ajvPsXPz+M2n2Ao=" ];
  ssh_keys = [ "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILouqEVZdQe9lSB5QC0XIU15poExO4BAQDlMLLNkDwFn thesis" ];
in
{
  config =
    let
      pinctrl = (import ./pinctrl.nix { inherit pkgs; });
    in
    {
      networking.hostName = settings.hostName;

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

      boot.tmp = {
        useTmpfs = true;
        tmpfsSize = "512M";
      };

      systemd.services.nix-daemon.serviceConfig.TMPDIR = "/nixtmp";
      systemd.services.createtmp = {
        description = "Create /nixtmp";
        wantedBy = [ "local-fs.target" ];
        serviceConfig.Type = "oneshot";
        serviceConfig.ExecStart = "${pkgs.writeScriptBin "create-nixtmp" ''
          #!${pkgs.stdenv.shell}
          rm -Rf /nixtmp
          mkdir -p /nixtmp
        ''}/bin/create-nixtmp";
        requiredBy = [ "nix-daemon.service" ];
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

      hardware.bluetooth.powerOnBoot = false;

      fileSystems = {
        "/" = {
          device = "/dev/disk/by-label/NIXOS_SD";
          fsType = "ext4";
          options = [ "noatime" ];
        };
      };

      networking = {
        wireless = {
          enable = true;
          networks."${SSID}".pskRaw = "ext:WIFI_${SSID}_PASS";
          interfaces = [ interface ];
          secretsFile = "/wifi.key";
        };
      };

      environment.systemPackages = with pkgs; [ pinctrl wol rustup helix killall htop dig lsof file coreutils openssl wget bat eza fd fzf ripgrep age tldr nh nix-output-monitor nvd git ];

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
        imageBaseName = "thesis-${config.networking.hostName}";
        expandOnBoot = true;
      };

      hardware.enableRedistributableFirmware = true;
      system.stateVersion = "23.11";
    };
}
