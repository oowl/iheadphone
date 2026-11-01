//! Apple Accessory Protocol (AAP) messages used by AirPods.
//!
//! This crate deliberately contains no Bluetooth I/O. Protocol facts and test
//! vectors were reviewed against the MIT/Apache-2.0 `Rockykln/podctl` project.

use aes::Aes128;
use aes::cipher::{BlockEncrypt, KeyInit};

pub const AAP_PSM: u16 = 0x1001;
const MAGIC: [u8; 4] = [0x04, 0x00, 0x04, 0x00];

pub const HANDSHAKE: &[u8] = &[
    0x00, 0x00, 0x04, 0x00, 0x01, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];
pub const SET_FEATURES: &[u8] = &[
    0x04, 0x00, 0x04, 0x00, 0x4d, 0x00, 0xd7, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];
pub const SUBSCRIBE_NOTIFICATIONS: &[u8] = &[
    0x04, 0x00, 0x04, 0x00, 0x0f, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff,
];
pub const REQUEST_PROXIMITY_KEYS: &[u8] = &[0x04, 0x00, 0x04, 0x00, 0x30, 0x00, 0x05, 0x00];

mod opcode {
    pub const BATTERY: u8 = 0x04;
    pub const EAR_DETECTION: u8 = 0x06;
    pub const SETTINGS: u8 = 0x09;
}

