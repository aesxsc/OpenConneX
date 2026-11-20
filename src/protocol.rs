pub const APP_CODE: u8 = 0xec;
pub const SERVICE_UUID: &str = "4809f301-1a48-11e9-ab14-d663bd873d93";
pub const RX_UUID: &str = "48090001-1a48-11e9-ab14-d663bd873d93";
pub const TX_UUID: &str = "48090002-1a48-11e9-ab14-d663bd873d93";
pub const DISCOVERY_UUID: &str = "0000f300-0000-1000-8000-00805f9b34fb";

pub mod cmd {
    pub const RESET: u8 = 0x07;
    pub const EQ_READ: u8 = 0x43;
    pub const BAND_WRITE: u8 = 0x44;
    pub const EQ_RESET: u8 = 0x45;
    pub const EQ_LOAD: u8 = 0x46;
    pub const EQ_NAME: u8 = 0x47;
    pub const PLAY_INFO: u8 = 0x50;
    pub const INPUT_READ: u8 = 0x61;
    pub const INPUT_WRITE: u8 = 0x62;
    pub const VOLUME_READ: u8 = 0x66;
    pub const VOLUME_WRITE: u8 = 0x67;
    pub const CODEC: u8 = 0x68;
    pub const TONE_READ: u8 = 0x86;
    pub const TONE_WRITE: u8 = 0x87;
    pub const HEADPHONES: u8 = 0xc0;
    pub const PLAYBACK: u8 = 0xc2;
    pub const A2DP: u8 = 0xc3;
    pub const SOUND_WRITE: u8 = 0xc4;
    pub const VERSION: u8 = 0xc6;
    pub const ADDRESS: u8 = 0xc8;
    pub const NAME: u8 = 0xc9;
    pub const NAME_WRITE: u8 = 0xca;
    pub const AUDIO_DISCONNECT: u8 = 0xcd;
    pub const POWER_OFF: u8 = 0xce;
    pub const SOUND_READ: u8 = 0xd5;
    pub const CAPABILITIES: u8 = 0xd8;
}

#[derive(Debug, PartialEq, Eq)]
pub struct Packet {
    pub code: u8,
    pub data: Vec<u8>,
}

pub fn request(code: u8, data: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(data.len() + 6);
    bytes.extend_from_slice(&[0xaa, APP_CODE, code]);
    bytes.extend_from_slice(&(data.len() as u16).to_be_bytes());
    bytes.extend_from_slice(data);
    bytes.push(checksum(&bytes));
    bytes
}

fn checksum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte))
}

#[derive(Default)]
pub struct Decoder {
    buffer: Vec<u8>,
    pub invalid: usize,
}

impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Packet> {
        self.buffer.extend_from_slice(bytes);
        let mut packets = Vec::new();
        loop {
            let marker = self.buffer.iter().position(|b| matches!(b, 0xbb | 0xcc));
            let skip = marker.unwrap_or(self.buffer.len());
            self.buffer.drain(..skip);
            if self.buffer.len() < 6 {
                break;
            }
            let data_size = u16::from_be_bytes([self.buffer[3], self.buffer[4]]) as usize;
            if self.buffer[1] != APP_CODE || data_size > 1024 {
                self.invalid += 1;
                self.buffer.drain(..1);
                continue;
            }
            let end = data_size + 5;
            if self.buffer.len() <= end {
                break;
            }
            if checksum(&self.buffer[..end]) == self.buffer[end] {
                packets.push(Packet {
                    code: self.buffer[2],
                    data: self.buffer[5..end].to_vec(),
                });
                self.buffer.drain(..=end);
            } else {
                self.invalid += 1;
                self.buffer.drain(..1);
            }
        }
        packets
    }
}

pub fn hex(bytes: &[u8], separator: &str) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(separator)
}

pub fn address_display(address: u64) -> String {
    hex(&address.to_be_bytes()[2..], ":")
}

pub fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_matches(char::from(0))
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_version_and_fragmented_custom_eq() {
        assert_eq!(request(cmd::VERSION, &[]), [0xaa, 0xec, 0xc6, 0, 0, 0x5c]);
        let mut decoder = Decoder::default();
        assert!(decoder.push(&[0xbb, 0xec, 0xc6, 0, 3, 1]).is_empty());
        assert_eq!(
            decoder.push(&[0, 7, 0x78, 0xbb, 0xec, 0x66, 0, 2, 0x1e, 0x0b, 0x38]),
            [
                Packet {
                    code: cmd::VERSION,
                    data: vec![1, 0, 7]
                },
                Packet {
                    code: cmd::VOLUME_READ,
                    data: vec![30, 11]
                }
            ]
        );
    }

    #[test]
    fn discards_bad_checksum_and_finds_next_frame() {
        let mut decoder = Decoder::default();
        let packets = decoder.push(&[
            0xbb, 0xec, 0x66, 0, 2, 0x1e, 0x0b, 0x39, 0xbb, 0xec, 0xc3, 0, 1, 3, 0x6e,
        ]);
        assert_eq!(
            packets,
            [Packet {
                code: cmd::A2DP,
                data: vec![3]
            }]
        );
        assert_eq!(decoder.invalid, 1);
    }
}
