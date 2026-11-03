use std::sync::Arc;

use async_channel::Sender;
use iheadphone_airpods_protocol::{BatteryState as AirPodsBattery, PodState};
use iheadphone_domain::{DeviceKind, NoiseMode};
use iheadphone_sony_protocol::{ConnectedDevice, EqPreset, WearingStatus};
use ksni::{
    Category, Icon, MenuItem, OfflineReason, ToolTip, Tray,
    menu::{CheckmarkItem, StandardItem, SubMenu},
};

use crate::{APP_ICON_BYTES, APP_ID};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraySony {
    pub charging: bool,
    pub codec: Option<String>,
    pub wearing: Option<WearingStatus>,
    pub dsee: bool,
    pub speak_to_chat: bool,
    pub pause_when_taken_off: bool,
    pub equalizer_preset: Option<EqPreset>,
    pub equalizer_choices: Vec<(EqPreset, String)>,
    pub connected_devices: Vec<ConnectedDevice>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayAirPods {
    pub battery: AirPodsBattery,
    pub wearing: Option<(PodState, PodState)>,
    pub earbuds: bool,
    pub ear_detection_supported: bool,
    pub ear_detection: bool,
    pub conversation_awareness_supported: bool,
    pub conversation_awareness: bool,
    pub personalized_volume: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayDevice {
    pub kind: DeviceKind,
    pub name: String,
    pub connected: bool,
    pub battery_percent: u8,
    pub noise_mode: NoiseMode,
    pub available_modes: Vec<NoiseMode>,
    pub sony: Option<TraySony>,
    pub airpods: Option<TrayAirPods>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TraySnapshot {
    pub chinese: bool,
    pub devices: Vec<TrayDevice>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayFeature {
    Dsee,
    SpeakToChat,
    PauseWhenTakenOff,
    ConversationAwareness,
    PersonalizedVolume,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayCommand {
    Open,
    Quit,
    Available(bool),
    SetMode(DeviceKind, NoiseMode),
    SetFeature(DeviceKind, TrayFeature, bool),
    SetEqualizer(EqPreset),
    PlayFrom(String),
}

pub struct HeadphoneTray {
    pub snapshot: TraySnapshot,
    pub commands: Sender<TrayCommand>,
    icon: Arc<Icon>,
}

impl HeadphoneTray {
    pub fn new(snapshot: TraySnapshot, commands: Sender<TrayCommand>) -> Self {
        let icon = image::load_from_memory_with_format(APP_ICON_BYTES, image::ImageFormat::Png)
            .expect("embedded tray icon is a valid PNG")
            .resize_exact(32, 32, image::imageops::FilterType::Lanczos3)
            .into_rgba8();
        let data = icon
            .pixels()
            .flat_map(|pixel| [pixel[3], pixel[0], pixel[1], pixel[2]])
            .collect();
        Self {
            snapshot,
            commands,
            icon: Arc::new(Icon {
                width: 32,
                height: 32,
                data,
            }),
        }
    }

    fn action(&self, label: String, enabled: bool, command: TrayCommand) -> MenuItem<Self> {
        MenuItem::Standard(StandardItem {
            label,
            enabled,
            activate: Box::new(move |tray: &mut Self| {
                let _ = tray.commands.try_send(command.clone());
            }),
            ..Default::default()
        })
    }

    fn check(&self, label: String, checked: bool, command: TrayCommand) -> MenuItem<Self> {
        CheckmarkItem {
            label,
            checked,
            activate: Box::new(move |tray: &mut Self| {
                let _ = tray.commands.try_send(command.clone());
            }),
            ..Default::default()
        }
        .into()
    }

    fn submenu(&self, label: String, items: Vec<MenuItem<Self>>) -> MenuItem<Self> {
        SubMenu {
            label,
            submenu: items,
            ..Default::default()
        }
        .into()
    }

    fn info(&self, label: String) -> MenuItem<Self> {
        self.action(label, false, TrayCommand::Open)
    }

    fn label(&self, english: &'static str, chinese: &'static str) -> &'static str {
        if self.snapshot.chinese {
            chinese
        } else {
            english
        }
    }
}

impl Tray for HeadphoneTray {
    const MENU_ON_ACTIVATE: bool = true;

    fn watcher_online(&self) {
        let _ = self.commands.try_send(TrayCommand::Available(true));
    }

    fn watcher_offline(&self, _reason: OfflineReason) -> bool {
        let _ = self.commands.try_send(TrayCommand::Available(false));
        true
    }

    fn id(&self) -> String {
        APP_ID.into()
    }

    fn category(&self) -> Category {
        Category::Hardware
    }

    fn title(&self) -> String {
        self.snapshot
            .devices
            .iter()
            .find(|device| device.connected)
            .map_or_else(
                || "iHeadphone".into(),
                |device| format!("iHeadphone · {}%", device.battery_percent),
            )
    }

    fn icon_name(&self) -> String {
        String::new()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        vec![(*self.icon).clone()]
    }

    fn tool_tip(&self) -> ToolTip {
        let description = self
            .snapshot
            .devices
            .iter()
            .filter(|device| device.connected)
            .map(|device| {
                format!(
                    "{} · {}% · {}",
                    device.name,
                    device.battery_percent,
                    mode_label(device.noise_mode, self.snapshot.chinese)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        ToolTip {
            title: "iHeadphone".into(),
            description: if description.is_empty() {
                self.label("No headphones connected", "没有已连接的耳机")
                    .into()
            } else {
                description
            },
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut menu = Vec::new();
        for device in &self.snapshot.devices {
            let status = if device.connected {
                format!("{} · {}", device.name, self.label("Connected", "已连接"))
            } else {
                format!("{} · {}", device.name, self.label("Disconnected", "未连接"))
            };
            menu.push(self.info(status));
            if device.connected {
                if let Some(sony) = &device.sony {
                    let charging = if sony.charging {
                        self.label(" · Charging", " · 充电中")
                    } else {
                        ""
                    };
                    menu.push(self.info(format!(
                        "{}: {}%{charging}",
                        self.label("Battery", "电量"),
                        device.battery_percent
                    )));
                    menu.push(self.info(format!(
                        "{}: {}",
                        self.label("Wearing", "佩戴"),
                        sony_wearing_label(sony.wearing, self.snapshot.chinese)
                    )));
                    if let Some(codec) = &sony.codec {
                        menu.push(self.info(format!("{}: {codec}", self.label("Codec", "编码"))));
                    }
                }
                if let Some(airpods) = &device.airpods {
                    if !airpods.earbuds {
                        menu.push(self.info(format!(
                            "{}: {}%",
                            self.label("Battery", "电量"),
                            device.battery_percent
                        )));
                    } else {
                        for (name, level, charging, wearing) in [
                            (
                                self.label("Left", "左耳"),
                                airpods.battery.left,
                                airpods.battery.left_charging,
                                airpods.wearing.map(|w| w.0),
                            ),
                            (
                                self.label("Right", "右耳"),
                                airpods.battery.right,
                                airpods.battery.right_charging,
                                airpods.wearing.map(|w| w.1),
                            ),
                            (
                                self.label("Case", "充电盒"),
                                airpods.battery.case,
                                airpods.battery.case_charging,
                                None,
                            ),
                        ] {
                            let level =
                                level.map_or_else(|| "—".into(), |level| format!("{level}%"));
                            let mut label = format!("{name}: {level}");
                            if charging {
                                label.push_str(self.label(" · Charging", " · 充电中"));
                            } else if let Some(wearing) = wearing {
                                label.push_str(" · ");
                                label.push_str(pod_state_label(wearing, self.snapshot.chinese));
                            }
                            menu.push(self.info(label));
                        }
                    }
                }
                if !device.available_modes.is_empty() {
                    menu.push(
                        self.submenu(
                            format!(
                                "{}: {}",
                                self.label("Noise control", "噪声控制"),
                                mode_label(device.noise_mode, self.snapshot.chinese)
                            ),
                            device
                                .available_modes
                                .iter()
                                .map(|mode| {
                                    self.check(
                                        mode_label(*mode, self.snapshot.chinese).into(),
                                        *mode == device.noise_mode,
                                        TrayCommand::SetMode(device.kind, *mode),
                                    )
                                })
                                .collect(),
                        ),
                    );
                }
                let mut features = Vec::new();
                if let Some(sony) = &device.sony {
                    features.push(self.check(
                        "DSEE Extreme".into(),
                        sony.dsee,
                        TrayCommand::SetFeature(device.kind, TrayFeature::Dsee, !sony.dsee),
                    ));
                    features.push(self.check(
                        self.label("Speak-to-Chat", "说话聊天").into(),
                        sony.speak_to_chat,
                        TrayCommand::SetFeature(
                            device.kind,
                            TrayFeature::SpeakToChat,
                            !sony.speak_to_chat,
                        ),
                    ));
                    features.push(self.check(
                        self.label("Pause when taken off", "摘下时暂停").into(),
                        sony.pause_when_taken_off,
                        TrayCommand::SetFeature(
                            device.kind,
                            TrayFeature::PauseWhenTakenOff,
                            !sony.pause_when_taken_off,
                        ),
                    ));
                    let summary = sony
                        .equalizer_preset
                        .and_then(|selected| {
                            sony.equalizer_choices
                                .iter()
                                .find(|(preset, _)| *preset == selected)
                        })
                        .map(|(_, name)| name.as_str())
                        .unwrap_or(self.label("Unknown", "未知"));
                    menu.push(
                        self.submenu(
                            format!("{}: {summary}", self.label("Equalizer", "均衡器")),
                            sony.equalizer_choices
                                .iter()
                                .map(|(preset, name)| {
                                    self.check(
                                        name.clone(),
                                        sony.equalizer_preset == Some(*preset),
                                        TrayCommand::SetEqualizer(*preset),
                                    )
                                })
                                .collect(),
                        ),
                    );
                    if sony.connected_devices.is_empty() {
                        menu.push(self.info(format!(
                            "{}: {}",
                            self.label("Multipoint", "多点连接"),
                            self.label("No devices", "暂无设备")
                        )));
                    } else {
                        menu.push(
                            self.submenu(
                                format!(
                                    "{}: {}",
                                    self.label("Multipoint", "多点连接"),
                                    sony.connected_devices.len()
                                ),
                                sony.connected_devices
                                    .iter()
                                    .map(|source| {
                                        self.check(
                                            source.name.clone(),
                                            source.playing,
                                            TrayCommand::PlayFrom(source.id.clone()),
                                        )
                                    })
                                    .collect(),
                            ),
                        );
                    }
                }
                if let Some(airpods) = &device.airpods {
                    if airpods.ear_detection_supported {
                        features.push(self.check(
                            self.label("Automatic ear detection", "自动入耳检测").into(),
                            airpods.ear_detection,
                            TrayCommand::SetFeature(
                                device.kind,
                                TrayFeature::PauseWhenTakenOff,
                                !airpods.ear_detection,
                            ),
                        ));
                    }
                    if airpods.conversation_awareness_supported {
                        features.push(self.check(
                            self.label("Conversation awareness", "对话感知").into(),
                            airpods.conversation_awareness,
                            TrayCommand::SetFeature(
                                device.kind,
                                TrayFeature::ConversationAwareness,
                                !airpods.conversation_awareness,
                            ),
                        ));
                    }
                    if let Some(personalized) = airpods.personalized_volume {
                        features.push(self.check(
                            self.label("Personalized volume", "个性化音量").into(),
                            personalized,
                            TrayCommand::SetFeature(
                                device.kind,
                                TrayFeature::PersonalizedVolume,
                                !personalized,
                            ),
                        ));
                    }
                }
                if !features.is_empty() {
                    menu.push(self.submenu(self.label("Smart sound", "智能声音").into(), features));
                }
            }
            menu.push(MenuItem::Separator);
        }
        menu.push(self.action(
            self.label("Open iHeadphone", "打开 iHeadphone").into(),
            true,
            TrayCommand::Open,
        ));
        menu.push(self.action(self.label("Quit", "退出").into(), true, TrayCommand::Quit));
        menu
    }
}

fn sony_wearing_label(status: Option<WearingStatus>, chinese: bool) -> &'static str {
    match (status, chinese) {
        (Some(WearingStatus::Normal), false) => "Worn",
        (Some(WearingStatus::Normal), true) => "已佩戴",
        (Some(WearingStatus::LeftRemoved | WearingStatus::RightRemoved), false) => {
            "Partially removed"
        }
        (Some(WearingStatus::LeftRemoved | WearingStatus::RightRemoved), true) => "部分摘下",
        (Some(WearingStatus::BothRemoved), false) => "Removed",
        (Some(WearingStatus::BothRemoved), true) => "已摘下",
        (_, false) => "Unknown",
        (_, true) => "未知",
    }
}

fn pod_state_label(state: PodState, chinese: bool) -> &'static str {
    match (state, chinese) {
        (PodState::InEar, false) => "In ear",
        (PodState::InEar, true) => "已佩戴",
        (PodState::OutOfEar, false) => "Out of ear",
        (PodState::OutOfEar, true) => "已摘下",
        (PodState::InCase, false) => "In case",
        (PodState::InCase, true) => "盒中",
        (_, false) => "Unknown",
        (_, true) => "未知",
    }
}

fn mode_label(mode: NoiseMode, chinese: bool) -> &'static str {
    match (mode, chinese) {
        (NoiseMode::Off, false) => "Off",
        (NoiseMode::Off, true) => "关闭",
        (NoiseMode::NoiseCancelling, false) => "Noise cancelling",
        (NoiseMode::NoiseCancelling, true) => "降噪",
        (NoiseMode::Transparency, false) => "Transparency",
        (NoiseMode::Transparency, true) => "通透模式",
        (NoiseMode::Adaptive, false) => "Adaptive",
        (NoiseMode::Adaptive, true) => "自适应",
        (NoiseMode::Ambient, false) => "Ambient sound",
        (NoiseMode::Ambient, true) => "环境声",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_tray_watcher_availability() {
        let (commands, receiver) = async_channel::unbounded();
        let tray = HeadphoneTray::new(TraySnapshot::default(), commands);

        assert!(tray.watcher_offline(OfflineReason::No));
        assert_eq!(receiver.try_recv(), Ok(TrayCommand::Available(false)));
        tray.watcher_online();
        assert_eq!(receiver.try_recv(), Ok(TrayCommand::Available(true)));
    }

    #[test]
    fn tray_menu_exposes_battery_and_sends_quick_mode_command() {
        let (commands, receiver) = async_channel::unbounded();
        let mut tray = HeadphoneTray::new(
            TraySnapshot {
                chinese: false,
                devices: vec![TrayDevice {
                    kind: DeviceKind::SonyWh1000Xm5,
                    name: "WH-1000XM5".into(),
                    connected: true,
                    battery_percent: 63,
                    noise_mode: NoiseMode::Ambient,
                    available_modes: vec![NoiseMode::NoiseCancelling, NoiseMode::Ambient],
                    sony: Some(TraySony {
                        charging: false,
                        codec: Some("LDAC".into()),
                        wearing: Some(WearingStatus::Normal),
                        dsee: true,
                        speak_to_chat: false,
                        pause_when_taken_off: true,
                        equalizer_preset: Some(EqPreset::Bright),
                        equalizer_choices: vec![
                            (EqPreset::Off, "Off".into()),
                            (EqPreset::Bright, "Bright".into()),
                        ],
                        connected_devices: vec![ConnectedDevice {
                            id: "laptop".into(),
                            name: "Laptop".into(),
                            playing: true,
                        }],
                    }),
                    airpods: None,
                }],
            },
            commands,
        );
        let menu = tray.menu();
        let MenuItem::Standard(status) = &menu[0] else {
            panic!("device status must be first");
        };
        assert_eq!(status.label, "WH-1000XM5 · Connected");
        assert!(!status.enabled);
        let MenuItem::Standard(battery) = &menu[1] else {
            panic!("battery status must follow device status");
        };
        assert_eq!(battery.label, "Battery: 63%");
        let noise = menu
            .iter()
            .find_map(|item| match item {
                MenuItem::SubMenu(item) if item.label.starts_with("Noise control") => Some(item),
                _ => None,
            })
            .unwrap();
        let MenuItem::Checkmark(noise_cancelling) = &noise.submenu[0] else {
            panic!("noise mode must be checkable");
        };
        (noise_cancelling.activate)(&mut tray);
        assert_eq!(
            receiver.try_recv(),
            Ok(TrayCommand::SetMode(
                DeviceKind::SonyWh1000Xm5,
                NoiseMode::NoiseCancelling
            ))
        );
        assert!(menu.iter().any(
            |item| matches!(item, MenuItem::SubMenu(item) if item.label.starts_with("Equalizer"))
        ));
        assert!(menu.iter().any(
            |item| matches!(item, MenuItem::SubMenu(item) if item.label.starts_with("Multipoint"))
        ));
    }

    #[test]
    fn airpods_menu_shows_each_battery_and_supported_features() {
        let (commands, receiver) = async_channel::unbounded();
        let mut tray = HeadphoneTray::new(
            TraySnapshot {
                chinese: true,
                devices: vec![TrayDevice {
                    kind: DeviceKind::AirPods,
                    name: "AirPods Pro 2".into(),
                    connected: true,
                    battery_percent: 90,
                    noise_mode: NoiseMode::Adaptive,
                    available_modes: vec![NoiseMode::NoiseCancelling, NoiseMode::Adaptive],
                    sony: None,
                    airpods: Some(TrayAirPods {
                        battery: AirPodsBattery {
                            left: Some(92),
                            right: Some(88),
                            case: Some(64),
                            case_charging: true,
                            ..Default::default()
                        },
                        wearing: Some((PodState::InEar, PodState::InCase)),
                        earbuds: true,
                        ear_detection_supported: true,
                        ear_detection: true,
                        conversation_awareness_supported: true,
                        conversation_awareness: false,
                        personalized_volume: Some(true),
                    }),
                }],
            },
            commands,
        );
        let menu = tray.menu();
        assert!(menu.iter().any(
            |item| matches!(item, MenuItem::Standard(item) if item.label == "左耳: 92% · 已佩戴")
        ));
        assert!(menu.iter().any(
            |item| matches!(item, MenuItem::Standard(item) if item.label == "右耳: 88% · 盒中")
        ));
        assert!(menu.iter().any(
            |item| matches!(item, MenuItem::Standard(item) if item.label == "充电盒: 64% · 充电中")
        ));
        let smart = menu
            .iter()
            .find_map(|item| match item {
                MenuItem::SubMenu(item) if item.label == "智能声音" => Some(item),
                _ => None,
            })
            .unwrap();
        let MenuItem::Checkmark(conversation) = &smart.submenu[1] else {
            panic!("conversation awareness must be checkable");
        };
        assert!(!conversation.checked);
        (conversation.activate)(&mut tray);
        assert_eq!(
            receiver.try_recv(),
            Ok(TrayCommand::SetFeature(
                DeviceKind::AirPods,
                TrayFeature::ConversationAwareness,
                true
            ))
        );
        tray.snapshot.devices[0].airpods.as_mut().unwrap().earbuds = false;
        let over_ear_menu = tray.menu();
        assert!(
            over_ear_menu
                .iter()
                .any(|item| matches!(item, MenuItem::Standard(item) if item.label == "电量: 90%"))
        );
        assert!(!over_ear_menu.iter().any(
            |item| matches!(item, MenuItem::Standard(item) if item.label.starts_with("左耳:"))
        ));
    }
}
