# System configuration

This folder contatins the configuration of the raspberry PI systems (fuzzing instrumentor and controller).

To provision the lab setup:
1. Edit the top section of the `system.nix` configuration file. This is the shared configuration between both system.
2. Edit `fuzzer_master/system.nix` and change the network config.
3. Edit `fuzzer_node/system.nix` and change the network config.

```bash
nix build .#images.master |& nom
 # result is stored in `result/`
nix build .#images.node |& nom
 # result is stored in `result/`
```
This builds SD card images for the fuzzer master and fuzzer instrumentor. After initial setup,
further changes may be deployed using (deploy-rs) by running in the `nix` directory (`|& nom` is optional):
```bash
nix run |& nom
```
