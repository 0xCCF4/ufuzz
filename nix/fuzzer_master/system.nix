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
        conflicts = ["fuzzer_master_corpus.service" "spec_fuzz.service"];

        enable = false;

        serviceConfig = {
          ExecStart = "${packages."${system}".fuzzer_master}/bin/fuzz_master genetic";
          WorkingDirectory = "/home/thesis";
          Restart = "always";
          RestartSec = 5;
        };
        environment = {
          RUST_LOG = "trace";
        };
      };

      systemd.services.fuzzer_master_corpus = {
        description = "Fuzzer Master Service with corpus";
        after = [ "network.target" ];
        wantedBy = [ "multi-user.target" ];
        conflicts = ["fuzzer_master.service" "spec_fuzz.service"];

        enable = false;

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

      systemd.services.spec_fuzz = {
        description = "Spec Fuzz Service";
        after = [ "network.target" ];
        wantedBy = [ "multi-user.target" ];
        conflicts = ["fuzzer_master.service" "fuzzer_master_corpus.service"];

        enable = false;

        serviceConfig = {
          ExecStart = "${packages."${system}".fuzzer_master}/bin/fuzz_master spec /home/thesis/spec_fuzz.json";
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
