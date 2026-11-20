use super::theme::*;
use crate::{engine::State, protocol};
use eframe::egui::{self, Align, Grid, Layout, RichText, ScrollArea, Ui};

static WINDOWS_VERSION: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

struct Section {
    title: &'static str,
    fields: Vec<(&'static str, String)>,
}

fn optional(value: Option<String>) -> String {
    value.unwrap_or_else(|| "Unavailable".into())
}

fn yes_no(value: bool) -> String {
    if value { "Yes" } else { "No" }.to_owned()
}

fn sections(state: &State) -> Vec<Section> {
    let speaker = &state.speaker;
    let mut system = vec![
        ("App", format!("OpenConneX {}", env!("CARGO_PKG_VERSION"))),
        ("Platform", format!("Windows · {}", std::env::consts::ARCH)),
        (
            "OS build",
            optional(
                std::sync::OnceLock::get_or_init(
                    &WINDOWS_VERSION,
                    crate::windows_ble::windows_version,
                )
                .clone(),
            ),
        ),
        ("Bluetooth backend", "Windows Runtime GATT".into()),
        ("Rendering", "Native egui · OpenGL".into()),
    ];
    if let Some(adapter) = &state.adapter {
        system.extend([
            ("Radio", adapter.name.clone()),
            ("Radio state", adapter.radio.clone()),
            ("Adapter address", adapter.address.clone()),
            ("Bluetooth LE", yes_no(adapter.supports_le)),
            ("Bluetooth Classic", yes_no(adapter.supports_classic)),
        ]);
    }
    let mut connection = vec![(
        "Control link",
        if state.link.is_some() {
            "Connected"
        } else {
            "Disconnected"
        }
        .into(),
    )];
    if let Some(link) = &state.link {
        connection.extend([
            ("BLE device name", link.name.clone()),
            (
                "BLE address",
                protocol::address_display(link.endpoint.address),
            ),
            (
                "Address type",
                if link.endpoint.random {
                    "Random"
                } else {
                    "Public"
                }
                .into(),
            ),
            (
                "Signal at discovery",
                optional(link.rssi.map(|rssi| format!("{rssi} dBm"))),
            ),
            ("ATT MTU", format!("{} bytes", link.att_mtu)),
            (
                "Last command round trip",
                optional(state.latency_ms.map(|ms| format!("{ms} ms"))),
            ),
            (
                "Advertisement company ID",
                optional(link.company.map(|id| format!("0x{id:04X} ({id})"))),
            ),
            (
                "Advertisement manufacturer data",
                protocol::hex(&link.advertisement, " "),
            ),
            ("Discovery UUID", protocol::DISCOVERY_UUID.into()),
            ("GATT service", protocol::SERVICE_UUID.into()),
            ("Notify characteristic", protocol::RX_UUID.into()),
            ("Write characteristic", protocol::TX_UUID.into()),
            (
                "Command application code",
                format!("0x{:02X}", protocol::APP_CODE),
            ),
            ("Invalid frames", state.invalid_frames.to_string()),
        ]);
    }
    let mut device = vec![
        ("Product", "EDIFIER MR3 · EDF100081".into()),
        ("Bluetooth name", optional(speaker.name.clone())),
        ("Classic Bluetooth address", optional(speaker.mac.clone())),
        ("Firmware", optional(speaker.firmware.clone())),
        ("Bluetooth audio link", speaker.a2dp_title().into()),
        ("Input", speaker.input_title().into()),
        (
            "Output volume",
            optional(speaker.volume.map(|(max, level)| {
                format!(
                    "{level}/{max} steps · {:.0}%",
                    f32::from(level) / f32::from(max) * 100.0
                )
            })),
        ),
        (
            "Wired headphone jack",
            optional(
                speaker
                    .wired
                    .map(|value| if value { "Plugged in" } else { "Unplugged" }.to_owned()),
            ),
        ),
        (
            "Codec query",
            optional(speaker.codec.map(|id| format!("Device code 0x{id:02X}"))),
        ),
        (
            "Playback",
            optional(
                speaker
                    .playback
                    .map(|playing| if playing { "Playing" } else { "Paused" }.into()),
            ),
        ),
    ];
    if let Some(enabled) = speaker.prompt_tones {
        device.push(("Prompt tones", if enabled { "On" } else { "Off" }.into()));
    }
    if let Some((group, index)) = speaker.input {
        device.push((
            "Input protocol values",
            format!("Group {group} · input {index}"),
        ));
    }
    if let Some(sound) = speaker.sound {
        device.extend([
            (
                "Sound preset",
                format!(
                    "{} · mode {} · layout {}",
                    speaker.mode_title(),
                    sound.mode,
                    sound.layout
                ),
            ),
            (
                "Low cutoff",
                format!(
                    "{} Hz · {} dB/oct",
                    sound.cutoff,
                    (u16::from(sound.slope) + 1) * 6
                ),
            ),
            (
                "Acoustic space",
                format!("{} dB", -(i16::from(sound.space))),
            ),
            (
                "Desktop compensation",
                if sound.desktop { "On" } else { "Off" }.into(),
            ),
        ]);
    }
    if let Some(eq) = &speaker.equalizer {
        device.extend([
            ("Custom EQ", eq.name.clone()),
            ("Custom EQ ID", protocol::hex(&eq.id, " ")),
            (
                "EQ gains",
                eq.gains
                    .iter()
                    .map(|value| format!("{:+.1}", (f32::from(*value) - 6.0) / 2.0))
                    .collect::<Vec<_>>()
                    .join(" / ")
                    + " dB",
            ),
        ]);
    }
    let mut capabilities = vec![];
    if !speaker.capability.is_empty() {
        capabilities.extend([
            ("Rename", yes_no(speaker.cap(3) != 0)),
            ("Disconnect audio source", yes_no(speaker.cap(6) == 1)),
            ("Power off", yes_no(speaker.cap(10) == 1)),
            ("Factory reset", yes_no(speaker.cap(17) & 32 != 0)),
            (
                "Android BLE firmware updates",
                yes_no(speaker.cap(12) & 2 != 0),
            ),
            ("EQ family", speaker.cap(13).to_string()),
            (
                "Raw capability bytes",
                protocol::hex(&speaker.capability, " "),
            ),
        ]);
    }
    vec![
        Section {
            title: "Computer and adapter",
            fields: system,
        },
        Section {
            title: "Bluetooth control link",
            fields: connection,
        },
        Section {
            title: "Speaker state",
            fields: device,
        },
        Section {
            title: "Firmware capabilities",
            fields: capabilities,
        },
        Section {
            title: "MR3 specifications",
            fields: vec![
                ("Output power", "18 W + 18 W RMS".into()),
                ("Frequency response", "52 Hz – 40 kHz".into()),
                ("Signal-to-noise ratio", "≥ 85 dB(A)".into()),
                (
                    "Drivers",
                    "3.5-inch mid-low driver and 1-inch silk dome tweeter per cabinet".into(),
                ),
                (
                    "Bluetooth specification",
                    "5.4 · SBC · multipoint for two audio sources".into(),
                ),
                (
                    "Physical inputs",
                    "¼-inch balanced TRS · RCA · 3.5 mm AUX · Bluetooth".into(),
                ),
                (
                    "Outputs",
                    "3.5 mm headphone jack · passive speaker connection".into(),
                ),
                (
                    "Dimensions per cabinet",
                    "125.5 × 220 × 185 mm (active) · 125.5 × 220 × 176 mm (passive)".into(),
                ),
                ("Total weight", "3.85 kg".into()),
            ],
        },
    ]
}

pub fn show(ui: &mut Ui, state: &State, editable: bool) -> bool {
    let sections = sections(state);
    let mut refresh = false;
    card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(title("Device and system report", 19.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if button(ui, "Copy report", false).clicked() {
                    use std::fmt::Write;
                    let mut report = String::new();
                    for section in &sections {
                        let _ = writeln!(report, "\n{}", section.title);
                        for (key, value) in &section.fields {
                            let _ = writeln!(report, "{key}\t{value}");
                        }
                    }
                    report.push_str("\nProtocol trace\n");
                    for line in &state.trace {
                        let _ = writeln!(report, "{line}");
                    }
                    ui.ctx().copy_text(report);
                }
                refresh = ui
                    .add_enabled(editable, egui::Button::new("Refresh"))
                    .clicked();
            });
        });
        ui.label(secondary(
            "The report includes Bluetooth addresses and recent protocol traffic.",
        ));
    });
    for section in sections {
        if section.fields.is_empty() {
            continue;
        }
        card(ui, |ui| {
            ui.label(title(section.title, 18.0));
            Grid::new(section.title)
                .num_columns(2)
                .spacing(egui::vec2(22.0, 10.0))
                .striped(true)
                .max_col_width((ui.available_width() - 234.0).max(180.0))
                .show(ui, |ui| {
                    for (key, value) in section.fields {
                        ui.label(secondary(key));
                        ui.label(RichText::new(value).color(TEXT).size(13.0));
                        ui.end_row();
                    }
                });
        });
    }
    card(ui, |ui| {
        ui.collapsing("GATT protocol trace", |ui| {
            if state.trace.is_empty() {
                ui.label(secondary("Connect the speaker to capture traffic."));
            }
            ScrollArea::both()
                .max_height(256.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for event in &state.trace {
                        ui.label(RichText::new(event).color(MUTED).monospace().size(11.0));
                    }
                });
        });
    });
    refresh
}
