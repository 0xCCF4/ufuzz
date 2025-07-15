# uFuzz
A x86 CPU fuzzer utilizing microcode coverage

## Overview
uFuzz is a CPU fuzzer that leverage custom microcode updates for x86 Intel
CPUs to extract e.g. microcode coverage information at runtime. For more details
see the [paper](xxx).

## Structure
uFuzz consists of three different systems:
1. The fuzzer agent: This is the target device that runs the device under test. We used the [Gigabyte Brix (GB-BPCE-3350C-BWUP)](https://www.gigabyte.com/de/Mini-PcBarebone/GB-BPCE-3350C-rev-10) with an Intel Apollo Lake (Celeron, Goldmont) N3350 processor (`CPUID[1].EAX=0x506ca`) ; vulnerable to the Red-unlock vulnerability.
2. A fuzzer instrumentor: This is a device that emulates a USB storage (for serving the UEFI app) and USB keyboard for skipping the BIOS screen automatically, further controls the power supply of the fuzzer device. (Raspberry Pi 4)
3. The fuzzer controller: The main fuzzing loop runs here, tasks are scheduled on the fuzzer device for execution. (Raspberry Pi 4)

## Project structure
The uFuzz project is structured as follows:

Component       | Description
--------------- | -----------
[`corpus-gen`](corpus-gen/) | Generates the corpus for initial fuzzing inputs. See the evaluation section of the paper. 
[`coverage`](coverage/) | Collects microcode coverage from the CPU by deploying microcode updates
[`custom_processing_unit`](custom_processing_unit/) | Contains utility function derived from [CustomProcessingUnit*](https://github.com/pietroborrello/CustomProcessingUnit).
[`data_types`](data_types/) | Contains shared data types for writing custom microcode updates.
[`fuzzer_data`](fuzzer_data/) | Contains shared data between the fuzzer instance and fuzzer master controller.
[`fuzzer_device`](fuzzer_device/) | Contains the implementation of the fuzzing agent, which runs on the target device/CPU.
[`fuzzer_master`](fuzzer_master/) | Contains the implementation of the fuzzer controler that controls a fuzzer agent.
[`fuzzer_node`](fuzzer_node/) | Contains the implementation of the fuzzer device instrumentor - emulating USB devices for the fuzzer device.
[`hypervisor`](hypervisor/) | Contains the implementation of the hypervisor environment.
[`literature_search`](literature_search/) | Contains a tool to search connected/related works by paper connections.
[`nix`](nix/) | Contains the definition of system of the fuzzer master and fuzzer instrumentor.
[`performance_timing`](performance_timing/) | Contains the tools to collect timing information from the fuzzer device,
[`performance_timing_macros`](performance_timing_macros/) | Contains utility macros to automate timing collection from target functions.
[`spec_fuzz`](spec_fuzz/) | Contains the implementation of the speculative micrcode fuzzer.
[`speculation_x86`](speculation_x86/) | Contains some test scenarios to check speculative execution behavior.
[`speculation_ucode`](speculation_ucode/) | Contains some test scenarios to check speculative execution behavior.
[`ucode_compiler_bridge`](ucode_compiler_bridge/) | Contains a bridge implementation to interface with the microcode compiler from [CustomProcessingUnit*](https://github.com/pietroborrello/CustomProcessingUnit) and preprocessor macros for deriving multi file microcode updates.
[`ucode_compiler_derive`](ucode_compiler_derive/)| Contains utility macros to automate the generation of microcode updates.
[`ucode_compiler_dynamic`](ucode_compiler_dynamic/) | Contains runtime mircocode update compilation.
[`ucode_dump`](ucode_dump/) | Contains microcode dumps of the CPU.
[`uefi_udp4`](uefi_udp4/) | Contains a basic UEFI driver implementation of UDP.
[`x86_perf_counter`](x86_perf_counter/) | Contains the implementation to use x86 performance counters. 
[`xtask`](xtask/) | Contains build automation for this project.

\* CustomProcessingUnit: Licensed under the Apache License, Version 2.0

## Install dependencies
To build and run the uFuzz project, you will need the Rust compiler with the nightly toolchain and UEFI target support.
```bash
# Install python (ubuntu/debian); required for the CustomProcessingUnit microcode compiler
sudo apt install python3 python3-click

# Install rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- --default-toolchain none -y # installs rustup
rustup install nightly-2024-09-06 # verified to work with the project
rustup target add x86_64-unknown-uefi # to compile UEFI applications
rustup target add aarch64-unknown-linux-gnu # to compile the fuzzer instrumentor
rustup target add x86_64-unknown-linux-gnu # to compile documentation
rustup default nightly-2024-09-06 # set the default toolchain to nightly
```

## Getting started
Download [CustomProcessingUnit](https://github.com/pietroborrello/CustomProcessingUnit) and 1. place it into the parent directory of this folder or 2. set the env var `UASM` to the
`uasm.py` file from CustomProcessingUnit. The uFuzz project uses the `uasm.py` script to compile microcode updates.

Then apply the following git-patch to `uasm.py`: [uasm.py.patch](ucode_compiler_bridge/uasm.py.patch).

To deploy the fuzzer instrumentor and fuzzer master, you will need `nix` installed (follow <https://nixos.org/download/> to install the package manager).
Go into the [`nix`}(nix/) directory, change the public ssh keys, IPs etc., to your likings, then, change IP settings within the [`fuzzer_master`](fuzzer_master/src/main.rs) project,
and run the following commands to build the SD card images for the PIs (`|& nom` is optional):
```bash
nix build .#images.master |& nom
nix build .#images.node |& nom
```
This builds SD card images for the fuzzer master and fuzzer instrumentor. After initial setup,
further changes may be deployed using (deploy-rs) by running in the `nix` directory (`|& nom` is optional):
```bash
nix run |& nom
```

To built and deploy the fuzzer device UEFI app:
```bash
HOST_NODE="put IP of the instrumentor here" cargo xtask put-remote --remote-ip {address of fuzzer controller} --source-ip {address of agent} --netmask {network mask} --port {udp port} --startup {app name here}
```
Depending on the target fuzzing scenario, use `spec_fuzz` (speculative microcode fuzzing) or `fuzzer_device` (x86 instruction fuzzing)
instead of `{app name here}`.

Then boot the fuzzer master. Start the `fuzzer_master` app. Settings can be displayed by using `--help` in the CLI.

---

The fuzzing results will be stored to a database file (specifiable via the `--database` argument) (fuzzer master) 
and can be viewed by running (fuzzer master):
```bash
fuzz_viewer /home/thesis/database.json
```

---

The following binaries are provided for the fuzzing master:

| Binary              | Description                                                                                    |
|---------------------|------------------------------------------------------------------------------------------------|
| `fuzzer_master`     | Main fuzzing controller, main fuzzing loop + manual execution, list all options using `--help` |
| `fuzz_viewer`       | Tool for viewing and analyzing fuzzing results from the database                               |
| `fuzz_compare`      | Compares execution results from different manual fuzzing input executions                      |
| `spec_compare`      | Analyzes and compares results from speculative execution fuzzing campaigns                     |
| `spec_new_database` | Creates new speculative execution databases from templates (copies exclusions)                 |
| `fuzz_combine`      | Combines multiple fuzzing databases into a single output database                              |
| `afl_convert`       | Converts AFL fuzzing findings to inputs usable with the manual execution fuzzing tool          |

## Documentation
Generate the rust doc documentation using the following command:
```bash
cargo xtask doc
```
The results will be placed inside the build directory `target/doc`

## Utilities
The `cargo xtask` command contains utilities to, for example, test the hypervisor environment on a simulated
CPU using the bochs emulator; allowing easier debugging.
```bash
cargo xtask emulate hypervisor bochs-intel
```

List all available xtask commands using:
```bash
cargo xtask --help
```