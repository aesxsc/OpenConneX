# OpenConneX

A native Windows and Linux app for controlling the EDIFIER MR3 over Bluetooth. Adjust the speaker's volume, EQ, and room compensation from your desktop.

[![Release](https://img.shields.io/github/v/release/aesxsc/OpenConneX?include_prereleases)](https://github.com/aesxsc/OpenConneX/releases) [![CI](https://github.com/aesxsc/OpenConneX/actions/workflows/ci.yml/badge.svg)](https://github.com/aesxsc/OpenConneX/actions/workflows/ci.yml) [![License](https://img.shields.io/badge/license-WTFPL-blue)](LICENSE)

**[Downloads](https://github.com/aesxsc/OpenConneX/releases)**

![OpenConneX overview connected to an EDIFIER MR3](docs/overview.png)

## Getting started

1. On Windows, extract the x64 ZIP from Releases and open `OpenConneX.exe`. On Linux, extract the x64 tarball and run `./openconnex`, or [build from source](#build-from-source). The v0.1.0 release contains the Windows build. Linux builds from `main`.
2. Turn on the MR3 and switch it to Bluetooth input. OpenConneX scans on startup. If needed, choose **Device settings → Scan nearby**.
3. To play audio from your PC, pair **EDIFIER MR3** in your system's Bluetooth settings and select it as your audio output. The system streams audio over Classic Bluetooth. OpenConneX uses a separate BLE connection for speaker controls.

## Features and compatibility

- Speaker volume, mute, playback and input controls
- Monitor, Music and Custom presets, a nine-band EQ, and a local preset library
- Low cutoff, acoustic-space compensation, and desktop compensation
- Device discovery, firmware version, and Bluetooth diagnostics

OpenConneX supports the **EDIFIER MR3** on Windows and Linux. Physical testing used MR3 firmware **1.0.7**. Linux uses BlueZ for Bluetooth and runs on Wayland or X11. Bluetooth must be enabled in your system settings. Firmware updates use the mobile ConneX app. See [verification and coverage](docs/verification.md) for the controls exercised on hardware.

## Build from source

Use Rust 1.92 or later. On Windows, install the MSVC Rust toolchain, Visual Studio Build Tools with **Desktop development with C++**, and a Windows SDK. On Linux, install a C toolchain, pkg-config, and the D-Bus, Wayland, X11, and xkbcommon development libraries. On Ubuntu or Debian, the library packages are `libdbus-1-dev`, `libwayland-dev`, `libx11-dev`, and `libxkbcommon-dev`.

```sh
cargo build --release --locked
```

Run `target/release/openconnex.exe` on Windows or `./target/release/openconnex` on Linux. See [Contributing](CONTRIBUTING.md) for the tests and hardware check.

## Contributing

Start with [GitHub Issues](https://github.com/aesxsc/OpenConneX/issues). Hardware reports, partial protocol findings, experimental device profiles, and improvements to the UI and documentation are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

OpenConneX is by [@aesxsc](https://github.com/aesxsc) and licensed under the [WTFPL](LICENSE). Bundled Inter fonts use the [SIL Open Font License](assets/Inter-OFL.txt). Dependency licenses are listed in [Third-party notices](THIRD_PARTY_NOTICES.txt).

OpenConneX is an independent, unofficial project with no affiliation with EDIFIER.
