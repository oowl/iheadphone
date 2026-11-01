//! WH-1000XM5 command payloads, adapted from `aecca/sound-connect` (MIT).

use crate::frame::{COMMAND, COMMAND_2};
use crate::{
    AutoOff, Battery, ConnectedDevice, EqPreset, Equalizer, Feature, ListeningMode, NoiseButton,
    SpeakToChatEnd, Update, VoiceSensitivity,
};

const NOISE_TYPE: u8 = 0x17;
const SPEAK_TO_CHAT: u8 = 0x0c;
const PAUSE_WHEN_TAKEN_OFF: u8 = 0x01;
const AUTO_OFF_WEARING: u8 = 0x05;
const NOISE_BUTTON: u8 = 0x30;
const PLAYBACK_DEVICE: u8 = 0x01;
const EQ_OFFSET: i8 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeviceList {
    Classic,
    WithClass,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChatOptions {
    pub sensitivity: VoiceSensitivity,
    pub end: SpeakToChatEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NoiseControl {
    pub mode: ListeningMode,
    pub focus_on_voice: bool,
    pub ambient_level: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Request {
    Init,
    FirmwareVersion,
    Battery,
    Codec,
    NoiseControl,
    SetNoiseControl(NoiseControl),
    SpeakToChat,
    SpeakToChatStatus,
    SetSpeakToChat(bool),
    SpeakToChatOptions,
    SetSpeakToChatOptions(ChatOptions),
    Equalizer,
    SetEqualizerPreset(EqPreset),
    SetEqualizerBands([i8; 6]),
    Dsee,
    SetDsee(bool),
    PauseWhenTakenOff,
    SetPauseWhenTakenOff(bool),
    AutoOff,
    SetAutoOff(AutoOff),
    NoiseButton,
    SetNoiseButton(NoiseButton),
    EnableOperationLogs,
    Table2Functions,
    WearingStatus,
    ConnectedDevices(DeviceList),
    PlayFrom([u8; 17]),
    PowerOff,
}

impl Request {
    pub fn kind(self) -> u8 {
        match self {
            Self::Table2Functions
            | Self::WearingStatus
            | Self::ConnectedDevices(_)
            | Self::PlayFrom(_) => COMMAND_2,
            _ => COMMAND,
        }
    }

    pub fn payload(self) -> Vec<u8> {
        match self {
            Self::Init => vec![0x00, 0x00],
            Self::FirmwareVersion => vec![0x04, 0x02],
            Self::Battery => vec![0x22, 0x00],
            Self::Codec => vec![0x12, 0x02],
            Self::NoiseControl => vec![0x66, NOISE_TYPE],
            Self::SetNoiseControl(noise) => noise_payload(noise),
            Self::SpeakToChat => vec![0xf6, SPEAK_TO_CHAT],
            Self::SpeakToChatStatus => vec![0xf2, SPEAK_TO_CHAT],
            Self::SetSpeakToChat(on) => vec![0xf8, SPEAK_TO_CHAT, u8::from(!on), 0x01],
            Self::SpeakToChatOptions => vec![0xfa, SPEAK_TO_CHAT],
            Self::SetSpeakToChatOptions(options) => vec![
                0xfc,
                SPEAK_TO_CHAT,
                sensitivity_code(options.sensitivity),
                chat_end_code(options.end),
            ],
            Self::Equalizer => vec![0x56, 0x00],
            Self::SetEqualizerPreset(preset) => vec![0x58, 0x00, preset_code(preset), 0x00],
            Self::SetEqualizerBands(bands) => {
                let mut payload = vec![0x58, 0x00, preset_code(EqPreset::Manual), 0x06];
                payload.extend(bands.map(eq_byte));
                payload
            }
            Self::Dsee => vec![0xe6, 0x01],
            Self::SetDsee(on) => vec![0xe8, 0x01, u8::from(on)],
            Self::PauseWhenTakenOff => vec![0xf6, PAUSE_WHEN_TAKEN_OFF],
            Self::SetPauseWhenTakenOff(on) => {
                vec![0xf8, PAUSE_WHEN_TAKEN_OFF, u8::from(!on)]
            }
            Self::AutoOff => vec![0x26, AUTO_OFF_WEARING],
            Self::SetAutoOff(AutoOff::WhenTakenOff) => vec![0x28, AUTO_OFF_WEARING, 0x10, 0x00],
            Self::SetAutoOff(AutoOff::Never) => vec![0x28, AUTO_OFF_WEARING, 0x11, 0x00],
            Self::SetAutoOff(AutoOff::Minutes(_)) => unreachable!("timer is not offered for XM5"),
            Self::NoiseButton => vec![0x66, NOISE_BUTTON],
            Self::SetNoiseButton(choice) => vec![0x68, NOISE_BUTTON, noise_button_code(choice)],
            Self::EnableOperationLogs => vec![0xc4, 0x01, 0x00],
            Self::Table2Functions => vec![0x06, 0x00],
            Self::WearingStatus => vec![0xf2, 0x00],
            Self::ConnectedDevices(DeviceList::Classic) => vec![0x36, 0x00],
            Self::ConnectedDevices(DeviceList::WithClass) => vec![0x36, 0x02],
            Self::PlayFrom(address) => [&[0x3c, PLAYBACK_DEVICE][..], &address].concat(),
            Self::PowerOff => vec![0x24, 0x03, 0x01],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Reply {
    Ready { table2: bool },
    Firmware(String),
    Battery(Battery),
    Codec(String),
    NoiseControl(NoiseControl),
    SpeakToChat(bool),
    SpeakToChatActive(bool),
    SpeakToChatOptions(ChatOptions),
    Equalizer(Equalizer),
    Dsee(bool),
    PauseWhenTakenOff(bool),
    AutoOff(AutoOff),
    NoiseButton(NoiseButton),
    Table2Functions(Vec<u8>),
    WearingStatus(crate::WearingStatus),
    ConnectedDevices(Vec<ConnectedDevice>),
    PlayedFrom,
}

impl Reply {
    pub fn updates(&self) -> Vec<Update> {
        match self {
            Self::Ready { .. } | Self::Table2Functions(_) | Self::PlayedFrom => Vec::new(),
            Self::Firmware(value) => vec![Update::Firmware(value.clone())],
            Self::Battery(value) => vec![Update::Battery(*value)],
            Self::Codec(value) => vec![Update::Codec(value.clone())],
            Self::NoiseControl(noise) => vec![
                Update::Mode(noise.mode),
                Update::AmbientLevel(noise.ambient_level),
                Update::FocusOnVoice(noise.focus_on_voice),
            ],
            Self::SpeakToChat(on) => vec![Update::Feature(Feature::SpeakToChat, *on)],
            Self::SpeakToChatActive(active) => vec![Update::SpeakToChatActive(*active)],
            Self::SpeakToChatOptions(options) => vec![
                Update::VoiceSensitivity(options.sensitivity),
                Update::SpeakToChatEnd(options.end),
            ],
            Self::Equalizer(value) => vec![Update::Equalizer(*value)],
            Self::Dsee(on) => vec![Update::Feature(Feature::Dsee, *on)],
            Self::PauseWhenTakenOff(on) => {
                vec![Update::Feature(Feature::PauseWhenTakenOff, *on)]
            }
            Self::AutoOff(value) => vec![Update::AutoOff(*value)],
            Self::NoiseButton(value) => vec![Update::NoiseButton(*value)],
            Self::WearingStatus(value) => vec![Update::WearingStatus(*value)],
            Self::ConnectedDevices(value) => vec![Update::ConnectedDevices(value.clone())],
        }
    }
}

pub(crate) fn parse(payload: &[u8]) -> Option<Reply> {
    match *payload.first()? {
        0x01 if payload.len() == 8 => Some(Reply::Ready {
            table2: payload[7] == 0,
        }),
        0x05 if payload.len() > 3 && payload[1] == 0x02 => Some(Reply::Firmware(
            String::from_utf8_lossy(&payload[3..]).into_owned(),
        )),
        0x23 | 0x25 if payload.len() >= 4 && payload[1] == 0x00 => {
            let (level, charging) = (payload[2], payload[3]);
            (level <= 100 && charging <= 1).then_some(Reply::Battery(Battery {
                level,
                charging: charging == 1,
            }))
        }
        0x13 | 0x15 if payload.len() >= 3 && payload[1] == 0x02 => {
            Some(Reply::Codec(codec_name(payload[2])))
        }
        0x67 | 0x69
            if payload.len() == 7
                && payload[1] == NOISE_TYPE
                && payload[3..=5].iter().all(|value| *value <= 1)
                && (1..=20).contains(&payload[6]) =>
        {
            let mode = match (payload[3], payload[4]) {
                (0, _) => ListeningMode::Off,
                (_, 0) => ListeningMode::NoiseCancelling,
                _ => ListeningMode::Ambient,
            };
            Some(Reply::NoiseControl(NoiseControl {
                mode,
                focus_on_voice: payload[5] == 1,
                ambient_level: payload[6],
            }))
        }
        0xf7 | 0xf9 if payload.len() >= 3 && payload[1] == SPEAK_TO_CHAT && payload[2] <= 1 => {
            Some(Reply::SpeakToChat(payload[2] == 0))
        }
        0xf3 | 0xf5
            if payload.len() == 4
                && payload[1] == SPEAK_TO_CHAT
                && payload[2] <= 1
                && payload[3] <= 1 =>
        {
            Some(Reply::SpeakToChatActive(payload[2] == 0 && payload[3] == 1))
        }
        0xfb | 0xfd if payload.len() == 4 && payload[1] == SPEAK_TO_CHAT => {
            Some(Reply::SpeakToChatOptions(ChatOptions {
                sensitivity: sensitivity_from_code(payload[2])?,
                end: chat_end_from_code(payload[3])?,
            }))
        }
        0x57 | 0x59 if payload.len() == 10 && payload[1] == 0x00 && payload[3] == 6 => {
            let mut bands = [0i8; 6];
            for (value, byte) in bands.iter_mut().zip(&payload[4..10]) {
                *value = eq_value(*byte);
            }
            Some(Reply::Equalizer(Equalizer {
                preset: preset_from_code(payload[2]),
                bands,
            }))
        }
        0xe7 | 0xe9 if payload.len() >= 3 && payload[1] == 0x01 && payload[2] <= 1 => {
            Some(Reply::Dsee(payload[2] == 1))
        }
        0xf7 | 0xf9
            if payload.len() == 3 && payload[1] == PAUSE_WHEN_TAKEN_OFF && payload[2] <= 1 =>
        {
            Some(Reply::PauseWhenTakenOff(payload[2] == 0))
        }
        0x27 | 0x29 if payload.len() >= 3 && payload[1] == AUTO_OFF_WEARING => {
            auto_off_from_code(payload[2]).map(Reply::AutoOff)
        }
        0x67 | 0x69 if payload.len() == 3 && payload[1] == NOISE_BUTTON => {
            noise_button_from_code(payload[2]).map(Reply::NoiseButton)
        }
        0xc9 if payload.len() >= 3 && payload[1] == 0x01 => {
            let length = usize::from(payload[2]);
            let operation = payload.get(3..3 + length)?;
            match operation {
                b"unitRemove" => Some(Reply::WearingStatus(crate::WearingStatus::BothRemoved)),
                b"unitWear" => Some(Reply::WearingStatus(crate::WearingStatus::Normal)),
                _ => None,
            }
        }
        _ => None,
    }
}

pub(crate) fn parse_table_2(payload: &[u8]) -> Option<Reply> {
    match *payload.first()? {
        0x07 if payload.len() >= 3 && payload[1] == 0 => {
            let count = usize::from(payload[2]);
            let (pairs, _) = payload[3..].as_chunks::<2>();
            Some(Reply::Table2Functions(
                pairs.iter().take(count).map(|pair| pair[0]).collect(),
            ))
        }
        0xf3 | 0xf5 if payload.len() == 3 && payload[1] == 0 => {
            wearing_status_from_code(payload[2]).map(Reply::WearingStatus)
        }
        0x37 | 0x39 if payload.len() >= 3 && [0x00, 0x02].contains(&payload[1]) => {
            connected_devices(&payload[2..], payload[1] == 0x02).map(Reply::ConnectedDevices)
        }
        0x3d if payload.len() >= 2 && payload[1] == PLAYBACK_DEVICE => Some(Reply::PlayedFrom),
        _ => None,
    }
}

fn wearing_status_from_code(code: u8) -> Option<crate::WearingStatus> {
    use crate::WearingStatus;

    match code {
        0x00 => Some(WearingStatus::Normal),
        0x01 => Some(WearingStatus::Illegal),
        0x02 => Some(WearingStatus::LeftRemoved),
        0x03 => Some(WearingStatus::RightRemoved),
        0x04 => Some(WearingStatus::BothRemoved),
        _ => None,
    }
}

pub(crate) fn device_list(functions: &[u8]) -> Option<DeviceList> {
    if functions
        .iter()
        .any(|function| [0x32, 0x33].contains(function))
    {
        Some(DeviceList::WithClass)
    } else if functions.contains(&0x30) {
        Some(DeviceList::Classic)
    } else {
        None
    }
}

fn noise_payload(noise: NoiseControl) -> Vec<u8> {
    vec![
        0x68,
        NOISE_TYPE,
        0x01,
        u8::from(noise.mode != ListeningMode::Off),
        u8::from(noise.mode == ListeningMode::Ambient),
        u8::from(noise.focus_on_voice),
        noise.ambient_level.clamp(1, 20),
    ]
}

fn eq_byte(value: i8) -> u8 {
    (value.clamp(-10, 10) + EQ_OFFSET) as u8
}

fn eq_value(byte: u8) -> i8 {
    byte.min(20) as i8 - EQ_OFFSET
}

fn preset_code(preset: EqPreset) -> u8 {
    match preset {
        EqPreset::Off => 0x00,
        EqPreset::Bright => 0x10,
        EqPreset::Excited => 0x11,
        EqPreset::Mellow => 0x12,
        EqPreset::Relaxed => 0x13,
        EqPreset::Vocal => 0x14,
        EqPreset::TrebleBoost => 0x15,
        EqPreset::BassBoost => 0x16,
        EqPreset::Speech => 0x17,
        EqPreset::Manual => 0xa0,
        EqPreset::Custom1 => 0xa1,
        EqPreset::Custom2 => 0xa2,
        EqPreset::Unknown(code) => code,
    }
}

fn preset_from_code(code: u8) -> EqPreset {
    EqPreset::CHOICES
        .into_iter()
        .find(|preset| preset_code(*preset) == code)
        .unwrap_or(EqPreset::Unknown(code))
}

fn sensitivity_code(value: VoiceSensitivity) -> u8 {
    match value {
        VoiceSensitivity::Auto => 0,
        VoiceSensitivity::High => 1,
        VoiceSensitivity::Low => 2,
    }
}

fn sensitivity_from_code(code: u8) -> Option<VoiceSensitivity> {
    VoiceSensitivity::ALL
        .into_iter()
        .find(|value| sensitivity_code(*value) == code)
}

fn chat_end_code(value: SpeakToChatEnd) -> u8 {
    match value {
        SpeakToChatEnd::Short => 0,
        SpeakToChatEnd::Standard => 1,
        SpeakToChatEnd::Long => 2,
        SpeakToChatEnd::Never => 3,
    }
}

fn chat_end_from_code(code: u8) -> Option<SpeakToChatEnd> {
    SpeakToChatEnd::ALL
        .into_iter()
        .find(|value| chat_end_code(*value) == code)
}

fn noise_button_code(value: NoiseButton) -> u8 {
    match value {
        NoiseButton::NoiseCancellingAmbientOff => 1,
        NoiseButton::NoiseCancellingAmbient => 2,
        NoiseButton::NoiseCancellingOff => 3,
        NoiseButton::AmbientOff => 4,
    }
}

fn noise_button_from_code(code: u8) -> Option<NoiseButton> {
    NoiseButton::ALL
        .into_iter()
        .find(|value| noise_button_code(*value) == code)
}

fn auto_off_from_code(code: u8) -> Option<AutoOff> {
    match code {
        0x10 => Some(AutoOff::WhenTakenOff),
        0x11 => Some(AutoOff::Never),
        0x00 => Some(AutoOff::Minutes(5)),
        0x01 => Some(AutoOff::Minutes(30)),
        0x02 => Some(AutoOff::Minutes(60)),
        0x03 => Some(AutoOff::Minutes(180)),
        0x04 => Some(AutoOff::Minutes(15)),
        _ => None,
    }
}

fn codec_name(code: u8) -> String {
    match code {
        0x01 => "SBC".into(),
        0x02 => "AAC".into(),
        0x10 => "LDAC".into(),
        0x20 => "aptX".into(),
        0x21 => "aptX HD".into(),
        other => format!("0x{other:02x}"),
    }
}

fn connected_devices(payload: &[u8], with_class: bool) -> Option<Vec<ConnectedDevice>> {
    let (&count, mut rest) = payload.split_first()?;
    let mut paired = Vec::new();
    for _ in 0..count {
        let (address, after) = rest.split_at_checked(17)?;
        let (&slot, after) = after.split_first()?;
        let after = if with_class { after.get(3..)? } else { after };
        let (&name_len, after) = after.split_first()?;
        let (name, after) = after.split_at_checked(name_len.into())?;
        paired.push((address, slot, name));
        rest = after;
    }
    let &playing = rest.first()?;
    Some(
        paired
            .into_iter()
            .filter(|(_, slot, _)| *slot != 0)
            .map(|(address, slot, name)| ConnectedDevice {
                id: String::from_utf8_lossy(address).into_owned(),
                name: String::from_utf8_lossy(name).into_owned(),
                playing: slot == playing,
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_encodes_xm5_features() {
        assert_eq!(
            parse(&[0x05, 0x02, 5, b'2', b'.', b'4']),
            Some(Reply::Firmware("2.4".into()))
        );
        assert_eq!(
            parse(&[0x13, 0x02, 0x10]),
            Some(Reply::Codec("LDAC".into()))
        );
        assert_eq!(Request::SetDsee(true).payload(), [0xe8, 1, 1]);
        assert_eq!(Request::SetPauseWhenTakenOff(true).payload(), [0xf8, 1, 0]);
        assert_eq!(Request::SpeakToChatStatus.payload(), [0xf2, 0x0c]);
        assert_eq!(Request::EnableOperationLogs.payload(), [0xc4, 0x01, 0x00]);
        assert_eq!(Request::WearingStatus.payload(), [0xf2, 0x00]);
        assert_eq!(Request::PowerOff.payload(), [0x24, 3, 1]);
        assert_eq!(
            parse(&[0xf5, 0x0c, 0x00, 0x01]),
            Some(Reply::SpeakToChatActive(true))
        );
        assert_eq!(
            parse(&[0xf5, 0x0c, 0x00, 0x00]),
            Some(Reply::SpeakToChatActive(false))
        );
        assert_eq!(
            parse_table_2(&[0xf5, 0x00, 0x00]),
            Some(Reply::WearingStatus(crate::WearingStatus::Normal))
        );
        assert_eq!(
            parse_table_2(&[0xf5, 0x00, 0x04]),
            Some(Reply::WearingStatus(crate::WearingStatus::BothRemoved))
        );
        assert_eq!(
            parse(&[
                0xc9, 0x01, 0x0a, b'u', b'n', b'i', b't', b'R', b'e', b'm', b'o', b'v', b'e', 0x00,
                0x00,
            ]),
            Some(Reply::WearingStatus(crate::WearingStatus::BothRemoved))
        );
        assert_eq!(
            parse(&[
                0xc9, 0x01, 0x08, b'u', b'n', b'i', b't', b'W', b'e', b'a', b'r', 0x00, 0x00,
            ]),
            Some(Reply::WearingStatus(crate::WearingStatus::Normal))
        );
    }

    #[test]
    fn equalizer_round_trips_and_clamps() {
        let bands = [3, -10, -1, 0, 1, 12];
        assert_eq!(
            Request::SetEqualizerBands(bands).payload(),
            [0x58, 0, 0xa0, 6, 13, 0, 9, 10, 11, 20]
        );
        let reply = parse(&[0x59, 0, 0xa0, 6, 13, 0, 9, 10, 11, 20]);
        assert_eq!(
            reply,
            Some(Reply::Equalizer(Equalizer {
                preset: EqPreset::Manual,
                bands: [3, -10, -1, 0, 1, 10],
            }))
        );
    }

    #[test]
    fn rejects_invalid_values() {
        assert_eq!(parse(&[0x23, 0, 101, 0]), None);
        assert_eq!(parse(&[0x67, 0x17, 1, 2, 1, 0, 10]), None);
        assert_eq!(parse(&[0xf7, 0x0c, 2]), None);
    }
}
