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
