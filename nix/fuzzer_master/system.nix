{ config, pkgs, lib, packages, system, ... }:
{
  config =
    {
      environment.systemPackages = with pkgs; [ packages."${system}".fuzzer_master ];

      networking.firewall.allowedTCPPorts = [ 8000 ];
      networking.interfaces.eth0 = {
        ipv4.addresses = [
          {
            address = "10.83.3.250"; # change the network configuration for your project
            prefixLength = 24;
          }
        ];
      };
      networking.defaultGateway = "10.83.3.1"; # change this

      services.journald.extraConfig = "SystemMaxUse=64G"; # if you use a smaller SD card feel free to lower the max storage space
    };
}
