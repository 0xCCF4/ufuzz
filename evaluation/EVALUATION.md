Artifact uFuzz:
# Overview
* Getting Started
* Setup experimentation

If any problems occur, questions arise feel free to contact the first and second
author (UTC+1) via mail, which will reply timely.

# Getting started (10 minutes human-time + 5 minutes download-time)
* Install rust
  - SIDE-EFFECT: The rust compiler will be installed to $HOME/.cargo
  - Install rustup <https://rustup.rs/>
  - Install required rust toolchains and targets
* You should have a reasonable new python version on your maschine (we are using 3.11.2, but any newish one should satisfy)
  - Install the `click` python library (e.g., `pip3 install python3-click`)
* Clone our project and dependencies
```bash
# Install python (ubuntu/debian)
sudo apt install python3 python3-click

# Install rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- --default-toolchain none -y
rustup install nightly-2024-09-06
rustup target add x86_64-unknown-uefi
rustup target add aarch64-unknown-linux-gnu
rustup default nightly-2024-09-06

# Clone project and setup dependencies
git clone <url> ufuzz
git clone https://github.com/pietroborrello/CustomProcessingUnit
cd CustomProcessingUnit
git reset --hard 4237524
git apply ../ufuzz/ucode_compiler_bridge/uasm.py.patch
cd ..
```

# Enter lab-environment (2 minutes human-time)
* Setup port forwarding to deploy applications to the target device
  - SIDE-EFFECT: Programs can connect to local ports 4444, 4445; will be automatically
    redirected to our lab environment. Port-forward can be stopped by CTRL+C/terminating
    the ssh process. Your public SSH keys are copied to our lab machines
```bash
ssh -L 127.0.0.1:4444:<IPa>:22 -L 127.0.0.1:4445:<IPb>:22 -v -NT -p 5555 <IPc>

# In a new terminal
ssh-copy-id -p 4444 user@127.0.0.1
ssh-copy-id -p 4445 user@127.0.0.1
```

# Prepare execution of proof-of-concepts

# Show that microcode updating works

# Show microcode speculation exists

# Show that microcode speculation is not rolled back (not-rolled back)

# Show that microcode speculation is not rolled back (DoS)

# Prepare execution of fuzzer

# False-positive `popf` interrupt prioritization

# False-positive `popf` state corruption

# Finding FPU initialization

# Prepare fuzzer execution (10 minutes)


# Run long-running coverage fuzzing (2 minutes human-time + N hours compute-time + 10 minutes human-time)
* To reproduce the coverage graphs from the paper run the fuzzer executables
* You may adjust how long the experiment should run in command line
* We advise you to start the fuzzer executable in a `tmux` session, so that you can close the `ssh` connection, while it is running

Steps:
1. Build the fuzzing executable running on the fuzzer controller (the main fuzzing loop is running on a Rpi)
2. Build the fuzzing agent running on the target CPU, which actually executed fuzzing inputs
3. Push both executables to the lab setup
4. Reboot the fuzzing agent to use the new executable
5. Start the main fuzzing loop on the fuzzing controller
6. Export the fuzzing outputs to a CSV file
7. Convert the CSV file to a graph

```bash
# -----------------------------------
# Current directory: ufuzz/
# -----------------------------------

# Build the fuzzer executable (on your machine)
cargo build --locked -p fuzzer_master --target aarch64-unknown-linux-gnu --lib
cargo build --locked -p fuzzer_master --target aarch64-unknown-linux-gnu --bin fuzz_master -j2

# Push the fuzzer master to the fuzzer controller ($HOME/fuzz_master) (on your machine)
echo put target/aarch64-unknown-linux-gnu/debug/fuzz_master | sftp -P 4445 thesis@127.0.0.1

# Build and push the fuzzer agent executable to the target CPU (on your machine)
HOST_NODE="127.0.0.1" SSH_ARGS="-p 4444" SFTP_ARGS="-P 4444" cargo xtask put-remote --startup fuzzer_device

# Enter the fuzzer controller (on your maschine)
ssh -p 4445 user@127.0.0.1

# Reboot the fuzzing agent (on the fuzzer controller)
  # view the screencast of the fuzzer agent, if there is font on the screen, it is likely on, so reboot it using
  curl -X POST http://10.83.3.198:8000/power_button_short # this presses the power button shortly
  # if the screen does not turn black, try
  curl -X POST http://10.83.3.198:8000/power_button_long # this presses the power button long
  # the device is now off, press the power button to turn back on
  curl -X POST http://10.83.3.198:8000/power_button_short
  # wait until the BIOS screen is shown
  curl -X POST http://10.83.3.198:8000/skip_bios # boots from the attached remote storage
  # in 3-4 minutes the device will be booted up

# Verify that the device is responsive (on the fuzzer controller)
~/fuzz_master cap
# todo expected output

# Start the experiment (on the fuzzer controller)
tmux
export DATABASE_NAME="db.json" # you may change the name of the database, if you run multiple runs
~/fuzz_master --database "${DATABASE_NAME}" afl --help # Select any experiment setup from the parameters
~/fuzz_master --database "${DATABASE_NAME}" afl --timeout-hours 10 # Put them here, or just use defaults
# you may now detach from the SSH session with CTRL+Y then d, you can then `exit`
# to reattach SSH into the fuzzer controller, then use `tmux attach-session`
exit
exit

# -----------------------------------
# Current directory: ufuzz/evaluation
# -----------------------------------

# Download the database (on your machine)
echo get db.json | sftp -P 4445 user@127.0.0.1 # you may change the database name accordingly

# Convert the fuzzer database to a CSV files; creates the files `plot-db.json.*.csv`
cargo run --locked -p fuzzer_master --bin fuzz_viewer -- db.json plot-db.x

# Convert the CSV file to graph
python3 plot.py --mode long --output graph-progress.pdf --x time plot-db.json.cov.csv
python3 plot.py --mode histogram --output graph-histogram.pdf --x time plot-db.json.cov.csv
```