# Contributing

Start with a [GitHub issue](https://github.com/aesxsc/OpenConneX/issues) for a bug, feature, or device. Partial findings and experiments are welcome.

## Build and test

Follow the [build instructions](README.md#build-from-source) and run both checks.

```powershell
cargo fmt --check
cargo test --locked
```

For a hardware check, close any other speaker controller and run the following with an MR3 in Bluetooth mode. The optional `--verify-writes` flag exercises settings and restores their original values.

```powershell
Start-Process ./target/release/openconnex.exe -ArgumentList @('--hardware-check', 'mr3-readback.txt') -Wait -PassThru
```

On Linux, run `./target/release/openconnex --hardware-check mr3-readback.txt`.

## Reports and pull requests

For bugs, include the OpenConneX version, Windows build or Linux distribution and BlueZ version, speaker model and firmware, steps to reproduce, and what happened. Diagnostics and hardware-check reports can contain device identifiers and raw Bluetooth traffic. Review them before posting.

For device or protocol work, share the exact model and firmware, the BLE service and characteristics, and the requests and responses you observed. Say which actions you tried on hardware. A read-only trace or an incomplete device profile can still help. Keep APKs and extracted vendor resources out of the repository.

Start with `src/protocol.rs` for packet framing, `src/device.rs` for the device model, and `src/windows_ble.rs` or `src/linux_ble.rs` for the Bluetooth transport. Without an MR3, you can help with protocol tests, the desktop UI, documentation, or Android transport research.

Keep pull requests focused. Link the relevant issue and list the checks you ran. Update the third-party notices when dependencies change.
