# uFuzz
A x86 CPU fuzzer utilizing microcode coverage

## Overview
uFuzz is a CPU fuzzer that leverage custom microcode updates for x86 Intel
CPUs to extract microcode coverage information at runtime. For more details
see the [paper](xxx).

## Structure
uFuzz consists of three different systems:
1. The fuzzer device: This is the target device that runs the fuzzer.
2. A fuzzer instrumentor: This is a device that emulates USB devices and controls the power supply of the fuzzer device. (Raspberry Pi 4)
3. The fuzzer master: The main fuzzing loop runs here, tasks are scheduled on the fuzzer device for execution. (Raspberry Pi 4)

## Project structure
The uFuzz project is structured as follows:

Component       | Description
--------------- | -----------
[`corpus-gen`](corpus-gen/) | Generates the corpus for initial fuzzing inputs. See the evaluation section of the paper. 
[`coverage`](coverage/) | Collects microcode coverage from the CPU
[`custom_processing_unit`](custom_processing_unit/) | Contains utility function derived from [CustomProcessingUnit](https://github.com/pietroborrello/CustomProcessingUnit).
[`data_types`](data_types/) | Contains shared data types for writing custom microcode updates.
[`fuzzer_data`](fuzzer_data/) | Contains shared data between the fuzzer instance and fuzzer master controller.
[`fuzzer_device`](fuzzer_device/) | Contains the implementation of uFuzz, which runs on the target device/CPU.
[`fuzzer_master`](fuzzer_master/) | Contains the implementation of the fuzzer master that controls a fuzzer device.
[`fuzzer_node`](fuzzer_node/) | Contains the implementation of the fuzzer device instrumentor - emulating USB devices for the fuzzer device.
[`hypervisor`](hypervisor/) | Contains the implementation of the hypervisor environment.
[`literature_search`](literature_search/) | Contains the tool to search connected/related works by paper connections.
[`nix`](nix/) | Contains the definition of system of the fuzzer master and fuzzer instrumentor.
[`performance_timing`](performance_timing/) | Contains the tools to collect timing information from the fuzzer device,
[`performance_timing_macros`](performance_timing_macros/) | Contains utility macros to automate timing collection from target functions.
[`speculation`](speculation/) | Contains some test scenarios to check speculative execution behavior.
[`ucode_compiler_bridge`](ucode_compiler_bridge/) | Contains a bridge implementation to interface with the microcode compiler from [CustomProcessingUnit](https://github.com/pietroborrello/CustomProcessingUnit).
[`ucode_compiler_derive`](ucode_compiler_derive/)| Contains utility macros to automate the generation of microcode updates.
[`ucode_compiler_dynamic`](ucode_compiler_dynamic/) | Contains runtime mircocode update compilation.
[`ucode_dump`](ucode_dump/) | Contains microcode dumps of the CPU.
[`uefi_udp4`](uefi_udp4/) | Contains a basic UEFI driver implementation of UDP.
[`xtask`](xtask/) | Contains build automation for this project.

## Getting started
To deploy the fuzzer instrumentor and fuzzer master, you will need `nix` installed.
Go into the `nix` directory, change the public ssh keys, and run the following command:
```bash
nix build .#images.master
nix build .#images.node
```
This builds SD card images for the fuzzer master and fuzzer instrumentor. After initial setup,
further changes may be deployed by running:
```bash
nix run
```

To built and deploy the fuzzer device UEFI app, the fuzzer instrumentor must be running, then
compile and deploy the app using:
```bash
cargo xtask put-remote --startup fuzzer_device
```

Then boot the fuzzer master.

The fuzzing results will be stored to `/home/thesis/database.json` (fuzzer master) by default
and can be viewed by running (fuzzer master):
```bash
fuzz_viewer /home/thesis/database.json
```

Disclaimer: The IP addresses of the devices must be changed within the codebase of uFuzz before builting the.
images/app using the above commands.