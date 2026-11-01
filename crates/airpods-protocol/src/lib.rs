//! Apple Accessory Protocol (AAP) messages used by AirPods.
//!
//! This crate deliberately contains no Bluetooth I/O. Protocol facts and test
//! vectors were reviewed against the MIT/Apache-2.0 `Rockykln/podctl` project.
//! Device information and setting layouts were cross-checked with LibrePods'
//! published AAP notes and live AirPods Pro 2 captures.

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
    pub const DEVICE_INFO: u8 = 0x1d;
    pub const CONVERSATION: u8 = 0x4b;
}

mod setting_id {
    pub const MICROPHONE: u8 = 0x01;
    pub const EAR_DETECTION: u8 = 0x0a;
    pub const NOISE_CONTROL: u8 = 0x0d;
    pub const PRESS_SPEED: u8 = 0x17;
    pub const PRESS_AND_HOLD_DURATION: u8 = 0x18;
    pub const LISTENING_MODE_CYCLE: u8 = 0x1a;
    pub const ONE_BUD_ANC: u8 = 0x1b;
    pub const CHIME_VOLUME: u8 = 0x1f;
    pub const VOLUME_SWIPE_INTERVAL: u8 = 0x23;
    pub const VOLUME_SWIPE: u8 = 0x25;
    pub const PERSONALIZED_VOLUME: u8 = 0x26;
    pub const CONVERSATION_AWARENESS: u8 = 0x28;
    pub const ADAPTIVE_NOISE_LEVEL: u8 = 0x2e;
}

/// Second byte the accessory pairs with the chime volume, both in its
/// settings dump and in the writes it accepts.
const CHIME_VOLUME_SUFFIX: u8 = 0x50;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListeningMode {
    Off,
    NoiseCancelling,
    Transparency,
    Adaptive,
}

impl ListeningMode {
    pub const ALL: [Self; 4] = [
        Self::Off,
        Self::NoiseCancelling,
        Self::Transparency,
        Self::Adaptive,
    ];

    const fn cycle_bit(self) -> u8 {
        match self {
            Self::Off => 0x01,
            Self::NoiseCancelling => 0x02,
            Self::Transparency => 0x04,
            Self::Adaptive => 0x08,
        }
    }
}

/// The modes that pressing and holding the stem (or the Max noise control
/// button) cycles through. The accessory expects at least two of them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ListeningModeCycle(u8);

impl ListeningModeCycle {
    pub fn from_modes(modes: impl IntoIterator<Item = ListeningMode>) -> Self {
        modes
            .into_iter()
            .fold(Self(0), |cycle, mode| cycle.with(mode, true))
    }

    pub const fn from_bits(bits: u8) -> Self {
        Self(bits & 0x0f)
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn contains(self, mode: ListeningMode) -> bool {
        self.0 & mode.cycle_bit() != 0
    }

    #[must_use]
    pub const fn with(self, mode: ListeningMode, included: bool) -> Self {
        if included {
            Self(self.0 | mode.cycle_bit())
        } else {
            Self(self.0 & !mode.cycle_bit())
        }
    }

    pub const fn len(self) -> u32 {
        self.0.count_ones()
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// How much time the accessory allows between presses, for a press and hold,
/// or between volume swipes. Larger values give the wearer more time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Timing {
    Default,
    Longer,
    Longest,
}

impl Timing {
    pub const ALL: [Self; 3] = [Self::Default, Self::Longer, Self::Longest];

    const fn to_wire(self) -> u8 {
        match self {
            Self::Default => 0,
            Self::Longer => 1,
            Self::Longest => 2,
        }
    }

    const fn from_wire(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Default,
            1 => Self::Longer,
            2 => Self::Longest,
            _ => return None,
        })
    }
}

/// Live speech detection from Conversation Awareness (opcode 0x4b). The
/// accessory only reports it; lowering media volume is up to the host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Conversation {
    /// The wearer is speaking: lower media volume.
    Speaking,
    /// Speech ended and volume should ease back; 1 is nearly ducked, 4 nearly
    /// restored.
    Easing(u8),
    /// Back to normal volume.
    Ended,
}

