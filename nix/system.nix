{ config, pkgs, lib, settings, ... }:

let
  user = "thesis";
  password = "abi2mf81l0sm";
  SSID = "mxlan";
  interface = "wlan0";
  trusted_nix_keys = [ "laptop:zhWq+p6//VSVJiSKFitrqdJfzrJ1ajvPsXPz+M2n2Ao=" ];
  ssh_keys = [ "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILouqEVZdQe9lSB5QC0XIU15poExO4BAQDlMLLNkDwFn thesis" ];
  wg_pubkey = "FYOAl5u+cZ0sb8jwgSF9OeeBE0pkN/4l3W53BX7DuQ0=";
  wg_endpoint = "5.252.225.58:51820";
in
{
  config =
    let
      pinctrl = (import ./pinctrl.nix { inherit pkgs; });
    in
    {
      networking.hostName = settings.hostName;
      networking.nameservers = [ "9.9.9.9" ];

      nix.settings.trusted-public-keys = trusted_nix_keys;

      nix.settings.experimental-features = [
        "nix-command"
        "flakes"
      ];

      nix.settings.auto-optimise-store = true;

      nix.gc = {
        automatic = true;
        dates = "daily";
        options = "--delete-older-than 1d";
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
          rm -Rf /nixtmp || true
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

      networking.wg-quick.interfaces.mx = {
        privateKeyFile = "/wg.key";
        address = [ "10.0.0.${settings.ip}/24" ];
        listenPort = 51820;
        autostart = true;
        peers = [
          {
            publicKey = wg_pubkey;
            allowedIPs = [
              "10.0.0.0/24"
            ];
            endpoint = wg_endpoint;
            persistentKeepalive = 25;
          }
        ];
      };
      networking.firewall.allowedUDPPorts = [ 51820 ];

      environment.systemPackages = with pkgs; [ nushell nixos-firewall-tool pinctrl wol rustup helix tmux killall htop dig lsof file coreutils openssl wget bat eza fd fzf ripgrep age tldr nh nix-output-monitor nvd git wireguard-tools ];

      services.openssh.enable = true;

      users = {
        mutableUsers = false;
        users."${user}" = {
          isNormalUser = true;
          extraGroups = [ "wheel" ];
          openssh.authorizedKeys.keys = ssh_keys;
          password = password;
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
