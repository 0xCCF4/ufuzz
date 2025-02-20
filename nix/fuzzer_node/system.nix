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

      networking.firewall.allowedTCPPorts = [ 4444 ];

      systemd.services.fuzzer_node = {
        description = "Fuzzer Node Service";
        after = [ "network.target" ];
        wantedBy = [ "multi-user.target" ];

        serviceConfig = {
          ExecStart = "${packages."${system}".fuzzer_node}/bin/fuzzer_node";
          Restart = "always";
          RestartSec = 5;
        };
      };
    };
}
