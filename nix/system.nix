{ config, pkgs, lib, settings, ... }:

let
    /*
    Replace these configuration values to your likings.
    */
  user = "fuzz"; # default user name (used across the whole build system)
  password = "abi2mf81l0sm"; # initial user's password, be aware when you commit this project that the initial password will be committed - make sure to passwd change it
  SSID = "yourWifiSSID"; # wifi SSID, wifi password is fetched from on-device file /wifi.key (which must include the line WIFI_{SSID}_PASS={PASSWORD}
  interface = "wlan0"; # wifi interface, when using a raspberry pi, leave as is
  trusted_nix_keys = [ "deployMachine:zhWq+p6//VSVJiSKFitrqdJfzrJ1ajvPsXPz+M2n2Ao=" ]; # when updating from other host, change this to your public nix signing key (not required for initial provisioning; via SD card image) - see https://docs.nixbuild.net/signing-keys/
  ssh_keys = [ # SSH keys that may connect to the user
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILouqEVZdQe9lSB5QC0XIU15poExO4BAQDlMLLNkDwFn"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMq/HVkrYPFG+zjYDluufADU37TlQGAowFeWI4f8vrG5"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGmAJWLk4ovGhb32f5u2R7Q08zONOo6GcgoQ0bSoIS8p"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINdkuuDi9oylTQPjNi0S4xKRno0KguR5JK2CTolvaL2q"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIP//WksskSNCSkYovQaZwKom6kRH2CdzVSO3zSrt5MHN"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEOEZ4fZluR+mdtCB/1HfwxVc346iH/B1HwkppuXoCMi"
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOSEwHr9Z0zHA/TaQgoOOXWbMnK+BDtk7jmVCGPhab2p"
  ];
  # In our lab setup, we connect our devices to an wireguard server, private key in stored in /wg.key on device
  # Feel free to set wg_endpoint to null to disable wireguard
  wg_endpoint = "1.1.1.1:51820"; # connect to this wireguard endpoint
  wg_pubkey = "FYOAl5u+cZ0sb8jwgSF9OeeBE0pkN/4l3W53BX7DuQ0="; # using this public key
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

      networking.wg-quick.interfaces.mx = lib.mkIf (wg_endpoint != null) {
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

      programs.nix-ld.enable = true;

      users = {
        mutableUsers = false;
        users."${user}" = {
          isNormalUser = true;
          extraGroups = [ "wheel" ];
          openssh.authorizedKeys.keys = ssh_keys;
          initialPassword = password;
        };
      };
      security.sudo.wheelNeedsPassword = false;

      time.timeZone = "Europe/Berlin";
      i18n.defaultLocale = "en_US.UTF-8";
      console.keyMap = "de";

      sdImage = {
        imageBaseName = "fuzz-${config.networking.hostName}";
        expandOnBoot = true;
      };

      hardware.enableRedistributableFirmware = true;
      system.stateVersion = "23.11";
    };
}
