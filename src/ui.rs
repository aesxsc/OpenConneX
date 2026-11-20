mod diagnostics;
mod theme;

use crate::storage::Library;
use crate::{
    device::{BANDS, Equalizer},
    engine::{self, Action, Control, State, Worker},
    protocol,
};
use eframe::egui::{
    self, Align, Color32, Frame, Layout, Margin, RichText, ScrollArea, Sense, Stroke,
    TextureHandle, Ui,
};
use theme::*;

const MANUAL: &str =
    "https://new-edifier-us-oss.edifier.com/files/20240902/93517b14f597b1ec5b5814993aa9fca7.pdf";
const SUPPORT: &str = "https://www.edifier.com/int/us/s/studio-monitors/mr3";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Overview,
    Equalizer,
    Room,
    Devices,
    Diagnostics,
    About,
}

impl Page {
    const ALL: [(Self, &'static str); 6] = [
        (Self::Overview, "Overview"),
        (Self::Equalizer, "Equalizer"),
        (Self::Room, "Room tuning"),
        (Self::Devices, "Device settings"),
        (Self::Diagnostics, "Diagnostics"),
        (Self::About, "About"),
    ];

    fn title(self) -> &'static str {
        Self::ALL.iter().find(|(page, _)| *page == self).unwrap().1
    }
}

enum Dialog {
    SavePreset,
    RenamePreset,
    ImportPreset,
    SpeakerName,
    Confirm(&'static str, &'static str, Control),
}

struct App {
    worker: Worker,
    state: State,
    library: Library,
    page: Page,
    artwork: TextureHandle,
    icon: TextureHandle,
    dialog: Option<Dialog>,
    editor: String,
    notice: Option<String>,
    submitted: bool,
    volume_draft: Option<u8>,
    cutoff_draft: Option<u8>,
    band_draft: [Option<u8>; 9],
    muted_level: Option<u8>,
}

fn raster(bytes: &[u8]) -> ([usize; 2], Vec<u8>) {
    let bitmap = image::load_from_memory(bytes)
        .expect("embedded image should decode")
        .into_rgba8();
    (
        [bitmap.width() as usize, bitmap.height() as usize],
        bitmap.into_raw(),
    )
}

pub fn run() -> eframe::Result {
    let ([width, height], rgba) = raster(include_bytes!("../assets/icon.png"));
    let icon = egui::IconData {
        width: width as u32,
        height: height as u32,
        rgba,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("OpenConneX")
            .with_inner_size([1170.0, 845.0])
            .with_min_inner_size([670.0, 545.0])
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native(
        "OpenConneX",
        options,
        Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))),
    )
}

impl App {
    fn new(ctx: &egui::Context) -> Self {
        setup(ctx);
        let (library, notice) = match Library::load() {
            Ok(library) => (library, None),
            Err(error) => (Library::default(), Some(error)),
        };
        let paint = ctx.clone();
        let worker = engine::spawn(move || paint.request_repaint());
        let texture = |name, bytes| {
            let (size, rgba) = raster(bytes);
            ctx.load_texture(
                name,
                egui::ColorImage::from_rgba_unmultiplied(size, &rgba),
                egui::TextureOptions::LINEAR,
            )
        };
        let mut app = Self {
            worker,
            state: State::default(),
            library,
            page: Page::Overview,
            artwork: texture("Studio monitors", include_bytes!("../assets/monitors.webp")),
            icon: texture("OpenConneX", include_bytes!("../assets/icon.png")),
            dialog: None,
            editor: String::new(),
            notice,
            submitted: false,
            volume_draft: None,
            cutoff_draft: None,
            band_draft: [None; 9],
            muted_level: None,
        };
        app.send(Action::Discover {
            auto: true,
            saved: app.library.last_speaker,
        });
        app
    }

    fn receive(&mut self) {
        while let Ok(state) = self.worker.updates.try_recv() {
            let old_link = self.state.link.as_ref().map(|link| link.endpoint);
            let new_link = state.link.as_ref().map(|link| link.endpoint);
            self.submitted = false;
            if old_link != new_link {
                self.muted_level = None;
                self.volume_draft = None;
                self.cutoff_draft = None;
                self.band_draft = [None; 9];
            }
            if let Some(endpoint) = new_link
                && self.library.last_speaker != Some(endpoint)
            {
                self.library.last_speaker = Some(endpoint);
                self.save();
            }
            self.state = state;
        }
    }

