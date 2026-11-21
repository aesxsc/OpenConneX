use crate::protocol::{self, Packet, cmd};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Endpoint {
    pub address: u64,
    pub random: bool,
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub endpoint: Endpoint,
    pub rssi: i16,
    pub name: String,
    pub mr3: bool,
    pub company: Option<u16>,
    pub advertisement: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct Adapter {
    pub address: String,
    pub name: String,
    pub radio: String,
    pub supports_le: Option<bool>,
    pub supports_classic: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct Link {
    pub endpoint: Endpoint,
    pub name: String,
    pub att_mtu: u16,
    pub rssi: Option<i16>,
    pub company: Option<u16>,
    pub advertisement: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sound {
    pub mode: u8,
    pub layout: u8,
    pub cutoff: u8,
    pub slope: u8,
    pub space: u8,
    pub desktop: bool,
}

impl Sound {
    pub fn encode(self, reset: bool) -> [u8; 7] {
        [
            self.mode,
            self.layout,
            u8::from(reset),
            self.cutoff.clamp(20, 100),
            self.slope.min(3),
            self.space.min(4),
            u8::from(self.desktop),
        ]
    }
}

pub const BANDS: [u16; 9] = [62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Equalizer {
    pub name: String,
    pub gains: [u8; 9],
    pub id: [u8; 4],
}

impl Equalizer {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty()
            || self.name.len() > 28
            || self.name.chars().any(char::is_control)
        {
            return Err("Use a preset name of 1–28 UTF-8 bytes.".into());
        }
        if self.gains.iter().any(|gain| *gain > 12) {
            return Err("Each band must be between −3 and +3 dB.".into());
        }
        Ok(())
    }

    pub fn payload(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(41 + self.name.len());
        data.push(0);
        for (index, frequency) in BANDS.iter().enumerate() {
            data.push(index as u8);
            data.extend(frequency.to_be_bytes());
            data.push(self.gains[index]);
        }
        data.extend(self.id);
        data.extend(self.name.as_bytes());
        data
    }
}

#[derive(Clone, Debug, Default)]
pub struct Speaker {
    pub name: Option<String>,
    pub mac: Option<String>,
    pub firmware: Option<String>,
    pub capability: Vec<u8>,
    pub sound: Option<Sound>,
    pub equalizer: Option<Equalizer>,
    pub volume: Option<(u8, u8)>,
    pub input: Option<(u8, u8)>,
    pub codec: Option<u8>,
    pub a2dp: Option<u8>,
    pub playback: Option<bool>,
    pub track: String,
    pub artist: String,
    pub wired: Option<bool>,
    pub prompt_tones: Option<bool>,
    pub responses: BTreeMap<u8, Vec<u8>>,
}

impl Speaker {
    pub fn apply(&mut self, packet: &Packet) {
        use cmd::*;
        let buf = packet.data.as_slice();
        self.responses.insert(packet.code, packet.data.clone());
        match (packet.code, buf) {
            (NAME, [_, ..]) => self.name = Some(protocol::text(buf)),
            (ADDRESS, [_, _, _, _, _, _]) => self.mac = Some(protocol::hex(buf, ":")),
            (VERSION, [_, ..]) => {
                self.firmware = Some(buf.iter().map(u8::to_string).collect::<Vec<_>>().join("."))
            }
            (CAPABILITIES, [_, ..]) => self.capability = buf.to_vec(),
            (VOLUME_READ, [max, level, ..]) if *max > 0 && level <= max => {
                self.volume = Some((*max, *level))
            }
            (INPUT_READ, [group, source, ..]) => self.input = Some((*group, *source)),
            (SOUND_READ, [mode, layout, _, cutoff, slope, space, desktop, ..]) => {
                self.sound = Some(Sound {
                    mode: *mode,
                    layout: *layout,
                    cutoff: *cutoff,
                    slope: *slope,
                    space: *space,
                    desktop: *desktop == 1,
                });
            }
            (A2DP, [state, ..]) => self.a2dp = Some(*state),
            (CODEC, [codec, ..]) => self.codec = Some(*codec),
            (TONE_READ, [2, state @ 0..=1, ..]) => self.prompt_tones = Some(*state == 1),
            (HEADPHONES, [state @ 0..=1, ..]) => self.wired = Some(*state == 1),
            (PLAY_INFO, [playing, ..]) => {
                self.playback = Some(*playing == 1);
                if let [_, title_length, artist_length, metadata @ ..] = buf {
                    let boundary = usize::from(*title_length);
                    let end = boundary + usize::from(*artist_length);
                    if end <= metadata.len() {
                        self.track = protocol::text(&metadata[..boundary]);
                        self.artist = protocol::text(&metadata[boundary..end]);
                    }
                }
            }
            (EQ_READ, [12, 9, bands @ ..]) if bands.len() >= 37 => {
                let mut gains = [6; 9];
                for (index, band) in bands[1..37].chunks_exact(4).enumerate() {
                    if band[0] as usize != index
                        || u16::from_be_bytes([band[1], band[2]]) != BANDS[index]
                        || band[3] > 12
                    {
                        return;
                    }
                    gains[index] = band[3];
                }
                let id = bands
                    .get(37..41)
                    .and_then(|bytes| bytes.try_into().ok())
                    .unwrap_or_default();
                let name = bands
                    .get(41..)
                    .map(protocol::text)
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "Custom".into());
                self.equalizer = Some(Equalizer { name, gains, id });
            }
            _ => {}
        }
    }

    pub fn cap(&self, offset: usize) -> u8 {
        self.capability.get(offset).copied().unwrap_or_default()
    }

    pub fn is_mr3(&self) -> bool {
        self.cap(13) == 30 && self.sound.is_some_and(|sound| sound.layout == 1)
    }

    pub fn input_title(&self) -> &'static str {
        match self.input {
            Some((4, 1)) => "Bluetooth",
            Some((4, 2)) => "Line input",
            _ => "Unknown input",
        }
    }

    pub fn mode_title(&self) -> &'static str {
        match self.sound.map(|s| s.mode) {
            Some(0) => "Monitor",
            Some(1) => "Music",
            Some(2) => "Custom",
            _ => "Unknown preset",
        }
    }

    pub fn a2dp_title(&self) -> &'static str {
        match self.a2dp {
            Some(0) => "Standby",
            Some(1) => "Connectable",
            Some(2) => "Pairing",
            Some(3) => "Connected",
            Some(13) => "Streaming",
            None => "Unavailable",
            _ => "Other",
        }
    }

    pub fn playback_available(&self) -> bool {
        self.input == Some((4, 1)) && matches!(self.a2dp, Some(3 | 13))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_device_readbacks() {
        let mut speaker = Speaker::default();
        speaker.apply(&Packet {
            code: cmd::SOUND_READ,
            data: vec![0, 1, 0, 40, 1, 3, 1],
        });
        assert_eq!(speaker.sound.unwrap().encode(false), [0, 1, 0, 40, 1, 3, 1]);
        speaker.apply(&Packet {
            code: cmd::VOLUME_READ,
            data: vec![30, 11],
        });
        assert_eq!(speaker.volume, Some((30, 11)));
        speaker.apply(&Packet {
            code: cmd::VOLUME_READ,
            data: vec![30, 250],
        });
        assert_eq!(speaker.volume, Some((30, 11)));
    }

    #[test]
    fn round_trips_mr3_equalizer() {
        let preset = Equalizer {
            name: "Nearfield".into(),
            gains: [6, 3, 8, 9, 6, 4, 2, 8, 6],
            id: [1, 2, 3, 4],
        };
        assert!(preset.validate().is_ok());
        let mut data = vec![12, 9];
        data.extend(preset.payload());
        let mut speaker = Speaker::default();
        speaker.apply(&Packet {
            code: cmd::EQ_READ,
            data,
        });
        assert_eq!(speaker.equalizer, Some(preset));
    }
}
