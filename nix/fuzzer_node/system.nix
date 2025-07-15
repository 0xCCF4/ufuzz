{ config, pkgs, lib, packages, system, ... }:
{
  config =
    let
      scripts = (import ./scripts { inherit pkgs; });
    in
    {
      boot.kernelModules = [ "libcomposite" ];
      hardware.raspberry-pi."4".dwc2.enable = true;

      environment.systemPackages = with pkgs; [ packages."${system}".fuzzer_node ] ++ scripts;

      networking.firewall.allowedTCPPorts = [ 8000 ];
      networking.interfaces.eth0 = {
        ipv4.addresses = [
          {
            address = "10.83.3.198"; # change the network configuration for your project
            prefixLength = 24;
          }
        ];
      };
      networking.defaultGateway = "10.83.3.1"; # change this

      services.journald.extraConfig = "SystemMaxUse=8G"; # if you use a smaller SD card feel free to lower the max storage space

      systemd.services.fuzzer_node = {
        description = "Fuzzer Node Service";
        after = [ "network.target" ];
        wantedBy = [ "multi-user.target" ];

        serviceConfig = {
          ExecStart = "${packages."${system}".fuzzer_node}/bin/fuzzer_node";
          Restart = "always";
          RestartSec = 5;
          Type = "notify";
          WatchdogSec = "180s";
          TimeoutStopSec = "10s";
        };
      };

      systemd.services.fuzzer_start = {
        description = "Setup platform";
        wantedBy = [ "local-fs.target" ];
        serviceConfig.Type = "oneshot";
        serviceConfig.ExecStart = "${pkgs.writeScriptBin "platform-setup" ''
          #!${pkgs.stdenv.shell}
          /run/current-system/sw/bin/device_control init
        ''}/bin/platform-setup";
      };
    };
}
