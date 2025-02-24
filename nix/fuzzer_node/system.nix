{ config, pkgs, lib, packages, system, ... }:
{
  config =
    let
      scripts = (import ./scripts { inherit pkgs; });
    in
    {
      boot.kernelModules = [ "libcomposite" ];
      hardware.raspberry-pi."4".dwc2.enable = true;

      environment.systemPackages = with pkgs; [ packages."${system}".fuzzer_node v4l-utils nixos-firewall-tool ] ++ scripts;

      networking.firewall.allowedTCPPorts = [ 8000 ];
      networking.interfaces.eth0 = {
        ipv4.addresses = [
          {
            address = "192.168.0.10";
            prefixLength = 24;
          }
        ];
      };
      networking.defaultGateway = "192.168.0.1";
      networking.nameservers = [ "8.8.8.8" ];

      systemd.services.fuzzer_node = {
        description = "Fuzzer Node Service";
        after = [ "network.target" ];
        wantedBy = [ "multi-user.target" ];

        serviceConfig = {
          ExecStart = "${packages."${system}".fuzzer_node}/bin/fuzzer_node";
          ExecStop = "${pkgs.curl}/bin/curl -X GET http://localhost:8000/shutdown";
          Restart = "always";
          RestartSec = 5;
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