impl Conversation {
    /// Levels 1–3 arrive while speech starts and continues, 4–7 as it fades,
    /// and 8 or more once it has ended.
    pub const fn from_level(level: u8) -> Option<Self> {
        Some(match level {
            1..=3 => Self::Speaking,
            4..=7 => Self::Easing(level - 3),
            8.. => Self::Ended,
            0 => return None,
        })
    }
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

/// AirPods generations with a known Bluetooth product ID or model number.
///
/// Product IDs follow podctl's table (0x2024 hardware-verified there); model
/// numbers follow Apple's "Identify your AirPods" support article.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Model {
    AirPods1,
    AirPods2,
    AirPods3,
    AirPods4,
    AirPods4Anc,
    AirPodsPro,
    AirPodsPro2,
    AirPodsPro2UsbC,
    AirPodsPro3,
    AirPodsMax,
    AirPodsMaxUsbC,
    AirPodsMax2,
}

impl Model {
    /// Maps the product half of the BlueZ modalias (`bluetooth:v004Cp2024…`).
    pub fn from_product_id(product: u32) -> Option<Self> {
        Some(match product {
            0x2002 => Self::AirPods1,
            0x200f => Self::AirPods2,
            0x2013 => Self::AirPods3,
            0x201b => Self::AirPods4Anc,
            0x200e => Self::AirPodsPro,
            0x2014 => Self::AirPodsPro2,
            0x2024 => Self::AirPodsPro2UsbC,
            0x2027 => Self::AirPodsPro3,
            0x200a => Self::AirPodsMax,
            0x201f => Self::AirPodsMaxUsbC,
            0x202d => Self::AirPodsMax2,
            _ => return None,
        })
    }

