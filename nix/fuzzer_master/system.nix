{ config, pkgs, lib, packages, system, ... }:
{
  config =
    {
      environment.systemPackages = with pkgs; [ packages."${system}".fuzzer_master ];

      networking.firewall.allowedTCPPorts = [ 8000 ];
      # networking.interfaces.eth0 = {
      #   ipv4.addresses = [
      #     {
      #       address = "192.168.0.11";
      #       prefixLength = 24;
      #     }
      #   ];
      # };
      # networking.defaultGateway = "192.168.0.1";
      # networking.nameservers = [ "8.8.8.8" ];

      systemd.services.fuzzer_master = {
        description = "Fuzzer Master Service";
        after = [ "network.target" ];
        wantedBy = [ "multi-user.target" ];

        serviceConfig = {
          ExecStart = "${packages."${system}".fuzzer_master}/bin/fuzzer_master";
          WorkingDirectory = "/home/thesis";
          Restart = "always";
          RestartSec = 5;
        };
      };
    };
}
