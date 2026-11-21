# Verification and coverage

Reference app version **EDIFIER ConneX 1.0.32**. Physical test device **EDIFIER MR3**, firmware **1.0.7**.

| Feature | Coverage |
| --- | --- |
| BLE discovery, identification, reconnect and notifications | App connected to the physical MR3 through WinRT on Windows and BlueZ on Arch Linux |
| Volume and nine-band EQ | Real writes and readbacks. The original volume and full EQ preset were restored |
| Prompt tones | The setting was toggled, read back and restored |
| Acoustic tuning | Real write and readback. The original acoustic-space value was restored |
| Device information | Firmware, Classic Bluetooth address, A2DP state, codec response, wired headphones, and capabilities read from the speaker |
| Diagnostics | Adapter details, negotiated ATT MTU, last request latency, UUIDs, capability bytes, per-command frames and copyable report |
| Playback, input switching, rename, power off and factory reset | Implemented from the APK protocol. The hardware smoke test avoids interrupting playback, renaming the speaker, or erasing its configuration |
| Firmware upgrade | Installed version and Edifier support links. Use mobile ConneX to flash firmware |
| Other Edifier speakers | Nearby Edifier advertisements can appear in discovery. Only the MR3 profile enables controls |

The app identifies the MR3 by its advertised service and confirms the EQ family and room-tuning layout before enabling writes. The BLE address and Classic audio address may differ. Signal strength is the most recent advertising sample, and the round-trip figure measures the last command and notification. Static specifications come from Edifier. [MR3 specifications](https://www.edifier.com/int/us/p/studio-monitors/mr3)

Test cases use captured MR3 packet layouts to exercise fragmented and concatenated notifications, checksum rejection, readback parsing and EQ encoding.

The window was rendered with a live MR3 connection at desktop size. A compact-window render also covered the disconnected state.