    /// Maps Apple's `A` number reported in the device information packet.
    pub fn from_model_number(number: &str) -> Option<Self> {
        Some(match number {
            "A1523" | "A1722" => Self::AirPods1,
            "A2032" | "A2031" => Self::AirPods2,
            "A2565" | "A2564" => Self::AirPods3,
            "A3053" | "A3050" | "A3054" => Self::AirPods4,
            "A3056" | "A3055" | "A3057" => Self::AirPods4Anc,
            "A2084" | "A2083" => Self::AirPodsPro,
            "A2931" | "A2699" | "A2698" => Self::AirPodsPro2,
            "A3047" | "A3048" | "A3049" => Self::AirPodsPro2UsbC,
            "A3063" | "A3064" | "A3065" => Self::AirPodsPro3,
            "A2096" => Self::AirPodsMax,
            "A3184" => Self::AirPodsMaxUsbC,
            "A3454" => Self::AirPodsMax2,
            _ => return None,
        })
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::AirPods1 => "AirPods",
            Self::AirPods2 => "AirPods 2",
            Self::AirPods3 => "AirPods 3",
            Self::AirPods4 => "AirPods 4",
            Self::AirPods4Anc => "AirPods 4 ANC",
            Self::AirPodsPro => "AirPods Pro",
            Self::AirPodsPro2 => "AirPods Pro 2",
            Self::AirPodsPro2UsbC => "AirPods Pro 2 (USB-C)",
            Self::AirPodsPro3 => "AirPods Pro 3",
            Self::AirPodsMax => "AirPods Max",
            Self::AirPodsMaxUsbC => "AirPods Max (USB-C)",
            Self::AirPodsMax2 => "AirPods Max 2",
        }
    }

    pub const fn capabilities(self) -> Capabilities {
        const ANC: &[ListeningMode] = &[
            ListeningMode::Off,
            ListeningMode::NoiseCancelling,
            ListeningMode::Transparency,
        ];
        match self {
            Self::AirPods1 | Self::AirPods2 => Capabilities {
                listening_modes: &[],
                adaptive_noise_level: false,
                conversation_awareness: false,
                ear_detection: true,
                one_bud_anc: false,
                microphone_selection: true,
                press_timing: false,
                chime_volume: false,
                volume_swipe: false,
                personalized_volume: false,
                earbuds: true,
            },
            // Stem force sensors, but no noise control.
            Self::AirPods3 => Capabilities {
                press_timing: true,
                ..Self::AirPods1.capabilities()
            },
            Self::AirPods4 => Capabilities {
                press_timing: true,
                personalized_volume: true,
                ..Self::AirPods1.capabilities()
            },
            Self::AirPods4Anc => Capabilities {
                volume_swipe: false,
                ..Capabilities::FULL
            },
            Self::AirPodsPro2 | Self::AirPodsPro2UsbC | Self::AirPodsPro3 => Capabilities::FULL,
            Self::AirPodsPro => Capabilities {
                listening_modes: ANC,
                adaptive_noise_level: false,
                conversation_awareness: false,
                volume_swipe: false,
                personalized_volume: false,
                ..Capabilities::FULL
            },
            // AirPods Max: ANC and Transparency only, no in-ear sensors or
            // per-bud microphones.
            Self::AirPodsMax | Self::AirPodsMaxUsbC | Self::AirPodsMax2 => Capabilities {
                listening_modes: ANC,
                adaptive_noise_level: false,
                conversation_awareness: false,
                ear_detection: false,
                one_bud_anc: false,
                microphone_selection: false,
                press_timing: false,
                chime_volume: false,
                volume_swipe: false,
                personalized_volume: true,
                earbuds: false,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capabilities {
    pub listening_modes: &'static [ListeningMode],
    pub adaptive_noise_level: bool,
    pub conversation_awareness: bool,
    pub ear_detection: bool,
    pub one_bud_anc: bool,
    pub microphone_selection: bool,
    /// Press speed and press-and-hold duration.
    pub press_timing: bool,
    pub chime_volume: bool,
    /// Swiping the stem to change volume, and the pause between swipes.
    pub volume_swipe: bool,
    pub personalized_volume: bool,
    /// Two earbuds and a charging case, as opposed to over-ear AirPods Max.
    pub earbuds: bool,
}

impl Capabilities {
    /// Everything this crate can control. Also used while the model is not
    /// yet known, so an unrecognised newer model keeps its controls.
    pub const FULL: Self = Self {
        listening_modes: &ListeningMode::ALL,
        adaptive_noise_level: true,
        conversation_awareness: true,
        ear_detection: true,
        one_bud_anc: true,
        microphone_selection: true,
        press_timing: true,
        chime_volume: true,
        volume_swipe: true,
        personalized_volume: true,
        earbuds: true,
    };

    pub fn mode_cycle(self) -> bool {
        self.listening_modes.len() >= 2
    }
}

/// Identification strings from the device information packet (opcode 0x1d),
/// which the accessory sends unprompted after the connection is set up.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeviceInfo {
    pub name: Option<String>,
    pub model_number: Option<String>,
    pub manufacturer: Option<String>,
    pub serial_number: Option<String>,
    pub firmware: Option<String>,
    pub left_serial_number: Option<String>,
    pub right_serial_number: Option<String>,
}

impl DeviceInfo {
    pub fn model(&self) -> Option<Model> {
        Model::from_model_number(self.model_number.as_deref()?)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Setting {
    Mode(ListeningMode),
    ConversationAwareness(bool),
    EarDetection(bool),
    OneBudNoiseCancelling(bool),
    Microphone(Microphone),
    ListeningModeCycle(ListeningModeCycle),
    /// 0 lets the most noise through, 100 the least.
    AdaptiveNoiseLevel(u8),
    PressSpeed(Timing),
    PressAndHoldDuration(Timing),
    /// 0 to 100.
    ChimeVolume(u8),
    VolumeSwipe(bool),
    VolumeSwipeInterval(Timing),
    PersonalizedVolume(bool),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Update {
    Battery(BatteryState),
    Wearing(WearingState),
    Mode(ListeningMode),
    ConversationAwareness(bool),
    EarDetection(bool),
    OneBudNoiseCancelling(bool),
    Microphone(Microphone),
    ListeningModeCycle(ListeningModeCycle),
    AdaptiveNoiseLevel(u8),
    PressSpeed(Timing),
    PressAndHoldDuration(Timing),
    ChimeVolume(u8),
    VolumeSwipe(bool),
    VolumeSwipeInterval(Timing),
    PersonalizedVolume(bool),
    Conversation(Conversation),
    DeviceInfo(DeviceInfo),
    Model(Model),
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
    let mut extra = 0x00;
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
        Setting::ListeningModeCycle(cycle) => (setting_id::LISTENING_MODE_CYCLE, cycle.bits()),
        Setting::AdaptiveNoiseLevel(level) => (setting_id::ADAPTIVE_NOISE_LEVEL, level.min(100)),
        Setting::PressSpeed(timing) => (setting_id::PRESS_SPEED, timing.to_wire()),
        Setting::PressAndHoldDuration(timing) => {
            (setting_id::PRESS_AND_HOLD_DURATION, timing.to_wire())
        }
        Setting::ChimeVolume(volume) => {
            extra = CHIME_VOLUME_SUFFIX;
            (setting_id::CHIME_VOLUME, volume.min(100))
        }
        Setting::VolumeSwipe(enabled) => (setting_id::VOLUME_SWIPE, switch_to_wire(enabled)),
        Setting::VolumeSwipeInterval(timing) => {
            (setting_id::VOLUME_SWIPE_INTERVAL, timing.to_wire())
        }
        Setting::PersonalizedVolume(enabled) => {
            (setting_id::PERSONALIZED_VOLUME, switch_to_wire(enabled))
        }
    };
    let mut packet = MAGIC.to_vec();
    packet.extend_from_slice(&[opcode::SETTINGS, 0x00, id, value, extra, 0x00, 0x00]);
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
        opcode::CONVERSATION => Ok(payload
            .last()
            .and_then(|&level| Conversation::from_level(level))
            .map(Update::Conversation)
            .into_iter()
            .collect()),
        opcode::DEVICE_INFO => Ok(parse_device_info(payload)
            .map(|info| {
                let model = info.model().map(Update::Model);
                std::iter::once(Update::DeviceInfo(info))
                    .chain(model)
                    .collect()
            })
            .unwrap_or_default()),
        _ => Ok(Vec::new()),
    }
}

/// Layout: five header bytes, then NUL-terminated strings in this order:
/// name, model number, manufacturer, serial number, firmware, firmware again,
/// hardware revision, updater identifier, left serial, right serial, and
/// opaque trailing data.
fn parse_device_info(payload: &[u8]) -> Option<DeviceInfo> {
    let mut fields = payload.get(5..)?.split(|&byte| byte == 0).map(|field| {
        std::str::from_utf8(field)
            .ok()
            .map(str::trim)
            .filter(|text| !text.is_empty() && !text.chars().any(char::is_control))
            .map(str::to_owned)
    });
    let mut next = || fields.next().flatten();
    let info = DeviceInfo {
        name: next(),
        model_number: next(),
        manufacturer: next(),
        serial_number: next(),
        firmware: next(),
        ..DeviceInfo::default()
    };
    // Skip the repeated firmware, hardware revision and updater identifier.
    for _ in 0..3 {
        next();
    }
    let info = DeviceInfo {
        left_serial_number: next(),
        right_serial_number: next(),
        ..info
    };
    (info.name.is_some() || info.model_number.is_some()).then_some(info)
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
        setting_id::LISTENING_MODE_CYCLE => Some(Update::ListeningModeCycle(
            ListeningModeCycle::from_bits(value),
        )),
        setting_id::ADAPTIVE_NOISE_LEVEL => {
            (value <= 100).then_some(Update::AdaptiveNoiseLevel(value))
        }
        setting_id::PRESS_SPEED => Timing::from_wire(value).map(Update::PressSpeed),
        setting_id::PRESS_AND_HOLD_DURATION => {
            Timing::from_wire(value).map(Update::PressAndHoldDuration)
        }
        setting_id::CHIME_VOLUME => (value <= 100).then_some(Update::ChimeVolume(value)),
        setting_id::VOLUME_SWIPE => switch_from_wire(value).map(Update::VolumeSwipe),
        setting_id::VOLUME_SWIPE_INTERVAL => {
            Timing::from_wire(value).map(Update::VolumeSwipeInterval)
        }
        setting_id::PERSONALIZED_VOLUME => switch_from_wire(value).map(Update::PersonalizedVolume),
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
    fn decodes_device_information() {
        let mut packet = vec![
            0x04, 0x00, 0x04, 0x00, 0x1d, 0x00, 0x02, 0xd5, 0x00, 0x04, 0x00,
        ];
        for field in [
            "Test AirPods",
            "A3048",
            "Apple Inc.",
            "SERIAL00001",
            "61.1868040002000000.2713",
            "61.1868040002000000.2713",
            "1.0.0",
            "com.apple.accessory.updater.app.71",
            "LEFT0000001",
            "RIGHT000001",
            "6357536",
        ] {
            packet.extend_from_slice(field.as_bytes());
            packet.push(0);
        }
        packet.extend_from_slice(&[0x89, 0x31, 0x2a, 0x65, 0x67, 0xa5, 0x00]);
        let info = DeviceInfo {
            name: Some("Test AirPods".into()),
            model_number: Some("A3048".into()),
            manufacturer: Some("Apple Inc.".into()),
            serial_number: Some("SERIAL00001".into()),
            firmware: Some("61.1868040002000000.2713".into()),
            left_serial_number: Some("LEFT0000001".into()),
            right_serial_number: Some("RIGHT000001".into()),
        };
        assert_eq!(
            parse(&packet),
            Ok(vec![
                Update::DeviceInfo(info),
                Update::Model(Model::AirPodsPro2UsbC),
            ])
        );
    }

    #[test]
    fn device_information_without_known_model_still_decodes() {
        let packet = b"\x04\x00\x04\x00\x1d\x00\x02\x40\x00\x04\x00Future Pods\0A9999\0";
        let Ok(updates) = parse(packet) else {
            panic!("device information should parse");
        };
        assert!(matches!(
            updates.as_slice(),
            [Update::DeviceInfo(DeviceInfo { model_number: Some(number), .. })] if number == "A9999"
        ));
        assert_eq!(parse(&[0x04, 0, 0x04, 0, 0x1d, 0, 2, 0x40]), Ok(Vec::new()));
    }

    #[test]
    fn maps_product_ids_and_model_numbers_to_the_same_model() {
        assert_eq!(Model::from_product_id(0x2024), Some(Model::AirPodsPro2UsbC));
        assert_eq!(
            Model::from_model_number("A3047"),
            Some(Model::AirPodsPro2UsbC)
        );
        assert_eq!(Model::from_product_id(0x200a), Some(Model::AirPodsMax));
        assert_eq!(Model::from_model_number("A2096"), Some(Model::AirPodsMax));
        // Beats share Apple's vendor ID; their product IDs must not match.
        assert_eq!(Model::from_product_id(0x2025), None);
        assert_eq!(
            Model::from_model_number("A2968"),
            None,
            "a case is not a model"
        );
    }

    #[test]
    fn capabilities_follow_the_model() {
        let max = Model::AirPodsMax.capabilities();
        assert!(!max.earbuds && !max.ear_detection && !max.adaptive_noise_level);
        assert!(max.mode_cycle());
        let basic = Model::AirPods3.capabilities();
        assert!(basic.listening_modes.is_empty() && !basic.mode_cycle());
        assert_eq!(Model::AirPodsPro2.capabilities(), Capabilities::FULL);
    }

    #[test]
    fn round_trips_mode_cycle_and_adaptive_level() {
        let cycle = ListeningModeCycle::from_modes([
            ListeningMode::NoiseCancelling,
            ListeningMode::Transparency,
            ListeningMode::Adaptive,
        ]);
        assert_eq!(cycle.bits(), 0x0e);
        assert!(!cycle.contains(ListeningMode::Off));
        assert_eq!(cycle.with(ListeningMode::Adaptive, false).len(), 2);
        let packet = encode_setting(Setting::ListeningModeCycle(cycle));
        assert_eq!(
            packet,
            [MAGIC.as_slice(), &[0x09, 0x00, 0x1a, 0x0e, 0, 0, 0]].concat()
        );
        assert_eq!(parse(&packet), Ok(vec![Update::ListeningModeCycle(cycle)]));

        let packet = encode_setting(Setting::AdaptiveNoiseLevel(50));
        assert_eq!(
            packet,
            [MAGIC.as_slice(), &[0x09, 0x00, 0x2e, 50, 0, 0, 0]].concat()
        );
        assert_eq!(parse(&packet), Ok(vec![Update::AdaptiveNoiseLevel(50)]));
        assert_eq!(
            parse(&[0x04, 0, 0x04, 0, 0x09, 0, 0x2e, 101]),
            Ok(Vec::new())
        );
    }

    #[test]
    fn decodes_captured_accessibility_and_volume_settings() {
        // From an AirPods Pro 2 (USB-C) settings dump.
        let captured: [(&[u8], Update); 6] = [
            (&[0x17, 0, 0, 0, 0], Update::PressSpeed(Timing::Default)),
            (
                &[0x18, 0, 0, 0, 0],
                Update::PressAndHoldDuration(Timing::Default),
            ),
            (&[0x1f, 0x50, 0x50, 0, 0], Update::ChimeVolume(80)),
            (&[0x25, 1, 0, 0, 0], Update::VolumeSwipe(true)),
            (
                &[0x23, 0, 0, 0, 0],
                Update::VolumeSwipeInterval(Timing::Default),
            ),
            (&[0x26, 2, 0, 0, 0], Update::PersonalizedVolume(false)),
        ];
        for (payload, update) in captured {
            let packet = [MAGIC.as_slice(), &[0x09, 0x00], payload].concat();
            assert_eq!(parse(&packet), Ok(vec![update]));
        }
    }

    #[test]
    fn encodes_new_settings_in_the_captured_layout() {
        assert_eq!(
            encode_setting(Setting::ChimeVolume(80)),
            [MAGIC.as_slice(), &[0x09, 0x00, 0x1f, 0x50, 0x50, 0, 0]].concat()
        );
        assert_eq!(
            encode_setting(Setting::PressAndHoldDuration(Timing::Longest)),
            [MAGIC.as_slice(), &[0x09, 0x00, 0x18, 2, 0, 0, 0]].concat()
        );
        for setting in [
            Setting::PressSpeed(Timing::Longer),
            Setting::VolumeSwipe(false),
            Setting::VolumeSwipeInterval(Timing::Longest),
            Setting::PersonalizedVolume(true),
            Setting::ChimeVolume(35),
        ] {
            let parsed = parse(&encode_setting(setting)).unwrap();
            assert_eq!(parsed.len(), 1, "{setting:?} should read back");
        }
    }

    #[test]
    fn decodes_conversation_levels() {
        let level = |level| parse(&[0x04, 0, 0x04, 0, 0x4b, 0, 0x02, 0, 0x01, level]);
        assert_eq!(
            level(1),
            Ok(vec![Update::Conversation(Conversation::Speaking)])
        );
        assert_eq!(
            level(3),
            Ok(vec![Update::Conversation(Conversation::Speaking)])
        );
        assert_eq!(
            level(5),
            Ok(vec![Update::Conversation(Conversation::Easing(2))])
        );
        assert_eq!(
            level(9),
            Ok(vec![Update::Conversation(Conversation::Ended)])
        );
        assert_eq!(level(0), Ok(Vec::new()));
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
