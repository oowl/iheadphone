#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeviceKind {
    AirPodsPro2,
    SonyWh1000Xm5,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormFactor {
    TrueWireless,
    OverEar,
}

impl DeviceKind {
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::AirPodsPro2 => "AirPods Pro 2",
            Self::SonyWh1000Xm5 => "WH-1000XM5",
        }
    }

    pub const fn manufacturer(self) -> &'static str {
        match self {
            Self::AirPodsPro2 => "Apple",
            Self::SonyWh1000Xm5 => "Sony",
        }
    }

    pub const fn form_factor(self) -> FormFactor {
        match self {
            Self::AirPodsPro2 => FormFactor::TrueWireless,
            Self::SonyWh1000Xm5 => FormFactor::OverEar,
        }
    }

    pub fn uses_sony_over_ear_layout(self) -> bool {
        self.manufacturer() == "Sony" && self.form_factor() == FormFactor::OverEar
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoiseMode {
    Off,
    NoiseCancelling,
    Transparency,
    Adaptive,
    Ambient,
}

impl NoiseMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::NoiseCancelling => "Noise cancelling",
            Self::Transparency => "Transparency",
            Self::Adaptive => "Adaptive",
            Self::Ambient => "Ambient sound",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceCapabilities {
    pub noise_modes: Vec<NoiseMode>,
    pub ambient_level: bool,
    pub equalizer: bool,
    pub dsee: bool,
    pub conversation_awareness: bool,
    pub speak_to_chat: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceSnapshot {
    pub kind: DeviceKind,
    pub connected: bool,
    pub battery_percent: u8,
    pub noise_mode: NoiseMode,
    pub ambient_level: u8,
    pub equalizer_enabled: bool,
    pub dsee_enabled: bool,
    pub conversation_awareness: bool,
    pub speak_to_chat: bool,
    pub capabilities: DeviceCapabilities,
}

impl DeviceSnapshot {
    pub fn set_noise_mode(&mut self, mode: NoiseMode) -> bool {
        if !self.capabilities.noise_modes.contains(&mode) {
            return false;
        }
        self.noise_mode = mode;
        true
    }

    pub fn adjust_ambient(&mut self, delta: i8) {
        if !self.capabilities.ambient_level {
            return;
        }
        self.ambient_level = self.ambient_level.saturating_add_signed(delta).clamp(1, 20);
    }
}

pub fn sony_wh1000xm5(connected: bool) -> DeviceSnapshot {
    DeviceSnapshot {
        kind: DeviceKind::SonyWh1000Xm5,
        connected,
        battery_percent: 87,
        noise_mode: NoiseMode::NoiseCancelling,
        ambient_level: 8,
        equalizer_enabled: true,
        dsee_enabled: true,
        conversation_awareness: false,
        speak_to_chat: false,
        capabilities: DeviceCapabilities {
            noise_modes: vec![
                NoiseMode::Off,
                NoiseMode::NoiseCancelling,
                NoiseMode::Ambient,
            ],
            ambient_level: true,
            equalizer: true,
            dsee: true,
            conversation_awareness: false,
            speak_to_chat: true,
        },
    }
}

pub fn airpods_pro2(connected: bool) -> DeviceSnapshot {
    DeviceSnapshot {
        kind: DeviceKind::AirPodsPro2,
        connected,
        battery_percent: 0,
        noise_mode: NoiseMode::Off,
        ambient_level: 10,
        equalizer_enabled: false,
        dsee_enabled: false,
        conversation_awareness: false,
        speak_to_chat: false,
        capabilities: DeviceCapabilities {
            noise_modes: vec![
                NoiseMode::Off,
                NoiseMode::NoiseCancelling,
                NoiseMode::Transparency,
                NoiseMode::Adaptive,
            ],
            ambient_level: false,
            equalizer: false,
            dsee: false,
            conversation_awareness: true,
            speak_to_chat: false,
        },
    }
}

pub fn mock_devices() -> Vec<DeviceSnapshot> {
    let mut devices = vec![sony_wh1000xm5(true)];

    if cfg!(target_os = "linux") {
        let mut airpods = airpods_pro2(true);
        airpods.battery_percent = 74;
        airpods.noise_mode = NoiseMode::Adaptive;
        airpods.conversation_awareness = true;
        devices.push(airpods);
    }

    devices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsupported_noise_mode() {
        let mut sony = mock_devices().remove(0);
        assert!(!sony.set_noise_mode(NoiseMode::Adaptive));
        assert_eq!(sony.noise_mode, NoiseMode::NoiseCancelling);
    }

    #[test]
    fn clamps_ambient_level() {
        let mut sony = mock_devices().remove(0);
        sony.adjust_ambient(-100);
        assert_eq!(sony.ambient_level, 1);
        sony.adjust_ambient(100);
        assert_eq!(sony.ambient_level, 20);
    }

    #[test]
    fn routes_only_sony_over_ear_devices_to_the_headband_layout() {
        assert!(DeviceKind::SonyWh1000Xm5.uses_sony_over_ear_layout());
        assert!(!DeviceKind::AirPodsPro2.uses_sony_over_ear_layout());
    }
}
