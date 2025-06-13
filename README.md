# uFuzz
A x86 CPU fuzzer utilizing microcode coverage

## Overview
uFuzz is a CPU fuzzer that leverage custom microcode updates for x86 Intel
CPUs to extract e.g. microcode coverage information at runtime. For more details
see the [paper](xxx).

## Structure
uFuzz consists of three different systems:
1. The fuzzer agent: This is the target device that runs the fuzzer. We used the [Gigabyte Brix (GB-BPCE-3350C-BWUP)](https://www.gigabyte.com/de/Mini-PcBarebone/GB-BPCE-3350C-rev-10) with an Intel Apollo Lake (Celeron, Goldmont) N3350 processor (`CPUID[1].EAX=0x506ca`) ; vulnerable to the Red-unlock vulnerability.
2. A fuzzer watchdog: This is a device that emulates an USB storage (for serving the UEFI app) and USB keyboard for skipping the BIOS screen automatically and controls the power supply of the fuzzer device. (Raspberry Pi 4)
3. The fuzzer controller: The main fuzzing loop runs here, tasks are scheduled on the fuzzer device for execution. (Raspberry Pi 4)

## Project structure
The uFuzz project is structured as follows:

Component       | Description
--------------- | -----------
[`corpus-gen`](corpus-gen/) | Generates the corpus for initial fuzzing inputs. See the evaluation section of the paper. 
[`coverage`](coverage/) | Collects microcode coverage from the CPU
[`custom_processing_unit`](custom_processing_unit/) | Contains utility function derived from [CustomProcessingUnit](https://github.com/pietroborrello/CustomProcessingUnit).
[`data_types`](data_types/) | Contains shared data types for writing custom microcode updates.
[`fuzzer_data`](fuzzer_data/) | Contains shared data between the fuzzer instance and fuzzer master controller.
[`fuzzer_device`](fuzzer_device/) | Contains the implementation of the fuzzing agent, which runs on the target device/CPU.
[`fuzzer_master`](fuzzer_master/) | Contains the implementation of the fuzzer controler that controls a fuzzer agent.
[`fuzzer_node`](fuzzer_node/) | Contains the implementation of the fuzzer device watchdog - emulating USB devices for the fuzzer device.
[`hypervisor`](hypervisor/) | Contains the implementation of the hypervisor environment.
[`literature_search`](literature_search/) | Contains the tool to search connected/related works by paper connections.
[`nix`](nix/) | Contains the definition of system of the fuzzer master and fuzzer instrumentor.
[`performance_timing`](performance_timing/) | Contains the tools to collect timing information from the fuzzer device,
[`performance_timing_macros`](performance_timing_macros/) | Contains utility macros to automate timing collection from target functions.
[`spec_fuzz`](spec_fuzz/) | Contains the implementation of the speculative micrcode fuzzer.
[`speculation_x86`](speculation_x86/) | Contains some test scenarios to check speculative execution behavior.
[`speculation_ucode`](speculation_ucode/) | Contains some test scenarios to check speculative execution behavior.
[`ucode_compiler_bridge`](ucode_compiler_bridge/) | Contains a bridge implementation to interface with the microcode compiler from [CustomProcessingUnit](https://github.com/pietroborrello/CustomProcessingUnit) and preprocessor macros for deriving multi file microcode updates.
[`ucode_compiler_derive`](ucode_compiler_derive/)| Contains utility macros to automate the generation of microcode updates.
[`ucode_compiler_dynamic`](ucode_compiler_dynamic/) | Contains runtime mircocode update compilation.
[`ucode_dump`](ucode_dump/) | Contains microcode dumps of the CPU.
[`uefi_udp4`](uefi_udp4/) | Contains a basic UEFI driver implementation of UDP.
[`x86_perf_counter`](x86_perf_counter/) | Contains the implementation to use x86 performance counters. 
[`xtask`](xtask/) | Contains build automation for this project.

## Getting started
Download [CustomProcessingUnit](https://github.com/pietroborrello/CustomProcessingUnit) and 1. place it into the parent directory of this folder or 2. set the env var `UASM` to the
`uasm.py` file from CustomProcessingUnit. The uFuzz project uses the `uasm.py` script to compile microcode updates.

Then apply the following git-patch to `uasm.py`: [uasm.py.patch](ucode_compiler_bridge/uasm.py.patch).

To deploy the fuzzer watchdog and fuzzer master, you will need `nix` installed.
Go into the [`nix`}(nix/) directory, change the public ssh keys, IPs etc., then, change IP settings within the [`fuzzer_master`](fuzzer_master/src/main.rs) project,
and run the following command (`|& nom` is optional):
```bash
nix build .#images.master |& nom
nix build .#images.node |& nom
```
This builds SD card images for the fuzzer master and fuzzer watchdog. After initial setup,
further changes may be deployed using (deploy-rs) by running in the `nix` directory (`|& nom` is optional):
```bash
nix run |& nom
```

To built and deploy the fuzzer device UEFI app, first, change the IP settings [`spec_fuzz`](spec_fuzz/src/controller_connection.rs)/[`fuzzer_device`](fuzzer_device/src/controller_connection.rs),
then; fuzzer watchdog must be running, then
compile and deploy the app using:
```bash
HOST_NODE="put IP of the watchdog here" cargo xtask put-remote --startup {XXX}
```
Depending on the target fuzzing scenario, use `spec_fuzz` (speculative microcode fuzzing) or `fuzzer_device` (x86 fuzzing)
instead of `{XXX}`.

Then boot the fuzzer master. Start the `fuzzer_master` app. Settings can be displayed by using `--help` in the CLI.

---

The fuzzing results will be stored to `/home/thesis/database.json` (fuzzer master) by default
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
| `spec_new_database` | Creates new speculative execution databases from templates                                     |
| `fuzz_combine`      | Combines multiple fuzzing databases into a single output database                              |
| `afl_convert`       | Converts AFL fuzzing findings to inputs usable with the manual execution fuzzing tool          |

## Documentation
Generate the documentation using the following command:
```bash
cargo xtask doc
```
The results will be placed inside the build directory `target/doc`

## Utilities
The `cargo xtask` command contains utilities to test the hypervisor environment on a simulated
CPU using the bochs emulator; allowing easier debugging.
```bash
cargo xtask emulate hypervisor bochs-intel
```