# uFuzz
A x86 CPU fuzzer utilizing microcode coverage

## Overview
uFuzz is the first x86 CPU fuzzer that leverages microcode coverage information as feedback to guide the fuzzing campaign. For more details see the [paper](xxx).

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

# Install compiler
sudo apt install gcc-aarch64-linux-gnu build-essential git

# Install rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- --default-toolchain none -y # installs rustup
rustup install nightly-2025-05-30 # verified to work with the project
rustup target add x86_64-unknown-uefi # to compile UEFI applications
rustup target add aarch64-unknown-linux-gnu # to compile the fuzzer instrumentor
rustup target add x86_64-unknown-linux-gnu # to compile documentation
rustup default nightly-2025-05-30 # set the default toolchain to nightly
```

## Getting started
Download [CustomProcessingUnit](https://github.com/pietroborrello/CustomProcessingUnit) and 1. place it into the parent directory of this folder or 2. set the env var `UASM` to the
`uasm.py` file from CustomProcessingUnit. The uFuzz project uses the `uasm.py` script to compile microcode updates.

Then apply the following git-patch to `uasm.py`: [uasm.py.patch](ucode_compiler_bridge/uasm.py.patch).

To deploy the fuzzer instrumentor and fuzzer master, you will need `nix` installed (follow <https://nixos.org/download/> to install the package manager).
Go into the [`nix`](nix/) directory, change the public ssh keys, IPs etc., to your likings,
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
HOST_NODE="put IP of the instrumentor here" cargo xtask put-remote
  --remote-ip {address of fuzzer controller}
  --source-ip {address of agent}
  --netmask {network mask}
  --port {udp port}
  --startup {app name here}
```
Depending on the target fuzzing scenario, use `spec_fuzz` (speculative microcode fuzzing) or `fuzzer_device` (x86 instruction fuzzing)
instead of `{app name here}`.

Then boot the fuzzer master. Start the `fuzzer_master` app. Settings can be displayed by using `--help` in the CLI.
          
```bash
fuzzer_master --help

# Main fuzzing application. This app governs and controls the entire fuzzing process,
# issuing commands to a fuzzer agent (which e.g. executes fuzzing inputs on its CPU)
# 
# Usage: fuzz_master [OPTIONS] <COMMAND>
# 
# Commands:
#   genetic               Perform coverage fuzzing using (bad) genetic mutation algorithm,
#                         probably you would like to execute the `afl` command. == Requires the `fuzzer_device`
#                         app running on the agent ==
#   instruction-mutation  Under-construction
#   init                  Bring up the fuzzer agent to a usable state
#   reboot                Reboot the fuzzer agent
#   cap                   Report the capabilities of the fuzzer agent
#   performance           Extract performance values from the fuzzer agent
#   spec                  Do speculative microcode fuzzing == Requires the `spec_fuzz` app running on the agent ==
#   spec-manual           Executes a given speculative fuzzing payload manually == Requires the `spec_fuzz`
#                         app running on the agent ==
#   manual                Executes a single fuzzing input manually == Requires the `fuzzer_device`
#                         app running on the agent ==
#   bulk-manual           Executes a corpus of fuzzing inputs; essentially runs the manual command using
#                         all files within the given directory == Requires the `fuzzer_device` app running on the agent ==
#   afl                   Executes the main fuzzing loop with AFL mutations == Requires the `fuzzer_device` app
#                         running on the agent ==
#   help                  Print this message or the help of the given subcommand(s)
# 
# Options:
#   -d, --database <DATABASE>          The database file to save fuzzing progress and results to
#       --instrumentor <INSTRUMENTOR>  Address of the fuzzer instrumentor [default: http://10.83.3.198:8000]
#       --agent <AGENT>                Address of the fuzzer agent [default: 10.83.3.6:4444]
#   -h, --help                         Print help
#   -V, --version                      Print version
```

Each mentioned `fuzz_master` commands (experiments) includes built-in help documentation that allows you to specify parameters. For example:

- For running the fuzzing campaign with AFL mutations:
``` bash
fuzz_master afl --help

# Executes the main fuzzing loop with AFL mutations == Requires the `fuzzer_device` app running on the agent ==

# Usage: fuzz_master afl [OPTIONS]

# Options:
#   -s, --solutions <SOLUTIONS>          Store findings to this path
#   -c, --corpus <CORPUS>                Use the provided corpus file to generate initial fuzzing inputs from
#   -a, --afl-corpus <AFL_CORPUS>        Store the fuzzing corpus to that path
#   -t, --timeout-hours <TIMEOUT_HOURS>  End fuzzing after that many hours automatically, if not set fuzzing does not terminate
#   -d, --disable-feedback               Disabled coverage fuzzing feedback; instead fuzzing feedback is randomized
#   -p, --printable-input-generation     When not using the `corpus` argument; initial fuzzing inputs are random byte sequences; enabling this flag these byte sequences are selected among printable ASCII characters
#   -h, --help                           Print help
```

- For running the fuzzing campaign with pure genetic mutations:
``` bash
fuzz_master genetic --help

# Perform coverage fuzzing using (bad) genetic mutation algorithm, probably you would like to execute the `afl` command. == Requires the `fuzzer_device` app running on the agent ==

# Usage: fuzz_master genetic [OPTIONS]
# 
# Options:
#   -c, --corpus <CORPUS>                A corpus file to generate the initial fuzzing inputs from
#   -t, --timeout-hours <TIMEOUT_HOURS>  After how many hours the fuzzing should be terminated. If not given fuzzing must be interrupted by CTRL+C
#   -d, --disable-feedback               Disable the feedback loop, fuzzing input ratings will become randomized
#   -h, --help                           Print help
```

- For running the fuzzing campaign with speculative microcode fuzzing:
``` bash
fuzz_master spec --help

# Do speculative microcode fuzzing == Requires the `spec_fuzz` app running on the agent ==
# 
# Usage: fuzz_master spec [OPTIONS] <REPORT>
# 
# Arguments:
#   <REPORT>  Path to database; save the results to this file
# 
# Options:
#   -a, --all                Execute fuzzing for all instructions extracted from MSROM
#   -s, --skip               Skip instruction that were already run; continue a stopped fuzzing execution
#   -n, --no-crbus           Skip all CRBUS related instructions
#   -e, --exclude <EXCLUDE>  Exclude a list of instructions from running through the fuzzer
#   -f, --fuzzy-pmc          Run all PMC variants through the fuzzer; takes a long time
#   -h, --help               Print help
```
---

The fuzzing results will be stored to a database file (specifiable via the `--database` argument) (fuzzer master) 
and can be viewed by running (fuzzer master):
```bash
fuzz_viewer database.json
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

# A build and test assist program
# 
# Usage: xtask <COMMAND>
# 
# Commands:
#   emulate         Emulate a UEFI application using BOCHS CPU emulator
#   put-remote      Push an UEFI app onto the remote machine
#   control-remote  Control a remote machine
#   update-node     Update the node's software, systemd service, etc
#   doc             Generate documentation
#   check           Compile all examples and subprojects
#   help            Print this message or the help of the given subcommand(s)
# 
# Options:
#   -h, --help  Print help
```

### Cite Us

Our work has been published as a [paper](xxx) at Network and Distributed System Security Symposium 2026 ([NDSS'26](https://www.ndss-symposium.org/ndss2026/)): 
```
@inproceedings{TBD}
```