mod setting_id {
    pub const MICROPHONE: u8 = 0x01;
    pub const EAR_DETECTION: u8 = 0x0a;
    pub const NOISE_CONTROL: u8 = 0x0d;
    pub const ONE_BUD_ANC: u8 = 0x1b;
    pub const CONVERSATION_AWARENESS: u8 = 0x28;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListeningMode {
    Off,
    NoiseCancelling,
    Transparency,
    Adaptive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Microphone {
    Automatic,
    Right,
    Left,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PodState {
    InEar,
    OutOfEar,
    InCase,
    Unknown(u8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Side {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WearingState {
    pub primary: PodState,
    pub secondary: PodState,
}

impl WearingState {
    pub fn any_in_ear(self) -> bool {
        matches!(self.primary, PodState::InEar) || matches!(self.secondary, PodState::InEar)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BatteryState {
    /// The physical side represented by `WearingState::primary`.
    pub primary: Option<Side>,
    pub left: Option<u8>,
    pub right: Option<u8>,
    pub case: Option<u8>,
    pub left_charging: bool,
    pub right_charging: bool,
    pub case_charging: bool,
}

impl BatteryState {
    pub fn earbuds_percent(self) -> Option<u8> {
        match (self.left, self.right) {
            (Some(left), Some(right)) => Some(((u16::from(left) + u16::from(right)) / 2) as u8),
            (Some(level), None) | (None, Some(level)) => Some(level),
            (None, None) => self.case,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Setting {
    Mode(ListeningMode),
    ConversationAwareness(bool),
    EarDetection(bool),
    OneBudNoiseCancelling(bool),
    Microphone(Microphone),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Update {
    Battery(BatteryState),
    Wearing(WearingState),
    Mode(ListeningMode),
    ConversationAwareness(bool),
    EarDetection(bool),
    OneBudNoiseCancelling(bool),
    Microphone(Microphone),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParseError {
    ShortFrame,
    InvalidMagic,
    MalformedBattery,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProximityKeys {
    pub identity: [u8; 16],
    pub advertisement: [u8; 16],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaseAdvertisement {
    pub model: u16,
    pub case_battery: Option<u8>,
    pub case_charging: bool,
}

pub fn initialization_packets() -> [&'static [u8]; 4] {
    [
        HANDSHAKE,
        SET_FEATURES,
        SUBSCRIBE_NOTIFICATIONS,
        REQUEST_PROXIMITY_KEYS,
    ]
}

pub fn parse_proximity_keys(packet: &[u8]) -> Option<ProximityKeys> {
    if packet.len() < 7 || packet[1..4] != MAGIC[1..4] || packet[4] != 0x31 {
        return None;
    }
    let mut identity = None;
    let mut advertisement = None;
    let mut rest = packet.get(7..)?;
    while rest.len() >= 4 {
        let kind = rest[0];
        let length = u16::from_be_bytes([rest[1], rest[2]]) as usize;
        let body = rest.get(4..4 + length)?;
        if length == 16 {
            let mut key: [u8; 16] = body.try_into().ok()?;
            key.reverse();
            match kind {
                1 => identity = Some(key),
                4 => advertisement = Some(key),
                _ => {}
            }
        }
        rest = &rest[4 + length..];
    }
    Some(ProximityKeys {
        identity: identity?,
        advertisement: advertisement?,
    })
}

pub fn address_resolves(identity_key: &[u8; 16], address: [u8; 6]) -> bool {
    if address[0] >> 6 != 0b01 {
        return false;
    }
    let mut block = [0u8; 16];
    block[13..].copy_from_slice(&address[..3]);
    let cipher = Aes128::new(identity_key.into());
    cipher.encrypt_block((&mut block).into());
    block[13..] == address[3..]
}

pub fn parse_case_advertisement(data: &[u8]) -> Option<CaseAdvertisement> {
    if data.len() < 11 || data[0] != 0x07 || data[1] != 0x19 {
        return None;
    }
    let case = data[7];
    let battery_nibble = case & 0x0f;
    Some(CaseAdvertisement {
        model: u16::from_le_bytes([data[3], data[4]]),
        case_battery: (battery_nibble <= 10).then_some(battery_nibble * 10),
        case_charging: case >> 4 & 0x04 != 0,
    })
}

pub fn encode_setting(setting: Setting) -> Vec<u8> {
    let (id, value) = match setting {
        Setting::Mode(mode) => (setting_id::NOISE_CONTROL, mode_to_wire(mode)),
        Setting::ConversationAwareness(enabled) => {
            (setting_id::CONVERSATION_AWARENESS, switch_to_wire(enabled))
        }
        Setting::EarDetection(enabled) => (setting_id::EAR_DETECTION, switch_to_wire(enabled)),
        Setting::OneBudNoiseCancelling(enabled) => {
            (setting_id::ONE_BUD_ANC, switch_to_wire(enabled))
        }
        Setting::Microphone(microphone) => (
            setting_id::MICROPHONE,
            match microphone {
                Microphone::Automatic => 0,
                Microphone::Right => 1,
                Microphone::Left => 2,
            },
        ),
    };
    let mut packet = MAGIC.to_vec();
    packet.extend_from_slice(&[opcode::SETTINGS, 0x00, id, value, 0x00, 0x00, 0x00]);
    packet
}

pub fn parse(packet: &[u8]) -> Result<Vec<Update>, ParseError> {
    if packet.len() < 6 {
        return Err(ParseError::ShortFrame);
    }
    // Handshake responses start with 0x01; bytes 1..4 remain stable.
    if packet[1..4] != MAGIC[1..4] {
        return Err(ParseError::InvalidMagic);
    }
    let payload = &packet[6..];
    match packet[4] {
        opcode::BATTERY => parse_battery(payload).map(|value| vec![Update::Battery(value)]),
        opcode::EAR_DETECTION => Ok(parse_wearing(payload)
            .map(Update::Wearing)
            .into_iter()
            .collect()),
        opcode::SETTINGS => Ok(parse_setting(payload).into_iter().collect()),
        _ => Ok(Vec::new()),
    }
}

fn parse_battery(payload: &[u8]) -> Result<BatteryState, ParseError> {
    let Some(&declared) = payload.first() else {
        return Err(ParseError::MalformedBattery);
    };
    let count = usize::from(declared);
    if payload.len() < 1 + count * 5 {
        return Err(ParseError::MalformedBattery);
    }
    let mut state = BatteryState::default();
    for component in payload[1..].as_chunks::<5>().0.iter().take(count) {
        let level = (component[2] <= 100 && component[3] != 0x04).then_some(component[2]);
        let charging = component[3] == 0x01;
        match component[0] {
            0x02 => {
                state.primary.get_or_insert(Side::Right);
                state.right = level;
                state.right_charging = charging;
            }
            0x04 => {
                state.primary.get_or_insert(Side::Left);
                state.left = level;
                state.left_charging = charging;
            }
            0x08 => {
                state.case = level;
                state.case_charging = charging;
            }
            _ => {}
        }
    }
    Ok(state)
}

fn parse_wearing(payload: &[u8]) -> Option<WearingState> {
    Some(WearingState {
        primary: pod_state(*payload.first()?),
        secondary: pod_state(*payload.get(1)?),
    })
}

fn parse_setting(payload: &[u8]) -> Option<Update> {
    let (&id, values) = payload.split_first()?;
    let &value = values.first()?;
    match id {
        setting_id::NOISE_CONTROL => mode_from_wire(value).map(Update::Mode),
        setting_id::CONVERSATION_AWARENESS => {
            switch_from_wire(value).map(Update::ConversationAwareness)
        }
        setting_id::EAR_DETECTION => switch_from_wire(value).map(Update::EarDetection),
        setting_id::ONE_BUD_ANC => switch_from_wire(value).map(Update::OneBudNoiseCancelling),
        setting_id::MICROPHONE => Some(Update::Microphone(match value {
            0 => Microphone::Automatic,
            1 => Microphone::Right,
            2 => Microphone::Left,
            _ => return None,
        })),
        _ => None,
    }
}

fn pod_state(value: u8) -> PodState {
    match value {
        0 => PodState::InEar,
        1 => PodState::OutOfEar,
        2 => PodState::InCase,
        unknown => PodState::Unknown(unknown),
    }
}

fn mode_to_wire(mode: ListeningMode) -> u8 {
    match mode {
        ListeningMode::Off => 1,
        ListeningMode::NoiseCancelling => 2,
        ListeningMode::Transparency => 3,
        ListeningMode::Adaptive => 4,
    }
}

fn mode_from_wire(value: u8) -> Option<ListeningMode> {
    Some(match value {
        1 => ListeningMode::Off,
        2 => ListeningMode::NoiseCancelling,
        3 => ListeningMode::Transparency,
        4 => ListeningMode::Adaptive,
        _ => return None,
    })
}

fn switch_to_wire(enabled: bool) -> u8 {
    if enabled { 1 } else { 2 }
}

fn switch_from_wire(value: u8) -> Option<bool> {
    match value {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_all_listening_modes() {
        let values = [
            (ListeningMode::Off, 1),
            (ListeningMode::NoiseCancelling, 2),
            (ListeningMode::Transparency, 3),
            (ListeningMode::Adaptive, 4),
        ];
        for (mode, wire) in values {
            assert_eq!(
                encode_setting(Setting::Mode(mode)),
                [MAGIC.as_slice(), &[0x09, 0x00, 0x0d, wire, 0, 0, 0]].concat()
            );
        }
    }

    #[test]
    fn decodes_real_battery_layout() {
        let packet = [
            0x04, 0x00, 0x04, 0x00, 0x04, 0x00, 0x03, 0x02, 0x01, 100, 0x02, 0x01, 0x04, 0x01, 99,
            0x01, 0x01, 0x08, 0x01, 17, 0x02, 0x01,
        ];
        assert_eq!(
            parse(&packet),
            Ok(vec![Update::Battery(BatteryState {
                primary: Some(Side::Right),
                right: Some(100),
                left: Some(99),
                case: Some(17),
                left_charging: true,
                ..BatteryState::default()
            })])
        );
    }

    #[test]
    fn decodes_wearing_and_settings() {
        let wearing = [0x04, 0, 0x04, 0, 0x06, 0, 0, 2];
        assert_eq!(
            parse(&wearing),
            Ok(vec![Update::Wearing(WearingState {
                primary: PodState::InEar,
                secondary: PodState::InCase,
            })])
        );
        let adaptive = [0x04, 0, 0x04, 0, 0x09, 0, 0x0d, 4, 0, 0, 0];
        assert_eq!(
            parse(&adaptive),
            Ok(vec![Update::Mode(ListeningMode::Adaptive)])
        );
    }

    #[test]
    fn rejects_truncated_battery_without_panicking() {
        let packet = [0x04, 0, 0x04, 0, 0x04, 0, 3, 2, 1, 50];
        assert_eq!(parse(&packet), Err(ParseError::MalformedBattery));
    }

    #[test]
    fn resolves_bluetooth_spec_rpa_sample() {
        let key = [
            0xec, 0x02, 0x34, 0xa3, 0x57, 0xc8, 0xad, 0x05, 0x34, 0x10, 0x10, 0xa6, 0x0a, 0x39,
            0x7d, 0x9b,
        ];
        assert!(address_resolves(&key, [0x70, 0x81, 0x94, 0x0d, 0xfb, 0xaa]));
    }

    #[test]
    fn reads_case_battery_from_proximity_advertisement() {
        let bytes = [
            0x07, 0x19, 0x01, 0x24, 0x20, 0x75, 0x77, 0xb8, 0x3a, 0x00, 0x05, 0xbe, 0x5e, 0xc3,
            0x5a, 0x37, 0xdc, 0xf3, 0x27, 0xb8, 0x47, 0x86, 0x82, 0x68, 0x5f, 0x4b, 0xd6,
        ];
        let advert = parse_case_advertisement(&bytes).unwrap();
        assert_eq!(advert.model, 0x2024);
        assert_eq!(advert.case_battery, Some(80));
        assert!(!advert.case_charging);
    }
}
