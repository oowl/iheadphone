//! Reviewed, I/O-free Sony WH-1000XM5 control protocol.
//!
//! Wire layouts and captured test messages were adapted from
//! `aecca/sound-connect` (MIT); see `LICENSE.sound-connect-MIT`.

mod command;
mod frame;
mod queue;
mod session;

use std::time::Instant;

use command::{ChatOptions, DeviceList, NoiseControl, Reply, Request, device_list};
pub use frame::FrameError;
use session::Session;

pub const SERVICE_UUID: &str = "956C7B26-D49A-4BA8-B03F-B17D393CB6E2";
pub type Frame = Vec<u8>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListeningMode {
    Off,
    NoiseCancelling,
    Ambient,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Feature {
    Dsee,
    SpeakToChat,
    PauseWhenTakenOff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Battery {
    pub level: u8,
    pub charging: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EqPreset {
    Off,
    Bright,
    Excited,
    Mellow,
    Relaxed,
    Vocal,
    TrebleBoost,
    BassBoost,
    Speech,
    Manual,
    Custom1,
    Custom2,
    Unknown(u8),
}

impl EqPreset {
    pub const CHOICES: [Self; 12] = [
        Self::Off,
        Self::Bright,
        Self::Excited,
        Self::Mellow,
        Self::Relaxed,
        Self::Vocal,
        Self::TrebleBoost,
        Self::BassBoost,
        Self::Speech,
        Self::Manual,
        Self::Custom1,
        Self::Custom2,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Bright => "Bright",
            Self::Excited => "Excited",
            Self::Mellow => "Mellow",
            Self::Relaxed => "Relaxed",
            Self::Vocal => "Vocal",
            Self::TrebleBoost => "Treble Boost",
            Self::BassBoost => "Bass Boost",
            Self::Speech => "Speech",
            Self::Manual => "Manual",
            Self::Custom1 => "Custom 1",
            Self::Custom2 => "Custom 2",
            Self::Unknown(_) => "Unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Equalizer {
    pub preset: EqPreset,
    /// Clear Bass, 400 Hz, 1 kHz, 2.5 kHz, 6.3 kHz and 16 kHz; each -10..=10.
    pub bands: [i8; 6],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AutoOff {
    WhenTakenOff,
    Never,
    Minutes(u16),
}

impl AutoOff {
    pub const XM5_CHOICES: [Self; 2] = [Self::WhenTakenOff, Self::Never];

    pub fn name(self) -> String {
        match self {
            Self::WhenTakenOff => "When taken off".into(),
            Self::Never => "Never".into(),
            Self::Minutes(minutes) => format!("After {minutes} minutes"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NoiseButton {
    NoiseCancellingAmbientOff,
    NoiseCancellingAmbient,
    NoiseCancellingOff,
    AmbientOff,
}

impl NoiseButton {
    pub const ALL: [Self; 4] = [
        Self::NoiseCancellingAmbientOff,
        Self::NoiseCancellingAmbient,
        Self::NoiseCancellingOff,
        Self::AmbientOff,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::NoiseCancellingAmbientOff => "ANC · Ambient · Off",
            Self::NoiseCancellingAmbient => "ANC · Ambient",
            Self::NoiseCancellingOff => "ANC · Off",
            Self::AmbientOff => "Ambient · Off",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VoiceSensitivity {
    Auto,
    High,
    Low,
}

impl VoiceSensitivity {
    pub const ALL: [Self; 3] = [Self::Auto, Self::High, Self::Low];
    pub const fn name(self) -> &'static str {
        match self {
            Self::Auto => "Automatic",
            Self::High => "High",
            Self::Low => "Low",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpeakToChatEnd {
    Short,
    Standard,
    Long,
    Never,
}

impl SpeakToChatEnd {
    pub const ALL: [Self; 4] = [Self::Short, Self::Standard, Self::Long, Self::Never];
    pub const fn name(self) -> &'static str {
        match self {
            Self::Short => "Short",
            Self::Standard => "Standard",
            Self::Long => "Long",
            Self::Never => "Never",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectedDevice {
    pub id: String,
    pub name: String,
    pub playing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Setting {
    Mode(ListeningMode),
    AmbientLevel(u8),
    FocusOnVoice(bool),
    Feature(Feature, bool),
    EqualizerPreset(EqPreset),
    EqualizerBands([i8; 6]),
    AutoOff(AutoOff),
    NoiseButton(NoiseButton),
    VoiceSensitivity(VoiceSensitivity),
    SpeakToChatEnd(SpeakToChatEnd),
    PlayFrom(String),
    PowerOff,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    Firmware(String),
    Codec(String),
    Battery(Battery),
    Mode(ListeningMode),
    AmbientLevel(u8),
    FocusOnVoice(bool),
    Feature(Feature, bool),
    Equalizer(Equalizer),
    AutoOff(AutoOff),
    NoiseButton(NoiseButton),
    VoiceSensitivity(VoiceSensitivity),
    SpeakToChatEnd(SpeakToChatEnd),
    SpeakToChatActive(bool),
    WearingStatus(WearingStatus),
    ConnectedDevices(Vec<ConnectedDevice>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WearingStatus {
    Normal,
    Illegal,
    LeftRemoved,
    RightRemoved,
    BothRemoved,
}

impl WearingStatus {
    pub fn is_taken_off(self) -> bool {
        matches!(
            self,
            Self::LeftRemoved | Self::RightRemoved | Self::BothRemoved
        )
    }
}

#[derive(Debug, Default)]
pub struct Output {
    pub writes: Vec<Frame>,
    pub updates: Vec<Update>,
    pub errors: Vec<FrameError>,
}

#[derive(Default)]
pub struct SonyXm5Driver {
    session: Session,
    noise: Option<NoiseControl>,
    chat: Option<ChatOptions>,
    equalizer: Option<Equalizer>,
    devices: Option<DeviceList>,
}

impl SonyXm5Driver {
    pub fn start(&mut self, now: Instant) -> Output {
        let requests = [
            Request::Init,
            Request::FirmwareVersion,
            Request::Battery,
            Request::Codec,
            Request::NoiseControl,
            Request::SpeakToChat,
            Request::SpeakToChatStatus,
            Request::SpeakToChatOptions,
            Request::Equalizer,
            Request::Dsee,
            Request::PauseWhenTakenOff,
            Request::AutoOff,
            Request::NoiseButton,
            Request::EnableOperationLogs,
        ];
        Output {
            writes: self.request_all(requests, now),
            ..Output::default()
        }
    }

    pub fn receive(&mut self, bytes: &[u8], now: Instant) -> Output {
        let received = self.session.receive(bytes, now);
        let mut output = Output {
            writes: received.writes,
            errors: received.errors,
            ..Output::default()
        };
        for reply in received.replies {
            match reply {
                Reply::Ready { table2: true } => {
                    output
                        .writes
                        .extend(self.session.request(Request::Table2Functions, now));
                    output
                        .writes
                        .extend(self.session.request(Request::WearingStatus, now));
                }
                Reply::Table2Functions(functions) => {
                    self.devices = device_list(&functions);
                    if let Some(layout) = self.devices {
                        output
                            .writes
                            .extend(self.session.request(Request::ConnectedDevices(layout), now));
                    }
                }
                Reply::PlayedFrom => {
                    if let Some(layout) = self.devices {
                        output
                            .writes
                            .extend(self.session.request(Request::ConnectedDevices(layout), now));
                    }
                }
                Reply::NoiseControl(noise) => {
                    self.noise = Some(noise);
                    output.updates.extend(reply.updates());
                }
                Reply::SpeakToChatOptions(options) => {
                    self.chat = Some(options);
                    output.updates.extend(reply.updates());
                }
                Reply::Equalizer(equalizer) => {
                    self.equalizer = Some(equalizer);
                    output.updates.extend(reply.updates());
                }
                _ => output.updates.extend(reply.updates()),
            }
        }
        output
    }

    pub fn set(&mut self, setting: Setting, now: Instant) -> Output {
        let request = match setting {
            Setting::Mode(mode) => self.change_noise(|noise| noise.mode = mode),
            Setting::AmbientLevel(level) => {
                self.change_noise(|noise| noise.ambient_level = level.clamp(1, 20))
            }
            Setting::FocusOnVoice(on) => self.change_noise(|noise| noise.focus_on_voice = on),
            Setting::Feature(Feature::SpeakToChat, on) => Some(Request::SetSpeakToChat(on)),
            Setting::Feature(Feature::Dsee, on) => Some(Request::SetDsee(on)),
            Setting::Feature(Feature::PauseWhenTakenOff, on) => {
                Some(Request::SetPauseWhenTakenOff(on))
            }
            Setting::EqualizerPreset(preset) => Some(Request::SetEqualizerPreset(preset)),
            Setting::EqualizerBands(bands) => {
                let bands = bands.map(|value| value.clamp(-10, 10));
                self.equalizer = Some(Equalizer {
                    preset: EqPreset::Manual,
                    bands,
                });
                Some(Request::SetEqualizerBands(bands))
            }
            Setting::AutoOff(choice) if AutoOff::XM5_CHOICES.contains(&choice) => {
                Some(Request::SetAutoOff(choice))
            }
            Setting::NoiseButton(choice) => Some(Request::SetNoiseButton(choice)),
            Setting::VoiceSensitivity(value) => self.change_chat(|chat| chat.sensitivity = value),
            Setting::SpeakToChatEnd(value) => self.change_chat(|chat| chat.end = value),
            Setting::PlayFrom(id) if self.devices.is_some() => {
                id.as_bytes().try_into().ok().map(Request::PlayFrom)
            }
            Setting::PowerOff => Some(Request::PowerOff),
            _ => None,
        };
        let Some(request) = request else {
            return Output::default();
        };
        Output {
            writes: self.session.request(request, now),
            ..Output::default()
        }
    }

    pub fn refresh(&mut self, now: Instant) -> Output {
        let mut requests = vec![Request::Battery];
        if let Some(layout) = self.devices {
            requests.push(Request::ConnectedDevices(layout));
        }
        Output {
            writes: self.request_all(requests, now),
            ..Output::default()
        }
    }

    pub fn tick(&mut self, now: Instant) -> Output {
        Output {
            writes: self.session.tick(now),
            ..Output::default()
        }
    }

    fn request_all(
        &mut self,
        requests: impl IntoIterator<Item = Request>,
        now: Instant,
    ) -> Vec<Frame> {
        requests
            .into_iter()
            .flat_map(|request| self.session.request(request, now))
            .collect()
    }

    fn change_noise(&mut self, change: impl FnOnce(&mut NoiseControl)) -> Option<Request> {
        let mut noise = self.noise?;
        change(&mut noise);
        if self.noise == Some(noise) {
            return None;
        }
        self.noise = Some(noise);
        Some(Request::SetNoiseControl(noise))
    }

    fn change_chat(&mut self, change: impl FnOnce(&mut ChatOptions)) -> Option<Request> {
        let mut chat = self.chat?;
        change(&mut chat);
        if self.chat == Some(chat) {
            return None;
        }
        self.chat = Some(chat);
        Some(Request::SetSpeakToChatOptions(chat))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{ACK, Message};

    fn headset_says(driver: &mut SonyXm5Driver, payload: &[u8], now: Instant) -> Output {
        let mut bytes = Message {
            kind: ACK,
            seq: 1,
            payload: Vec::new(),
        }
        .encode()
        .unwrap();
        bytes.extend(Message::command(0, payload.to_vec()).encode().unwrap());
        driver.receive(&bytes, now)
    }

    #[test]
    fn turns_captured_notifications_into_updates() {
        let now = Instant::now();
        let mut driver = SonyXm5Driver::default();
        driver.start(now);
        let output = headset_says(&mut driver, &[0x67, 0x17, 1, 1, 1, 0, 20], now);
        assert_eq!(
            output.updates,
            [
                Update::Mode(ListeningMode::Ambient),
                Update::AmbientLevel(20),
                Update::FocusOnVoice(false),
            ]
        );
    }

    #[test]
    fn waits_for_composite_state_before_changing_it() {
        let now = Instant::now();
        let mut driver = SonyXm5Driver::default();
        assert!(
            driver
                .set(Setting::Mode(ListeningMode::Ambient), now)
                .writes
                .is_empty()
        );
        assert!(
            driver
                .set(Setting::VoiceSensitivity(VoiceSensitivity::High), now)
                .writes
                .is_empty()
        );
    }
}
