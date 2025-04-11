{ config, pkgs, lib, packages, system, ... }:
{
  config =
    {
      environment.systemPackages = with pkgs; [ packages."${system}".fuzzer_master ];

      networking.firewall.allowedTCPPorts = [ 8000 ];
      networking.interfaces.eth0 = {
        ipv4.addresses = [
          {
            address = "10.83.3.250";
            prefixLength = 24;
          }
        ];
      };
      networking.defaultGateway = "10.83.3.1";

      services.journald.extraConfig = "SystemMaxUse=64G";

      systemd.services.fuzzer_master = {
        description = "Fuzzer Master Service";
        after = [ "network.target" ];
        wantedBy = [ "multi-user.target" ];

        serviceConfig = {
          ExecStart = "${packages."${system}".fuzzer_master}/bin/fuzz_master genetic /home/thesis/corpus.json";
          WorkingDirectory = "/home/thesis";
          Restart = "always";
          RestartSec = 5;
        };
        environment = {
          RUST_LOG = "trace";
        };
      };
    };
}