    fn save(&mut self) {
        if let Err(error) = self.library.save() {
            self.notice = Some(error);
        }
    }

    fn send(&mut self, action: Action) {
        if self.submitted {
            return;
        }
        match self.worker.commands.try_send(action) {
            Ok(()) => self.submitted = true,
            Err(error) => {
                self.notice = Some(format!("Could not reach the Bluetooth worker. {error}"))
            }
        }
    }

    fn control(&mut self, control: Control) {
        self.send(Action::Set(control));
    }

    fn can_edit(&self) -> bool {
        self.state.link.is_some() && self.state.busy.is_none() && !self.submitted
    }

    fn connect_label(&self) -> (&str, Color32) {
        if let Some(busy) = self.state.busy {
            return (busy, ACCENT);
        }
        if self.state.link.is_some() {
            ("BLE connected", GREEN)
        } else if self.submitted {
            ("Working", ACCENT)
        } else {
            ("Disconnected", MUTED)
        }
    }

    fn navigation(&mut self, ui: &mut Ui, narrow: bool) {
        ui.horizontal(|ui| {
            ui.image((self.icon.id(), egui::vec2(32.0, 32.0)));
            ui.label(title("OpenConneX", 18.0));
        });
        if narrow {
            ui.horizontal_wrapped(|ui| {
                for (page, label) in Page::ALL {
                    if ui.selectable_label(page == self.page, label).clicked() {
                        self.page = page;
                    }
                }
            });
        } else {
            ui.add_space(31.0);
            let spacing = ui.spacing_mut();
            spacing.item_spacing.y = 5.0;
            for (i, (page, label)) in Page::ALL.into_iter().enumerate() {
                if nav(ui, label, i, page == self.page) {
                    self.page = page;
                }
            }
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                ui.label(secondary(format!("OpenConneX {}", env!("CARGO_PKG_VERSION"))).size(11.0));
                ui.separator();
                ui.label(title("Studio monitors", 13.0));
                status(ui, self.connect_label().0, self.connect_label().1);
            });
        }
    }

    fn header(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading(title(self.page.title(), 29.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (label, color) = self.connect_label();
                status(ui, label, color);
                if self.state.busy.is_some() {
                    ui.spinner();
                }
            });
        });
        if let Some(message) = self.state.message.as_ref().or(self.notice.as_ref()) {
            let message = message.clone();
            Frame::new()
                .fill(TILE)
                .corner_radius(8)
                .inner_margin(10)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(message).size(13.5).color(AMBER));
                        if self.notice.is_some() && ui.small_button("Dismiss").clicked() {
                            self.notice = None;
                        }
                    });
                });
        }
        ui.add_space(5.0);
    }

    fn hero(&mut self, ui: &mut Ui) {
        let name = self
            .state
            .speaker
            .name
            .as_deref()
            .filter(|name| !name.is_empty())
            .unwrap_or("EDIFIER MR3");
        let name = name.to_owned();
        let width = ui.available_width();
        let height = if width < 600.0 { 200.0 } else { 230.0 };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
        let painter = ui.painter();
        let art_width = (width * 0.59).clamp(240.0, height * 1.87);
        let art_rect = egui::Rect::from_min_max(
            rect.right_top() - egui::vec2(art_width, 0.0),
            rect.right_bottom(),
        );
        painter.rect_filled(rect, 16, PANEL);
        let aspect = art_width / height;
        let x_crop = (1.0 - aspect / 1.5).max(0.0) * 0.5;
        let y_crop = (1.0 - 1.5 / aspect).max(0.0) * 0.5;
        let uv = egui::Rect::from_min_max(
            egui::pos2(x_crop, y_crop),
            egui::pos2(1.0 - x_crop, 1.0 - y_crop),
        );
        let corners = egui::CornerRadius {
            ne: 15,
            se: 15,
            nw: 0,
            sw: 0,
        };
        painter.add(
            egui::epaint::RectShape::filled(art_rect.shrink(1.0), corners, Color32::WHITE)
                .with_texture(self.artwork.id(), uv),
        );
        let mut fade = egui::Mesh::default();
        let left = art_rect.left();
        let right = left + 73.0;
        for (pos, color) in [
            (egui::pos2(left, rect.top()), PANEL),
            (egui::pos2(right, rect.top()), Color32::TRANSPARENT),
            (egui::pos2(right, rect.bottom()), Color32::TRANSPARENT),
            (egui::pos2(left, rect.bottom()), PANEL),
        ] {
            fade.colored_vertex(pos, color);
        }
        fade.add_triangle(0, 1, 2);
        fade.add_triangle(0, 2, 3);
        painter.add(egui::Shape::mesh(fade));
        painter.rect_stroke(rect, 16, Stroke::new(1.0, EDGE), egui::StrokeKind::Inside);
        let text_rect = egui::Rect::from_min_size(
            rect.min + egui::vec2(24.0, 28.0),
            egui::vec2(width - art_width + 28.0, height - 45.0),
        );
        {
            let ui = &mut ui.new_child(egui::UiBuilder::new().max_rect(text_rect));
            ui.label(accent("STUDIO MONITORS"));
            ui.add_space(3.0);
            ui.label(title(name, if width < 610.0 { 28.0 } else { 40.0 }));
            let description = self.state.speaker.firmware.as_ref().map_or_else(
                || "2.0 nearfield speaker system".to_owned(),
                |version| {
                    format!(
                        "{}  ·  Firmware {version}",
                        self.state.speaker.input_title()
                    )
                },
            );
            ui.label(secondary(description));
            ui.add_space(5.0);
            let available = self.state.busy.is_none() && !self.submitted;
            ui.add_enabled_ui(available, |ui| {
                if self.state.link.is_some() {
                    if button(ui, "Manage device", false).clicked() {
                        self.page = Page::Devices;
                    }
                } else if button(ui, title("Find speaker", 13.5).color(INK), true).clicked() {
                    self.page = Page::Devices;
                    self.send(Action::Discover {
                        auto: true,
                        saved: self.library.last_speaker,
                    });
                }
            });
        }
    }

    fn overview(&mut self, ui: &mut Ui) {
        self.hero(ui);
        ui.add_space(5.0);
        ui.columns(2, |columns| {
            card(&mut columns[0], |ui| self.volume(ui));
            card(&mut columns[1], |ui| self.playback(ui));
        });
        ui.add_space(6.0);
        card(ui, |ui| {
            ui.label(title("Sound presets", 19.0));
            ui.columns(3, |columns| {
                let sound = self.state.speaker.sound;
                for (index, (label, description)) in [
                    ("Monitor", "Dedicated monitoring"),
                    ("Music", "Music listening"),
                    ("Custom", "Your nine-band EQ"),
                ]
                .into_iter()
                .enumerate()
                {
                    let ui = &mut columns[index];
                    let selected = sound.is_some_and(|sound| sound.mode == index as u8);
                    let fill = if selected { ACCENT_FILL } else { TILE };
                    let button = egui::Button::new(title(label, 15.0).color(if selected {
                        ACCENT
                    } else {
                        TEXT
                    }))
                    .fill(fill)
                    .stroke(Stroke::new(1.0, if selected { ACCENT } else { EDGE }))
                    .corner_radius(10);
                    let width = ui.available_width();
                    let response = ui.add_enabled(
                        self.can_edit() && sound.is_some(),
                        button.min_size(egui::vec2(width, 43.0)),
                    );
                    if response.clicked()
                        && let Some(mut sound) = sound
                    {
                        sound.mode = index as u8;
                        self.control(Control::Sound(sound, false));
                    }
                    ui.label(secondary(description).size(12.0));
                }
            });
        });
    }

    fn volume(&mut self, ui: &mut Ui) {
        let volume = self.state.speaker.volume;
        let (max, current) = volume.unwrap_or((30, 0));
        let mut value = self.volume_draft.unwrap_or(current);
        ui.horizontal(|ui| {
            ui.label(title("Master volume", 18.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    title(
                        if volume.is_some() {
                            format!("{value}/{max}")
                        } else {
                            "—".into()
                        },
                        17.0,
                    )
                    .color(ACCENT),
                );
            });
        });
        let available = self.can_edit() && volume.is_some();
        ui.add_enabled_ui(available, |ui| {
            let width = ui.available_width();
            ui.spacing_mut().slider_width = width.max(90.0);
            let slider = ui
                .add(egui::Slider::new(&mut value, 0..=max).show_value(false))
                .on_hover_text("Speaker volume in hardware steps");
            if slider.changed() {
                self.volume_draft = Some(value);
            }
            if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                self.volume_draft = None;
                self.control(Control::Volume(value));
            }
            ui.horizontal(|ui| {
                if button(ui, "−", false)
                    .on_hover_text("Volume down")
                    .clicked()
                {
                    self.control(Control::Volume(current.saturating_sub(1)));
                }
                if button(ui, "+", false).on_hover_text("Volume up").clicked() {
                    self.control(Control::Volume(current.saturating_add(1).min(max)));
                }
                if button(
                    ui,
                    if current == 0 && self.muted_level.is_some() {
                        "Unmute"
                    } else {
                        "Mute"
                    },
                    false,
                )
                .clicked()
                {
                    if current > 0 {
                        self.muted_level = Some(current);
                        self.control(Control::Volume(0));
                    } else if let Some(value) = self.muted_level.take() {
                        self.control(Control::Volume(value));
                    }
                }
            });
        });
    }

    fn playback(&mut self, ui: &mut Ui) {
        ui.set_min_height(132.0);
        ui.label(title("Playback", 18.0));
        ui.add_space(3.0);
        let speaker = &self.state.speaker;
        let label = if !speaker.track.is_empty() {
            if speaker.artist.is_empty() {
                speaker.track.clone()
            } else {
                format!("{} · {}", speaker.track, speaker.artist)
            }
        } else if speaker.playback_available() {
            "Bluetooth audio source".into()
        } else {
            "Available with Bluetooth audio".into()
        };
        ui.add(egui::Label::new(secondary(label)).truncate());
        let available = self.can_edit() && self.state.speaker.playback_available();
        ui.add_enabled_ui(available, |ui| {
            ui.add_space(7.0);
            ui.horizontal_wrapped(|ui| {
                for (text, code) in [("Previous", 5), ("Play", 0), ("Pause", 1), ("Next", 4)] {
                    if button(ui, text, false).clicked() {
                        self.control(Control::Playback(code));
                    }
                }
            });
        });
    }

    fn equalizer(&mut self, ui: &mut Ui) {
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(title("Nine-band equalizer", 20.0));
                    ui.label(secondary(
                        self.state
                            .speaker
                            .equalizer
                            .as_ref()
                            .map(|eq| eq.name.as_str())
                            .unwrap_or("Connect the speaker to read its EQ"),
                    ));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            self.can_edit()
                                && self.state.speaker.sound.is_some_and(|s| s.mode != 2),
                            egui::Button::new(title("Use Custom", 13.0).color(INK)).fill(ACCENT),
                        )
                        .clicked()
                        && let Some(mut sound) = self.state.speaker.sound
                    {
                        sound.mode = 2;
                        self.control(Control::Sound(sound, false));
                    }
                });
            });
            ui.add_space(4.0);
            let eq = self.state.speaker.equalizer.clone();
            ui.add_enabled_ui(self.can_edit() && eq.is_some(), |ui| {
                ui.columns(9, |columns| {
                    for (index, column) in columns.iter_mut().enumerate() {
                        let push = column.spacing_mut();
                        push.slider_width = 160.0;
                        column.vertical_centered(|ui| {
                            let mut gain = self.band_draft[index]
                                .or_else(|| eq.as_ref().map(|eq| eq.gains[index]))
                                .unwrap_or(6);
                            ui.label(
                                title(format!("{:+.1}", (f32::from(gain) - 6.0) / 2.0), 13.0)
                                    .color(ACCENT),
                            );
                            let response = ui
                                .allocate_ui_with_layout(
                                    egui::vec2(36.0, 160.0),
                                    Layout::top_down(Align::Min),
                                    |ui| {
                                        ui.add(
                                            egui::Slider::new(&mut gain, 0..=12)
                                                .vertical()
                                                .show_value(false),
                                        )
                                        .on_hover_text(
                                            format!("{} Hz gain, −3.0 to +3.0 dB", BANDS[index]),
                                        )
                                    },
                                )
                                .inner;
                            if response.changed() {
                                self.band_draft[index] = Some(gain);
                            }
                            if response.drag_stopped()
                                || (response.changed() && !response.dragged())
                            {
                                self.band_draft[index] = None;
                                self.control(Control::Band(index, gain));
                            }
                            let frequency = BANDS[index];
                            let label = if frequency >= 1000 {
                                format!("{}k", frequency / 1000)
                            } else {
                                frequency.to_string()
                            };
                            ui.label(secondary(label));
                        });
                    }
                });
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    if button(ui, "Save preset", false).clicked() {
                        self.editor = String::new();
                        self.dialog = Some(Dialog::SavePreset);
                    }
                    if button(ui, "Rename active preset", false).clicked() {
                        self.editor = eq.as_ref().map(|eq| eq.name.clone()).unwrap_or_default();
                        self.dialog = Some(Dialog::RenamePreset);
                    }
                    if button(ui, "Reset EQ", false).clicked() {
                        self.dialog = Some(Dialog::Confirm(
                            "Reset custom EQ",
                            "Restore the speaker’s default nine-band EQ.",
                            Control::ResetEq,
                        ));
                    }
                });
            });
        });
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(title("Preset library", 19.0));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if button(ui, "Import preset", false).clicked() {
                        self.editor.clear();
                        self.dialog = Some(Dialog::ImportPreset);
                    }
                });
            });
            if self.library.presets.is_empty() {
                ui.label(secondary("Saved and imported presets appear here."));
            }
            let mut remove = None;
            for index in 0..self.library.presets.len() {
                let preset = self.library.presets[index].clone();
                ui.horizontal(|ui| {
                    ui.label(title(&preset.name, 14.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if button(ui, "Remove", false).clicked() {
                            remove = Some(index);
                        }
                        if button(ui, "Copy JSON", false).clicked()
                            && let Ok(json) = serde_json::to_string_pretty(&preset)
                        {
                            ui.ctx().copy_text(json);
                        }
                        if ui
                            .add_enabled(
                                self.can_edit(),
                                egui::Button::new("Apply").corner_radius(8),
                            )
                            .clicked()
                        {
                            self.control(Control::Preset(preset));
                        }
                    });
                });
            }
            if let Some(index) = remove {
                self.library.presets.remove(index);
                self.save();
            }
        });
    }

    fn room(&mut self, ui: &mut Ui) {
        let Some(sound) = self.state.speaker.sound else {
            card(ui, |ui| {
                ui.label(secondary(
                    "Connect the speaker to adjust its room compensation.",
                ));
            });
            return;
        };
        let mut draft = sound;
        draft.cutoff = self.cutoff_draft.unwrap_or(sound.cutoff);
        card(ui, |ui| {
            ui.label(title("Low cutoff", 20.0));
            ui.label(secondary("High-pass filter for low frequencies."));
            ui.add_enabled_ui(self.can_edit(), |ui| {
                ui.spacing_mut().slider_width = (ui.available_width() * 0.5).min(380.0);
                let response = ui.add(
                    egui::Slider::new(&mut draft.cutoff, 20..=100)
                        .step_by(5.0)
                        .suffix(" Hz"),
                );
                if response.changed() {
                    self.cutoff_draft = Some(draft.cutoff);
                }
                if response.drag_stopped() || (response.changed() && !response.dragged()) {
                    self.cutoff_draft = None;
                    self.control(Control::Sound(draft, false));
                }
                ui.horizontal_wrapped(|ui| {
                    for slope in 0..4 {
                        if ui
                            .selectable_label(
                                sound.slope == slope,
                                format!("{} dB/oct", (slope + 1) * 6),
                            )
                            .clicked()
                        {
                            self.control(Control::Sound(
                                crate::device::Sound { slope, ..sound },
                                false,
                            ));
                        }
                    }
                });
            });
        });
        card(ui, |ui| {
            ui.label(title("Acoustic space", 20.0));
            ui.label(secondary("Reduce bass buildup near walls and corners."));
            ui.add_enabled_ui(self.can_edit(), |ui| {
                ui.horizontal_wrapped(|ui| {
                    for space in 0..=4 {
                        let text = if space == 0 {
                            "0 dB".to_owned()
                        } else {
                            format!("−{space} dB")
                        };
                        if ui.selectable_label(sound.space == space, text).clicked() {
                            self.control(Control::Sound(
                                crate::device::Sound { space, ..sound },
                                false,
                            ));
                        }
                    }
                });
            });
        });
        card(ui, |ui| {
            ui.label(title("Desktop control", 20.0));
            ui.label(secondary(
                "Compensate for reflections from the work surface.",
            ));
            ui.add_enabled_ui(self.can_edit(), |ui| {
                let mut desktop = sound.desktop;
                if ui
                    .checkbox(&mut desktop, "Enable desktop compensation")
                    .changed()
                {
                    self.control(Control::Sound(
                        crate::device::Sound { desktop, ..sound },
                        false,
                    ));
                }
                if button(ui, "Reset room tuning", false).clicked() {
                    self.dialog = Some(Dialog::Confirm(
                        "Reset room tuning",
                        "Restore the speaker’s acoustic defaults.",
                        Control::Sound(sound, true),
                    ));
                }
            });
        });
    }

    fn devices(&mut self, ui: &mut Ui) {
        let enabled = self.state.busy.is_none() && !self.submitted;
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(title("Bluetooth devices", 20.0));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add_enabled(enabled, egui::Button::new("Scan nearby"))
                        .clicked()
                    {
                        self.send(Action::Discover {
                            auto: false,
                            saved: None,
                        });
                    }
                });
            });
            if self.state.candidates.is_empty() {
                ui.label(secondary(
                    "Put the MR3 in Bluetooth mode and scan for its control radio.",
                ));
            }
            let selected = self.state.link.as_ref().map(|link| link.endpoint);
            for found in self.state.candidates.clone() {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        let name = if found.mr3 {
                            "EDIFIER MR3"
                        } else if found.name.is_empty() {
                            "Edifier device"
                        } else {
                            found.name.as_str()
                        };
                        ui.label(title(name, 15.0));
                        ui.label(secondary(format!(
                            "{}  ·  {} dBm",
                            protocol::address_display(found.endpoint.address),
                            found.rssi
                        )));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if selected == Some(found.endpoint) {
                            if ui
                                .add_enabled(enabled, egui::Button::new("Disconnect control"))
                                .clicked()
                            {
                                self.send(Action::Disconnect);
                            }
                        } else if ui
                            .add_enabled(enabled && found.mr3, egui::Button::new("Connect"))
                            .clicked()
                        {
                            self.send(Action::Connect(found.endpoint));
                        } else if !found.mr3 {
                            ui.label(secondary("Profile unavailable"));
                        }
                    });
                });
            }
            if let Some(saved) = self.library.last_speaker
                && self
                    .state
                    .candidates
                    .iter()
                    .all(|candidate| candidate.endpoint != saved)
            {
                ui.horizontal(|ui| {
                    ui.label(secondary("Saved MR3"));
                    if selected == Some(saved) {
                        if ui
                            .add_enabled(enabled, egui::Button::new("Disconnect control"))
                            .clicked()
                        {
                            self.send(Action::Disconnect);
                        }
                    } else if ui
                        .add_enabled(enabled, egui::Button::new("Reconnect"))
                        .clicked()
                    {
                        self.send(Action::Connect(saved));
                    }
                });
            }
            if button(ui, "Windows Bluetooth settings ↗", false).clicked()
                && let Err(error) = std::process::Command::new("explorer.exe")
                    .arg("ms-settings:bluetooth")
                    .spawn()
            {
                self.notice = Some(format!("Could not open Bluetooth settings. {error}"));
            }
            ui.label(secondary("Pair EDIFIER MR3 in Windows to stream PC audio. OpenConneX uses a separate BLE link."));
        });
        let speaker = self.state.speaker.clone();
        if let Some((4, source)) = speaker.input {
            card(ui, |ui| {
                ui.label(title("Input source", 19.0));
                ui.add_enabled_ui(self.can_edit(), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (code, title) in [(1, "Bluetooth"), (2, "Line input")] {
                            if ui.selectable_label(source == code, title).clicked() {
                                self.control(Control::Source(code));
                            }
                        }
                    });
                });
                ui.label(secondary(
                    "Line input includes the analog TRS, RCA and AUX connections.",
                ));
            });
        }
        if self.state.link.is_some() {
            card(ui, |ui| {
                ui.label(title("Speaker controls", 19.0));
                ui.add_enabled_ui(self.can_edit(), |ui| {
                    if let Some(mut enabled) = speaker.prompt_tones
                        && ui
                            .checkbox(&mut enabled, "Enable speaker prompt tones")
                            .changed()
                    {
                        self.control(Control::PromptTones(enabled));
                    }
                    ui.horizontal_wrapped(|ui| {
                        if speaker.cap(3) > 0 && button(ui, "Rename speaker", false).clicked() {
                            self.editor = speaker.name.clone().unwrap_or_default();
                            self.dialog = Some(Dialog::SpeakerName);
                        }
                        if speaker.cap(6) == 1
                            && button(ui, "Disconnect audio source", false).clicked()
                        {
                            self.dialog = Some(Dialog::Confirm(
                                "Disconnect Bluetooth audio",
                                "Disconnect the current Bluetooth playback source.",
                                Control::DisconnectAudio,
                            ));
                        }
                        if speaker.cap(10) == 1 && button(ui, "Power off", false).clicked() {
                            self.dialog = Some(Dialog::Confirm(
                                "Power off the speaker",
                                "Use the physical power knob to turn the speaker back on.",
                                Control::PowerOff,
                            ));
                        }
                        if speaker.cap(17) & 32 != 0
                            && button(ui, title("Factory reset", 13.0).color(RED), false).clicked()
                        {
                            self.dialog = Some(Dialog::Confirm(
                                "Factory reset",
                                "Restore factory settings and clear saved speaker preferences.",
                                Control::FactoryReset,
                            ));
                        }
                    });
                });
            });
        }
        card(ui, |ui| {
            ui.label(title("Firmware and documentation", 19.0));
            let version = speaker.firmware.as_deref().unwrap_or("Connect to read");
            ui.label(secondary(format!("Installed firmware  {version}")));
            if speaker.cap(12) & 0x0a != 0 {
                ui.label(secondary(
                    "Use EDIFIER ConneX on your phone for firmware updates.",
                ));
            }
            ui.horizontal_wrapped(|ui| {
                ui.hyperlink_to("MR3 user manual ↗", MANUAL);
                ui.hyperlink_to("Edifier support ↗", SUPPORT);
            });
        });
    }

    fn about(&self, ui: &mut Ui) {
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.image((self.icon.id(), egui::vec2(44.0, 44.0)));
                ui.vertical(|ui| {
                    ui.label(title("OpenConneX", 22.0));
                    ui.label(secondary(format!("v{}", env!("CARGO_PKG_VERSION"))));
                });
            });
            ui.label("A native Windows Bluetooth controller for EDIFIER MR3 studio monitors.");
            ui.horizontal_wrapped(|ui| {
                ui.label(secondary("Created by"));
                ui.hyperlink_to("@aesxsc", "https://github.com/aesxsc");
            });
            ui.horizontal_wrapped(|ui| {
                ui.hyperlink_to("GitHub repository", env!("CARGO_PKG_REPOSITORY"));
                ui.hyperlink_to(
                    "WTFPL license",
                    concat!(env!("CARGO_PKG_REPOSITORY"), "/blob/main/LICENSE"),
                );
            });
            ui.separator();
            ui.label(secondary(
                "An independent, unofficial project with no affiliation with EDIFIER.",
            ));
            ui.label(accent("Made with love"));
        });
    }

    fn draw_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        let mut close = false;
        let response = egui::Modal::new(egui::Id::new("device-dialog")).show(ctx, |ui| {
            ui.set_width(360.0);
            let (heading, action) = match &dialog {
                Dialog::SavePreset => ("Save EQ preset", "Save"),
                Dialog::RenamePreset => ("Rename active preset", "Rename"),
                Dialog::ImportPreset => ("Import EQ preset", "Import"),
                Dialog::SpeakerName => ("Rename speaker", "Rename"),
                Dialog::Confirm(heading, _, _) => (*heading, "Confirm"),
            };
            ui.label(title(heading, 22.0));
            match &dialog {
                Dialog::Confirm(_, prompt, _) => {
                    ui.label(secondary(*prompt));
                }
                Dialog::ImportPreset => {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.editor)
                            .hint_text("Paste preset JSON")
                            .desired_rows(7)
                            .desired_width(f32::INFINITY)
                            .char_limit(8192),
                    );
                }
                _ => {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.editor)
                            .hint_text("Name")
                            .desired_width(f32::INFINITY)
                            .char_limit(28),
                    );
                }
            }
            ui.horizontal(|ui| {
                if button(ui, "Cancel", false).clicked() {
                    close = true;
                }
                if button(ui, title(action, 14.0).color(INK), true).clicked() {
                    match self.commit_dialog(&dialog) {
                        Ok(()) => close = true,
                        Err(error) => self.notice = Some(error),
                    }
                }
            });
            if let Some(notice) = &self.notice {
                ui.label(secondary(notice).color(AMBER));
            }
        });
        if !close && !response.should_close() {
            self.dialog = Some(dialog);
        }
    }

    fn commit_dialog(&mut self, dialog: &Dialog) -> Result<(), String> {
        let name = self.editor.trim().to_owned();
        match dialog {
            Dialog::Confirm(_, _, control) => self.control(control.clone()),
            Dialog::SpeakerName => {
                if name.is_empty() || name.len() > 24 || name.chars().any(char::is_control) {
                    return Err("Enter a Bluetooth name of 1–24 UTF-8 bytes.".into());
                }
                self.control(Control::SpeakerName(name));
            }
            Dialog::SavePreset | Dialog::RenamePreset => {
                let mut eq = self
                    .state
                    .speaker
                    .equalizer
                    .clone()
                    .ok_or("The speaker EQ is unavailable.")?;
                eq.name = name;
                eq.validate()?;
                if matches!(dialog, Dialog::RenamePreset) {
                    self.control(Control::PresetName(eq));
                } else {
                    let seconds = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as u32;
                    eq.id = seconds.to_le_bytes();
                    self.library.presets.push(eq);
                    self.save();
                }
            }
            Dialog::ImportPreset => {
                let eq: Equalizer = serde_json::from_str(&self.editor)
                    .map_err(|error| format!("Invalid preset JSON. {error}"))?;
                eq.validate()?;
                self.library.presets.push(eq);
                self.save();
            }
        }
        Ok(())
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.receive();
        let narrow = ui.available_width() < 800.0;
        if narrow {
            egui::Panel::top("navigation")
                .frame(
                    Frame::new()
                        .fill(RAIL)
                        .inner_margin(Margin::symmetric(18, 12)),
                )
                .show_inside(ui, |ui| self.navigation(ui, true));
        } else {
            egui::Panel::left("navigation")
                .exact_size(215.0)
                .resizable(false)
                .frame(
                    Frame::new()
                        .fill(RAIL)
                        .inner_margin(Margin::symmetric(17, 22)),
                )
                .show_inside(ui, |ui| self.navigation(ui, false));
        }
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(BG)
                    .inner_margin(Margin::symmetric(26, 22)),
            )
            .show_inside(ui, |ui| {
                self.header(ui);
                ScrollArea::vertical()
                    .id_salt(self.page.title())
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.page {
                        Page::Overview => self.overview(ui),
                        Page::Equalizer => self.equalizer(ui),
                        Page::Room => self.room(ui),
                        Page::Devices => self.devices(ui),
                        Page::Diagnostics => {
                            if diagnostics::show(ui, &self.state, self.can_edit()) {
                                self.send(Action::Read);
                            }
                        }
                        Page::About => self.about(ui),
                    });
            });
        self.draw_dialog(ui.ctx());
    }
}
