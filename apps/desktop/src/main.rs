use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::process::Child;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::process::{Command, Stdio};
use std::sync::{Arc, OnceLock, mpsc};

use gpui_kit::component::{Icon, scroll::ScrollableElement};
use gpui_kit::{
    App, AppContext, Bounds, Context, Entity, FontWeight, Image as GpuiImage, ImageFormat,
    IntoElement, ObjectFit, Render, SharedString, Window, WindowBounds, WindowOptions,
    assets::IconName, div, img, prelude::*, px, rgb, size,
};
use iheadphone_airpods_protocol::{
    BatteryState as AirPodsBattery, ListeningMode as AirPodsListeningMode,
    Microphone as AirPodsMicrophone, PodState, Setting as AirPodsSetting, Side as AirPodsSide,
    Update as AirPodsUpdate, WearingState as AirPodsWearing,
};
use iheadphone_domain::{
    DeviceKind, DeviceSnapshot, NoiseMode, airpods_pro2, mock_devices, sony_wh1000xm5,
};
use iheadphone_sony_protocol::{
    AutoOff, ConnectedDevice, EqPreset, Equalizer, Feature, ListeningMode, NoiseButton, Setting,
    SpeakToChatEnd, Update, VoiceSensitivity, WearingStatus,
};
use iheadphone_transport::{
    Device as BluetoothDevice, Event as BluetoothEvent, Request as BluetoothRequest,
};

// Material 3 Expressive dark color roles.
const CANVAS: u32 = 0x111014;
const SURFACE: u32 = 0x1b1b1f;
const SURFACE_CONTAINER: u32 = 0x211f26;
const SURFACE_HIGH: u32 = 0x2b2930;
const ON_SURFACE: u32 = 0xe8e1e9;
const ON_SURFACE_VARIANT: u32 = 0xcac4d0;
const OUTLINE: u32 = 0x948f99;
const PRIMARY: u32 = 0xcfbdff;
const ON_PRIMARY: u32 = 0x382b67;
const PRIMARY_CONTAINER: u32 = 0x4e3f78;
const ON_PRIMARY_CONTAINER: u32 = 0xeadcff;
const SECONDARY_CONTAINER: u32 = 0x3e4759;
const ON_SECONDARY_CONTAINER: u32 = 0xdbe2f9;
const TERTIARY_CONTAINER: u32 = 0x633b48;
const ON_TERTIARY_CONTAINER: u32 = 0xffd9e2;
const SUCCESS: u32 = 0xa7f0ba;
const AIRPODS_WORN: u32 = 0x70e1a1;
const AIRPODS_CHARGING: u32 = 0xb79cff;
const AIRPODS_CASE: u32 = 0x88b8ff;
const SONY_MODE: u32 = 0xf18ae8;
const SONY_CODEC: u32 = 0x78b0ff;
const APP_ID: &str = "io.github.iheadphone.iheadphone";
const APP_ICON_BYTES: &[u8] = include_bytes!("../assets/iheadphone-icon.png");
const AIRPODS_LEFT_BYTES: &[u8] = include_bytes!("../assets/airpods-left.png");
const AIRPODS_RIGHT_BYTES: &[u8] = include_bytes!("../assets/airpods-right.png");
const AIRPODS_CASE_BYTES: &[u8] = include_bytes!("../assets/airpods-case.png");
const SONY_WH1000XM5_BYTES: &[u8] = include_bytes!("../assets/sony-wh1000xm5.png");
static APP_ICON_IMAGE: OnceLock<Arc<GpuiImage>> = OnceLock::new();
static AIRPODS_LEFT_IMAGE: OnceLock<Arc<GpuiImage>> = OnceLock::new();
static AIRPODS_RIGHT_IMAGE: OnceLock<Arc<GpuiImage>> = OnceLock::new();
static AIRPODS_CASE_IMAGE: OnceLock<Arc<GpuiImage>> = OnceLock::new();
static SONY_WH1000XM5_IMAGE: OnceLock<Arc<GpuiImage>> = OnceLock::new();

fn app_icon_image() -> Arc<GpuiImage> {
    APP_ICON_IMAGE
        .get_or_init(|| {
            Arc::new(GpuiImage::from_bytes(
                ImageFormat::Png,
                APP_ICON_BYTES.to_vec(),
            ))
        })
        .clone()
}

fn embedded_png(cache: &'static OnceLock<Arc<GpuiImage>>, bytes: &'static [u8]) -> Arc<GpuiImage> {
    cache
        .get_or_init(|| Arc::new(GpuiImage::from_bytes(ImageFormat::Png, bytes.to_vec())))
        .clone()
}

fn airpods_left_image() -> Arc<GpuiImage> {
    embedded_png(&AIRPODS_LEFT_IMAGE, AIRPODS_LEFT_BYTES)
}

fn airpods_right_image() -> Arc<GpuiImage> {
    embedded_png(&AIRPODS_RIGHT_IMAGE, AIRPODS_RIGHT_BYTES)
}

fn airpods_case_image() -> Arc<GpuiImage> {
    embedded_png(&AIRPODS_CASE_IMAGE, AIRPODS_CASE_BYTES)
}

fn sony_wh1000xm5_image() -> Arc<GpuiImage> {
    embedded_png(&SONY_WH1000XM5_IMAGE, SONY_WH1000XM5_BYTES)
}

fn window_icon() -> Option<Arc<image::RgbaImage>> {
    image::load_from_memory_with_format(APP_ICON_BYTES, image::ImageFormat::Png)
        .ok()
        .map(|image| Arc::new(image.into_rgba8()))
}

#[cfg(all(not(test), any(target_os = "linux", target_os = "macos")))]
fn send_desktop_notification(title: &str, body: &str) {
    let title = title.to_owned();
    let body = body.to_owned();
    let _ = std::thread::Builder::new()
        .name("iheadphone-notification".into())
        .spawn(move || {
            #[cfg(target_os = "linux")]
            let result = Command::new("notify-send")
                .args(["--app-name=iHeadphone", "--icon", APP_ID])
                .arg(&title)
                .arg(&body)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();

            #[cfg(target_os = "macos")]
            let result = Command::new("osascript")
                .args([
                    "-e",
                    "on run argv\n display notification (item 1 of argv) with title (item 2 of argv)\nend run",
                ])
                .arg(&body)
                .arg(&title)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();

            if let Err(error) = result {
                eprintln!("notification: cannot show desktop notification: {error}");
            }
        });
}

#[cfg(any(test, not(any(target_os = "linux", target_os = "macos"))))]
fn send_desktop_notification(_title: &str, _body: &str) {}

#[cfg(target_os = "linux")]
struct MediaBridge {
    child: Option<Child>,
    paused_players: Vec<String>,
    speak_to_chat_active: bool,
    taken_off_active: bool,
}

#[cfg(target_os = "linux")]
impl MediaBridge {
    fn start() -> Self {
        let service_is_active = Command::new("systemctl")
            .args(["--user", "is-active", "--quiet", "mpris-proxy.service"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if service_is_active {
            return Self {
                child: None,
                paused_players: Vec::new(),
                speak_to_chat_active: false,
                taken_off_active: false,
            };
        }

        let child = Command::new("mpris-proxy")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .inspect_err(|error| {
                eprintln!(
                    "media: cannot start mpris-proxy; headset playback buttons may not reach applications: {error}"
                );
            })
            .ok();
        Self {
            child,
            paused_players: Vec::new(),
            speak_to_chat_active: false,
            taken_off_active: false,
        }
    }

    fn set_speak_to_chat_active(&mut self, active: bool) {
        let was_pausing = self.should_pause();
        self.speak_to_chat_active = active;
        self.apply_pause_transition(was_pausing);
    }

    fn set_taken_off_active(&mut self, active: bool) {
        let was_pausing = self.should_pause();
        self.taken_off_active = active;
        self.apply_pause_transition(was_pausing);
    }

    fn clear_pause_reasons(&mut self) {
        let was_pausing = self.should_pause();
        self.speak_to_chat_active = false;
        self.taken_off_active = false;
        self.apply_pause_transition(was_pausing);
    }

    fn should_pause(&self) -> bool {
        self.speak_to_chat_active || self.taken_off_active
    }

    fn apply_pause_transition(&mut self, was_pausing: bool) {
        let is_pausing = self.should_pause();
        if was_pausing == is_pausing {
            return;
        }
        if is_pausing {
            self.paused_players = Self::players_with_status("Playing")
                .into_iter()
                .filter(|player| Self::player_command(player, "pause"))
                .collect();
        } else {
            for player in self.paused_players.drain(..) {
                let _ = Self::player_command(&player, "play");
            }
        }
    }

    fn players_with_status(expected: &str) -> Vec<String> {
        let Ok(output) = Command::new("playerctl").arg("-l").output() else {
            return Vec::new();
        };
        if !output.status.success() {
            return Vec::new();
        }
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|player| {
                Command::new("playerctl")
                    .args(["-p", player, "status"])
                    .output()
                    .is_ok_and(|output| {
                        output.status.success()
                            && String::from_utf8_lossy(&output.stdout).trim() == expected
                    })
            })
            .map(str::to_owned)
            .collect()
    }

    fn player_command(player: &str, command: &str) -> bool {
        Command::new("playerctl")
            .args(["-p", player, command])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
}

#[cfg(target_os = "linux")]
impl Drop for MediaBridge {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Language {
    #[default]
    English,
    SimplifiedChinese,
}

impl Language {
    fn load() -> Self {
        let Some(path) = Self::settings_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(path).as_deref().map(str::trim) {
            Ok("zh-CN") => Self::SimplifiedChinese,
            _ => Self::English,
        }
    }

    fn save(self) {
        let Some(path) = Self::settings_path() else {
            return;
        };
        if let Some(parent) = path.parent()
            && std::fs::create_dir_all(parent).is_err()
        {
            return;
        }
        let value = match self {
            Self::English => "en",
            Self::SimplifiedChinese => "zh-CN",
        };
        let _ = std::fs::write(path, value);
    }

    fn settings_path() -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        {
            return std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join("Library/Application Support/iHeadphone/settings"));
        }
        #[cfg(not(target_os = "macos"))]
        {
            if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
                return Some(PathBuf::from(config).join("iheadphone/settings"));
            }
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".config/iheadphone/settings"))
        }
    }
}

#[derive(Clone)]
struct SonyDetails {
    charging: bool,
    firmware: Option<String>,
    codec: Option<String>,
    focus_on_voice: bool,
    equalizer: Option<Equalizer>,
    pause_when_taken_off: bool,
    auto_off: Option<AutoOff>,
    noise_button: Option<NoiseButton>,
    voice_sensitivity: Option<VoiceSensitivity>,
    speak_to_chat_end: Option<SpeakToChatEnd>,
    wearing_status: Option<WearingStatus>,
    connected_devices: Vec<ConnectedDevice>,
}

impl Default for SonyDetails {
    fn default() -> Self {
        Self {
            charging: false,
            firmware: None,
            codec: None,
            focus_on_voice: false,
            equalizer: None,
            pause_when_taken_off: true,
            auto_off: None,
            noise_button: None,
            voice_sensitivity: None,
            speak_to_chat_end: None,
            wearing_status: None,
            connected_devices: Vec::new(),
        }
    }
}

#[derive(Clone, Default)]
struct AirPodsDetails {
    battery: AirPodsBattery,
    wearing: Option<AirPodsWearing>,
    ear_detection: bool,
    one_bud_anc: bool,
    microphone: Option<AirPodsMicrophone>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DetailPage {
    Overview,
    Sound,
    Device,
}

struct HeadphoneApp {
    devices: Vec<DeviceSnapshot>,
    selected: usize,
    requests: Option<mpsc::Sender<BluetoothRequest>>,
    backend_status: String,
    sony: SonyDetails,
    airpods: AirPodsDetails,
    language: Language,
    show_settings: bool,
    detail_page: DetailPage,
    #[cfg(target_os = "linux")]
    _media_bridge: Option<MediaBridge>,
}

impl HeadphoneApp {
    fn new(requests: Option<mpsc::Sender<BluetoothRequest>>) -> Self {
        let hardware = requests.is_some();
        let language = Language::load();
        let devices = if hardware {
            let mut sony = sony_wh1000xm5(false);
            sony.battery_percent = 0;
            sony.noise_mode = NoiseMode::Off;
            let mut devices = vec![sony];
            #[cfg(target_os = "linux")]
            devices.push(airpods_pro2(false));
            devices
        } else {
            mock_devices()
        };
        Self {
            devices,
            selected: 0,
            requests,
            backend_status: if hardware && language == Language::SimplifiedChinese {
                "正在查找 WH-1000XM5…".into()
            } else if hardware {
                "Searching for WH-1000XM5…".into()
            } else if language == Language::SimplifiedChinese {
                "控制功能正在预览模式下运行。".into()
            } else {
                "Controls are running in preview mode.".into()
            },
            sony: SonyDetails::default(),
            airpods: AirPodsDetails {
                ear_detection: true,
                ..AirPodsDetails::default()
            },
            language,
            show_settings: std::env::var_os("IHEADPHONE_OPEN_SETTINGS").is_some(),
            detail_page: DetailPage::Overview,
            #[cfg(target_os = "linux")]
            _media_bridge: None,
        }
    }

    fn t(&self, english: &'static str, chinese: &'static str) -> &'static str {
        match self.language {
            Language::English => english,
            Language::SimplifiedChinese => chinese,
        }
    }

    fn noise_mode_label(&self, mode: NoiseMode) -> &'static str {
        match mode {
            NoiseMode::Off => self.t("Off", "关闭"),
            NoiseMode::NoiseCancelling => self.t("Noise cancelling", "降噪"),
            NoiseMode::Transparency => self.t("Transparency", "通透模式"),
            NoiseMode::Adaptive => self.t("Adaptive", "自适应"),
            NoiseMode::Ambient => self.t("Ambient sound", "环境声"),
        }
    }

    fn preset_label(&self, preset: EqPreset) -> &'static str {
        match preset {
            EqPreset::Off => self.t("Off", "关闭"),
            EqPreset::Bright => self.t("Bright", "明亮"),
            EqPreset::Excited => self.t("Excited", "激昂"),
            EqPreset::Mellow => self.t("Mellow", "柔和"),
            EqPreset::Relaxed => self.t("Relaxed", "轻松"),
            EqPreset::Vocal => self.t("Vocal", "人声"),
            EqPreset::TrebleBoost => self.t("Treble Boost", "高音增强"),
            EqPreset::BassBoost => self.t("Bass Boost", "低音增强"),
            EqPreset::Speech => self.t("Speech", "语音"),
            EqPreset::Manual => self.t("Manual", "手动"),
            EqPreset::Custom1 => self.t("Custom 1", "自定义 1"),
            EqPreset::Custom2 => self.t("Custom 2", "自定义 2"),
            EqPreset::Unknown(_) => self.t("Unknown", "未知"),
        }
    }

    fn selected_device(&self) -> &DeviceSnapshot {
        &self.devices[self.selected]
    }

    fn send_setting(&self, setting: Setting) {
        if let Some(requests) = &self.requests {
            let _ = requests.send(BluetoothRequest::Sony(setting));
        }
    }

    fn send_airpods_setting(&self, setting: AirPodsSetting) {
        if let Some(requests) = &self.requests {
            let _ = requests.send(BluetoothRequest::AirPods(setting));
        }
    }

    fn device_index(&self, device: BluetoothDevice) -> Option<usize> {
        let kind = match device {
            BluetoothDevice::SonyWh1000Xm5 => DeviceKind::SonyWh1000Xm5,
            BluetoothDevice::AirPods => DeviceKind::AirPodsPro2,
        };
        self.devices.iter().position(|item| item.kind == kind)
    }

    fn apply_bluetooth_event(&mut self, event: BluetoothEvent) {
        match event {
            BluetoothEvent::Searching(kind) => {
                if let Some(index) = self.device_index(kind) {
                    self.devices[index].connected = false;
                }
                if kind == BluetoothDevice::SonyWh1000Xm5 {
                    self.sony.wearing_status = None;
                } else {
                    self.airpods.wearing = None;
                }
                self.backend_status = match (self.language, kind) {
                    (Language::English, BluetoothDevice::SonyWh1000Xm5) => {
                        "Searching for WH-1000XM5…".into()
                    }
                    (Language::SimplifiedChinese, BluetoothDevice::SonyWh1000Xm5) => {
                        "正在查找 WH-1000XM5…".into()
                    }
                    (Language::English, BluetoothDevice::AirPods) => {
                        "Searching for connected AirPods…".into()
                    }
                    (Language::SimplifiedChinese, BluetoothDevice::AirPods) => {
                        "正在查找已连接的 AirPods…".into()
                    }
                };
            }
            BluetoothEvent::Disconnected(kind) => {
                let was_connected = self
                    .device_index(kind)
                    .is_some_and(|index| self.devices[index].connected);
                if let Some(index) = self.device_index(kind) {
                    self.devices[index].connected = false;
                }
                if kind == BluetoothDevice::SonyWh1000Xm5 {
                    self.sony.wearing_status = None;
                } else {
                    self.airpods.wearing = None;
                }
                #[cfg(target_os = "linux")]
                if let Some(bridge) = self._media_bridge.as_mut() {
                    bridge.clear_pause_reasons();
                }
                if was_connected {
                    let name = match kind {
                        BluetoothDevice::SonyWh1000Xm5 => "WH-1000XM5",
                        BluetoothDevice::AirPods => "AirPods",
                    };
                    let body = match self.language {
                        Language::English => format!("{name} disconnected."),
                        Language::SimplifiedChinese => format!("{name} 已断开连接。"),
                    };
                    send_desktop_notification("iHeadphone", &body);
                }
            }
            BluetoothEvent::Connected { device: kind, name } => {
                let Some(index) = self.device_index(kind) else {
                    return;
                };
                let newly_connected = !self.devices[index].connected;
                self.devices[index].connected = true;
                self.backend_status = match self.language {
                    Language::English => format!("{name} hardware controls are live."),
                    Language::SimplifiedChinese => format!("{name} 硬件控制已启用。"),
                };
                if newly_connected {
                    let body = match self.language {
                        Language::English => format!("{name} connected."),
                        Language::SimplifiedChinese => format!("{name} 已连接。"),
                    };
                    send_desktop_notification("iHeadphone", &body);
                }
            }
            BluetoothEvent::Unsupported {
                device: kind,
                reason,
            } => {
                if let Some(index) = self.device_index(kind) {
                    self.devices[index].connected = false;
                }
                self.backend_status = reason;
            }
            BluetoothEvent::SonyUpdate(update) => {
                let Some(index) = self.device_index(BluetoothDevice::SonyWh1000Xm5) else {
                    return;
                };
                let device = &mut self.devices[index];
                match update {
                    Update::Firmware(value) => self.sony.firmware = Some(value),
                    Update::Codec(value) => self.sony.codec = Some(value),
                    Update::Battery(battery) => {
                        device.battery_percent = battery.level;
                        self.sony.charging = battery.charging;
                    }
                    Update::Mode(mode) => {
                        device.noise_mode = match mode {
                            ListeningMode::NoiseCancelling => NoiseMode::NoiseCancelling,
                            ListeningMode::Ambient => NoiseMode::Ambient,
                            ListeningMode::Off => NoiseMode::Off,
                        };
                    }
                    Update::AmbientLevel(level) => device.ambient_level = level.clamp(1, 20),
                    Update::FocusOnVoice(enabled) => self.sony.focus_on_voice = enabled,
                    Update::Feature(Feature::Dsee, enabled) => device.dsee_enabled = enabled,
                    Update::Feature(Feature::SpeakToChat, enabled) => {
                        device.speak_to_chat = enabled;
                    }
                    Update::Feature(Feature::PauseWhenTakenOff, enabled) => {
                        self.sony.pause_when_taken_off = enabled;
                        #[cfg(target_os = "linux")]
                        if let (Some(bridge), Some(status)) =
                            (self._media_bridge.as_mut(), self.sony.wearing_status)
                        {
                            bridge.set_taken_off_active(enabled && status.is_taken_off());
                        }
                    }
                    Update::Equalizer(equalizer) => {
                        device.equalizer_enabled = equalizer.preset != EqPreset::Off;
                        self.sony.equalizer = Some(equalizer);
                    }
                    Update::AutoOff(value) => self.sony.auto_off = Some(value),
                    Update::NoiseButton(value) => self.sony.noise_button = Some(value),
                    Update::VoiceSensitivity(value) => self.sony.voice_sensitivity = Some(value),
                    Update::SpeakToChatEnd(value) => self.sony.speak_to_chat_end = Some(value),
                    Update::SpeakToChatActive(active) => {
                        #[cfg(target_os = "linux")]
                        if let Some(bridge) = self._media_bridge.as_mut() {
                            bridge.set_speak_to_chat_active(active);
                        }
                        self.backend_status = if active {
                            self.t(
                                "Speak-to-Chat paused Linux media.",
                                "说话聊天已暂停 Linux 媒体播放。",
                            )
                        } else {
                            self.t("Speak-to-Chat conversation ended.", "说话聊天对话已结束。")
                        }
                        .into();
                    }
                    Update::WearingStatus(status) => {
                        self.sony.wearing_status = Some(status);
                        let taken_off = self.sony.pause_when_taken_off && status.is_taken_off();
                        #[cfg(target_os = "linux")]
                        if let Some(bridge) = self._media_bridge.as_mut() {
                            bridge.set_taken_off_active(taken_off);
                        }
                        self.backend_status = if taken_off {
                            self.t(
                                "Headphones removed; Linux media paused.",
                                "已摘下耳机；Linux 媒体已暂停。",
                            )
                        } else if status.is_taken_off() {
                            self.t("Headphones removed.", "已摘下耳机。")
                        } else {
                            self.t(
                                "Headphones are being worn; Linux media resumed.",
                                "已佩戴耳机；Linux 媒体已恢复。",
                            )
                        }
                        .into();
                    }
                    Update::ConnectedDevices(value) => self.sony.connected_devices = value,
                }
            }
            BluetoothEvent::AirPodsUpdate(update) => {
                let Some(index) = self.device_index(BluetoothDevice::AirPods) else {
                    return;
                };
                let device = &mut self.devices[index];
                match update {
                    AirPodsUpdate::Battery(battery) => {
                        if battery.primary.is_some() {
                            self.airpods.battery.primary = battery.primary;
                        }
                        if battery.left.is_some() {
                            self.airpods.battery.left = battery.left;
                            self.airpods.battery.left_charging = battery.left_charging;
                        }
                        if battery.right.is_some() {
                            self.airpods.battery.right = battery.right;
                            self.airpods.battery.right_charging = battery.right_charging;
                        }
                        if battery.case.is_some() {
                            self.airpods.battery.case = battery.case;
                            self.airpods.battery.case_charging = battery.case_charging;
                        }
                        if let Some(level) = self.airpods.battery.earbuds_percent() {
                            device.battery_percent = level;
                        }
                    }
                    AirPodsUpdate::Wearing(wearing) => {
                        self.airpods.wearing = Some(wearing);
                        let taken_off = self.airpods.ear_detection && !wearing.any_in_ear();
                        #[cfg(target_os = "linux")]
                        if let Some(bridge) = self._media_bridge.as_mut() {
                            bridge.set_taken_off_active(taken_off);
                        }
                        self.backend_status = if taken_off {
                            self.t(
                                "AirPods removed; Linux media paused.",
                                "已摘下 AirPods；Linux 媒体已暂停。",
                            )
                        } else {
                            self.t("AirPods are being worn.", "已佩戴 AirPods。")
                        }
                        .into();
                    }
                    AirPodsUpdate::Mode(mode) => {
                        device.noise_mode = match mode {
                            AirPodsListeningMode::Off => NoiseMode::Off,
                            AirPodsListeningMode::NoiseCancelling => NoiseMode::NoiseCancelling,
                            AirPodsListeningMode::Transparency => NoiseMode::Transparency,
                            AirPodsListeningMode::Adaptive => NoiseMode::Adaptive,
                        };
                    }
                    AirPodsUpdate::ConversationAwareness(enabled) => {
                        device.conversation_awareness = enabled;
                    }
                    AirPodsUpdate::EarDetection(enabled) => {
                        self.airpods.ear_detection = enabled;
                    }
                    AirPodsUpdate::OneBudNoiseCancelling(enabled) => {
                        self.airpods.one_bud_anc = enabled;
                    }
                    AirPodsUpdate::Microphone(microphone) => {
                        self.airpods.microphone = Some(microphone);
                    }
                }
            }
        }
    }

    fn chip(text: impl Into<SharedString>, color: u32) -> impl IntoElement {
        div()
            .px_3()
            .py_1()
            .rounded_full()
            .bg(rgb(color).opacity(0.14))
            .text_sm()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(color))
            .child(text.into())
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div().flex().flex_col().gap_2().mt_4();
        for (index, device) in self.devices.iter().enumerate() {
            let selected = index == self.selected && !self.show_settings;
            let device_icon = match device.kind {
                DeviceKind::SonyWh1000Xm5 => img(sony_wh1000xm5_image())
                    .size(px(36.))
                    .object_fit(ObjectFit::Contain)
                    .into_any_element(),
                DeviceKind::AirPodsPro2 => div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        img(airpods_left_image())
                            .size(px(28.))
                            .object_fit(ObjectFit::Contain),
                    )
                    .child(
                        img(airpods_right_image())
                            .ml_neg_3()
                            .size(px(28.))
                            .object_fit(ObjectFit::Contain),
                    )
                    .into_any_element(),
            };
            list = list.child(
                div()
                    .id(SharedString::from(format!("device-{index}")))
                    .flex()
                    .items_center()
                    .gap_2()
                    .p_2()
                    .rounded_3xl()
                    .cursor_pointer()
                    .when(selected, |e| {
                        e.bg(rgb(PRIMARY_CONTAINER))
                            .text_color(rgb(ON_PRIMARY_CONTAINER))
                    })
                    .when(!selected, |e| {
                        e.bg(rgb(SURFACE_CONTAINER))
                            .hover(|e| e.bg(rgb(SURFACE_HIGH)))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = index;
                        this.show_settings = false;
                        this.detail_page = DetailPage::Overview;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .size(px(40.))
                            .rounded_full()
                            .overflow_hidden()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(if selected {
                                rgb(PRIMARY)
                            } else {
                                rgb(SURFACE_HIGH)
                            })
                            .text_color(if selected {
                                rgb(ON_PRIMARY)
                            } else {
                                rgb(ON_SURFACE)
                            })
                            .child(device_icon),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .gap_1()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(device.kind.display_name()),
                            )
                            .child(div().text_sm().text_color(rgb(ON_SURFACE_VARIANT)).child(
                                if device.connected {
                                    self.t("Connected", "已连接")
                                } else {
                                    self.t("Offline", "离线")
                                },
                            )),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("{}%", device.battery_percent)),
                    ),
            );
        }

        let status_title = if self.requests.is_none() {
            self.t("Preview mode", "预览模式")
        } else if self.devices[0].connected {
            self.t("Hardware connected", "硬件已连接")
        } else {
            self.t("Finding your headphones", "正在查找耳机")
        };

        div()
            .w(px(264.))
            .h_full()
            .flex()
            .flex_col()
            .p_4()
            .bg(rgb(SURFACE))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        img(app_icon_image())
                            .size(px(36.))
                            .rounded_xl()
                            .object_fit(ObjectFit::Cover),
                    )
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::BOLD)
                            .child("iHeadphone"),
                    ),
            )
            .child(
                div()
                    .mt_5()
                    .px_2()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(ON_SURFACE_VARIANT))
                    .child(self.t("YOUR DEVICES", "你的设备")),
            )
            .child(list)
            .child(
                div()
                    .id("open-settings")
                    .mt_4()
                    .flex()
                    .items_center()
                    .gap_3()
                    .p_2()
                    .rounded_3xl()
                    .cursor_pointer()
                    .when(self.show_settings, |item| {
                        item.bg(rgb(PRIMARY_CONTAINER))
                            .text_color(rgb(ON_PRIMARY_CONTAINER))
                    })
                    .when(!self.show_settings, |item| {
                        item.bg(rgb(SURFACE_CONTAINER))
                            .hover(|item| item.bg(rgb(SURFACE_HIGH)))
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_settings = true;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .size(px(48.))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(rgb(SURFACE_HIGH))
                            .font_weight(FontWeight::BOLD)
                            .child("⚙"),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.t("Settings", "设置")),
                    ),
            )
            .child(
                div().mt_auto().pt_4().child(
                    div()
                        .p_3()
                        .rounded_3xl()
                        .bg(rgb(SECONDARY_CONTAINER))
                        .text_color(rgb(ON_SECONDARY_CONTAINER))
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(status_title))
                        .child(
                            div()
                                .mt_1()
                                .text_sm()
                                .text_color(rgb(ON_SECONDARY_CONTAINER).opacity(0.72))
                                .child(self.backend_status.clone()),
                        ),
                ),
            )
    }

    fn app_header(&self, device: &DeviceSnapshot) -> impl IntoElement {
        div()
            .flex()
            .items_end()
            .justify_between()
            .mb_2()
            .child(
                div()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(PRIMARY))
                            .child(self.t("NOW CONTROLLING", "正在控制")),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_xl()
                            .font_weight(FontWeight::BOLD)
                            .child(device.kind.display_name()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(Self::chip(
                        if device.connected {
                            self.t("Connected", "已连接")
                        } else {
                            self.t("Searching", "正在查找")
                        },
                        if device.connected { SUCCESS } else { OUTLINE },
                    ))
                    .when(device.connected, |chips| {
                        chips.child(Self::chip(
                            match self.language {
                                Language::English => format!("{}% battery", device.battery_percent),
                                Language::SimplifiedChinese => {
                                    format!("电量 {}%", device.battery_percent)
                                }
                            },
                            PRIMARY,
                        ))
                    })
                    .when(device.connected && self.sony.charging, |chips| {
                        chips.child(Self::chip(self.t("Charging", "充电中"), SUCCESS))
                    }),
            )
    }

    fn detail_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tabs = [
            (DetailPage::Overview, self.t("Overview", "概览")),
            (DetailPage::Sound, self.t("Sound", "声音")),
            (DetailPage::Device, self.t("Device", "设备")),
        ];
        let mut row = div()
            .flex()
            .gap_1()
            .p_1()
            .mb_2()
            .rounded_full()
            .bg(rgb(SURFACE_CONTAINER));
        for (page, label) in tabs {
            let selected = self.detail_page == page;
            row = row.child(
                div()
                    .id(SharedString::from(format!("detail-page-{page:?}")))
                    .flex_1()
                    .px_4()
                    .py_1()
                    .rounded_full()
                    .cursor_pointer()
                    .text_sm()
                    .text_center()
                    .font_weight(FontWeight::SEMIBOLD)
                    .when(selected, |tab| {
                        tab.bg(rgb(PRIMARY)).text_color(rgb(ON_PRIMARY))
                    })
                    .when(!selected, |tab| {
                        tab.text_color(rgb(ON_SURFACE_VARIANT))
                            .hover(|tab| tab.bg(rgb(SURFACE_HIGH)))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.detail_page = page;
                        cx.notify();
                    }))
                    .child(label),
            );
        }
        row
    }

    fn hero(&self, device: &DeviceSnapshot) -> impl IntoElement {
        let (container, content, eyebrow, headline, description) = if !device.connected {
            (
                SURFACE_HIGH,
                ON_SURFACE,
                self.t("GETTING READY", "准备连接"),
                self.t("Looking for your XM5.", "正在查找你的 XM5。"),
                self.t(
                    "Turn on your headphones and connect them in system Bluetooth settings.",
                    "请打开耳机，并在系统蓝牙设置中连接。",
                ),
            )
        } else {
            match device.noise_mode {
                NoiseMode::NoiseCancelling => (
                    PRIMARY_CONTAINER,
                    ON_PRIMARY_CONTAINER,
                    self.t("DEEP FOCUS", "沉浸专注"),
                    self.t("Quiet, shaped around you.", "为你隔绝喧嚣。"),
                    self.t(
                        "Noise cancelling is filtering the room in real time.",
                        "降噪正在实时过滤周围声音。",
                    ),
                ),
                NoiseMode::Transparency | NoiseMode::Ambient => (
                    SECONDARY_CONTAINER,
                    ON_SECONDARY_CONTAINER,
                    self.t("STAY AWARE", "感知环境"),
                    self.t("Let the world back in.", "让外界声音自然进入。"),
                    self.t(
                        "Ambient sound is balanced for conversation and movement.",
                        "环境声已针对交谈和行走进行平衡。",
                    ),
                ),
                NoiseMode::Adaptive => (
                    TERTIARY_CONTAINER,
                    ON_TERTIARY_CONTAINER,
                    self.t("ADAPTIVE SOUND", "自适应声音"),
                    self.t("The right balance, automatically.", "自动找到合适的平衡。"),
                    self.t(
                        "Your headphones adjust as your surroundings change.",
                        "耳机会随周围环境变化自动调节。",
                    ),
                ),
                NoiseMode::Off => (
                    SURFACE_HIGH,
                    ON_SURFACE,
                    self.t("DIRECT LISTENING", "自然聆听"),
                    self.t("Just the music.", "只留下音乐。"),
                    self.t(
                        "Sound control is off for a natural listening experience.",
                        "声音控制已关闭，呈现自然聆听体验。",
                    ),
                ),
            }
        };
        let battery_fill = device.battery_percent as f32 / 100.0;

        div()
            .h(px(220.))
            .w_full()
            .rounded_3xl()
            .bg(rgb(container))
            .text_color(rgb(content))
            .p_6()
            .flex()
            .items_end()
            .justify_between()
            .child(
                div()
                    .flex_1()
                    .child(div().text_sm().font_weight(FontWeight::BOLD).child(eyebrow))
                    .child(
                        div()
                            .mt_3()
                            .text_3xl()
                            .font_weight(FontWeight::BOLD)
                            .child(headline),
                    )
                    .child(
                        div()
                            .mt_3()
                            .text_color(rgb(content).opacity(0.76))
                            .child(description),
                    ),
            )
            .child(
                div()
                    .w(px(190.))
                    .ml_8()
                    .p_5()
                    .rounded_3xl()
                    .bg(rgb(CANVAS).opacity(0.22))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(content).opacity(0.72))
                            .child(self.t("BATTERY", "电量")),
                    )
                    .child(div().mt_1().text_3xl().font_weight(FontWeight::BOLD).child(
                        if device.connected {
                            format!("{}%", device.battery_percent)
                        } else {
                            "—".into()
                        },
                    ))
                    .child(
                        div()
                            .mt_4()
                            .h(px(10.))
                            .w_full()
                            .rounded_full()
                            .overflow_hidden()
                            .bg(rgb(CANVAS).opacity(0.22))
                            .child(
                                div()
                                    .h_full()
                                    .w(gpui_kit::relative(battery_fill))
                                    .rounded_full()
                                    .bg(rgb(content)),
                            ),
                    ),
            )
    }

    fn mode_controls(&self, device: &DeviceSnapshot, cx: &mut Context<Self>) -> impl IntoElement {
        let mut modes = div()
            .flex()
            .gap_1()
            .mt_3()
            .p_1()
            .rounded_full()
            .bg(rgb(CANVAS).opacity(0.46));
        for mode in device.capabilities.noise_modes.iter().copied() {
            let selected = mode == device.noise_mode;
            let device_kind = device.kind;
            modes = modes.child(
                div()
                    .id(SharedString::from(format!("mode-{mode:?}")))
                    .flex_1()
                    .px_3()
                    .py_2()
                    .rounded_full()
                    .cursor_pointer()
                    .text_sm()
                    .text_center()
                    .font_weight(FontWeight::SEMIBOLD)
                    .when(selected, |e| e.bg(rgb(PRIMARY)).text_color(rgb(ON_PRIMARY)))
                    .when(!selected, |e| {
                        e.text_color(rgb(ON_SURFACE_VARIANT))
                            .hover(|e| e.bg(rgb(SURFACE_HIGH)).text_color(rgb(ON_SURFACE)))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.devices[this.selected].set_noise_mode(mode);
                        match device_kind {
                            DeviceKind::SonyWh1000Xm5 => {
                                let mode = match mode {
                                    NoiseMode::Off => Some(ListeningMode::Off),
                                    NoiseMode::NoiseCancelling => {
                                        Some(ListeningMode::NoiseCancelling)
                                    }
                                    NoiseMode::Ambient | NoiseMode::Transparency => {
                                        Some(ListeningMode::Ambient)
                                    }
                                    NoiseMode::Adaptive => None,
                                };
                                if let Some(mode) = mode {
                                    this.send_setting(Setting::Mode(mode));
                                }
                            }
                            DeviceKind::AirPodsPro2 => {
                                let mode = match mode {
                                    NoiseMode::Off => AirPodsListeningMode::Off,
                                    NoiseMode::NoiseCancelling => {
                                        AirPodsListeningMode::NoiseCancelling
                                    }
                                    NoiseMode::Transparency | NoiseMode::Ambient => {
                                        AirPodsListeningMode::Transparency
                                    }
                                    NoiseMode::Adaptive => AirPodsListeningMode::Adaptive,
                                };
                                this.send_airpods_setting(AirPodsSetting::Mode(mode));
                            }
                        }
                        cx.notify();
                    }))
                    .child(self.noise_mode_label(mode)),
            );
        }
        self.panel(
            self.t("Noise control", "声音控制"),
            self.t("Shape what you hear around you", "调节你听到的周围声音"),
            modes,
        )
    }

    fn round_action(
        id: &'static str,
        label: &'static str,
        delta: i8,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .size(px(36.))
            .rounded_full()
            .bg(rgb(SECONDARY_CONTAINER))
            .text_color(rgb(ON_SECONDARY_CONTAINER))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_lg()
            .hover(|e| e.bg(rgb(PRIMARY_CONTAINER)))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.devices[this.selected].adjust_ambient(delta);
                if this.devices[this.selected].kind == DeviceKind::SonyWh1000Xm5 {
                    let level = this.devices[this.selected].ambient_level;
                    this.send_setting(Setting::AmbientLevel(level));
                }
                cx.notify();
            }))
            .child(label)
    }

    fn ambient_control(&self, device: &DeviceSnapshot, cx: &mut Context<Self>) -> impl IntoElement {
        let fill = device.ambient_level as f32 / 20.0;
        self.panel(
            self.t("Ambient level", "环境声等级"),
            self.t(
                "Fine tune how much sound gets through",
                "精细调节外界声音的通透程度",
            ),
            div()
                .mt_3()
                .flex()
                .items_center()
                .gap_3()
                .child(Self::round_action("ambient-minus", "−", -1, cx))
                .child(
                    div()
                        .flex_1()
                        .h(px(10.))
                        .rounded_full()
                        .bg(rgb(SURFACE_HIGH))
                        .overflow_hidden()
                        .child(
                            div()
                                .h_full()
                                .w(gpui_kit::relative(fill))
                                .rounded_full()
                                .bg(rgb(PRIMARY)),
                        ),
                )
                .child(Self::round_action("ambient-plus", "+", 1, cx))
                .child(
                    div()
                        .px_3()
                        .py_2()
                        .rounded_full()
                        .bg(rgb(PRIMARY_CONTAINER))
                        .text_color(rgb(ON_PRIMARY_CONTAINER))
                        .text_sm()
                        .font_weight(FontWeight::BOLD)
                        .child(format!("{} / 20", device.ambient_level)),
                ),
        )
    }

    fn feature_card(
        &self,
        title: &'static str,
        description: &'static str,
        enabled: bool,
        toggle: impl Fn(&mut Self) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .p_3()
            .rounded_2xl()
            .bg(if enabled {
                rgb(SECONDARY_CONTAINER)
            } else {
                rgb(SURFACE_CONTAINER)
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_px()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(ON_SURFACE_VARIANT))
                            .child(description),
                    ),
            )
            .child(
                div()
                    .id(SharedString::from(format!("feature-{title}")))
                    .w(px(48.))
                    .h(px(28.))
                    .p(px(3.))
                    .rounded_full()
                    .cursor_pointer()
                    .flex()
                    .bg(if enabled { rgb(PRIMARY) } else { rgb(OUTLINE) })
                    .when(enabled, |e| e.justify_end())
                    .when(!enabled, |e| e.justify_start())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        toggle(this);
                        cx.notify();
                    }))
                    .child(div().size(px(22.)).rounded_full().bg(if enabled {
                        rgb(ON_PRIMARY)
                    } else {
                        rgb(SURFACE_HIGH)
                    })),
            )
    }

    fn choice_button(
        id: SharedString,
        label: SharedString,
        selected: bool,
        action: impl Fn(&mut Self) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .px_3()
            .py_2()
            .rounded_full()
            .cursor_pointer()
            .text_sm()
            .font_weight(FontWeight::SEMIBOLD)
            .when(selected, |item| {
                item.bg(rgb(PRIMARY)).text_color(rgb(ON_PRIMARY))
            })
            .when(!selected, |item| {
                item.bg(rgb(SURFACE_HIGH))
                    .text_color(rgb(ON_SURFACE_VARIANT))
                    .hover(|item| item.text_color(rgb(ON_SURFACE)))
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                action(this);
                cx.notify();
            }))
            .child(label)
    }

    fn sony_over_ear_hero(&self, device: &DeviceSnapshot) -> impl IntoElement {
        let (wearing, wearing_color, wearing_icon) = match self.sony.wearing_status {
            Some(WearingStatus::Normal) => (self.t("Worn", "已佩戴"), AIRPODS_WORN, IconName::Ear),
            Some(WearingStatus::LeftRemoved | WearingStatus::RightRemoved) => (
                self.t("Partially removed", "部分摘下"),
                AIRPODS_CASE,
                IconName::EarOff,
            ),
            Some(WearingStatus::BothRemoved) => {
                (self.t("Removed", "已摘下"), OUTLINE, IconName::EarOff)
            }
            Some(WearingStatus::Illegal) => (
                self.t("Status unavailable", "状态异常"),
                OUTLINE,
                IconName::CircleOff,
            ),
            None => (self.t("Reading…", "读取中…"), OUTLINE, IconName::Ear),
        };
        let battery = if self.sony.charging {
            format!(
                "{}% · {}",
                device.battery_percent,
                self.t("Charging", "充电中")
            )
        } else {
            format!("{}%", device.battery_percent)
        };
        let codec = self
            .sony
            .codec
            .clone()
            .unwrap_or_else(|| self.t("Reading…", "读取中…").into());

        div()
            .w_full()
            .h(px(250.))
            .rounded_3xl()
            .overflow_hidden()
            .bg(rgb(SURFACE_CONTAINER))
            .p_4()
            .flex()
            .child(
                div()
                    .w(gpui_kit::relative(0.57))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        img(sony_wh1000xm5_image())
                            .size(px(225.))
                            .object_fit(ObjectFit::Contain),
                    ),
            )
            .child(
                div()
                    .w(gpui_kit::relative(0.43))
                    .h_full()
                    .pl_4()
                    .border_l_1()
                    .border_color(rgb(OUTLINE).opacity(0.24))
                    .flex()
                    .flex_col()
                    .justify_center()
                    .child(self.sony_status_row(
                        self.t("Battery", "整机电量"),
                        battery.into(),
                        PRIMARY,
                        if self.sony.charging {
                            IconName::BatteryCharging
                        } else {
                            IconName::Battery
                        },
                        Some(device.battery_percent as f32 / 100.0),
                    ))
                    .child(self.sony_status_row(
                        self.t("Wearing status", "佩戴状态"),
                        wearing.into(),
                        wearing_color,
                        wearing_icon,
                        None,
                    ))
                    .child(self.sony_status_row(
                        self.t("Sound mode", "声音模式"),
                        self.noise_mode_label(device.noise_mode).into(),
                        SONY_MODE,
                        IconName::AudioLines,
                        None,
                    ))
                    .child(self.sony_status_row(
                        self.t("Codec", "编解码器"),
                        codec.into(),
                        SONY_CODEC,
                        IconName::Cpu,
                        None,
                    )),
            )
    }

    fn sony_status_row(
        &self,
        label: &'static str,
        value: SharedString,
        color: u32,
        icon: IconName,
        progress: Option<f32>,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_2()
            .py_1()
            .border_b_1()
            .border_color(rgb(OUTLINE).opacity(0.14))
            .child(
                div()
                    .size(px(34.))
                    .rounded_full()
                    .border_2()
                    .border_color(rgb(color).opacity(0.76))
                    .bg(rgb(color).opacity(0.10))
                    .text_color(rgb(color))
                    .text_sm()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(icon).text_color(rgb(color))),
            )
            .child(
                div()
                    .flex_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(ON_SURFACE_VARIANT))
                            .child(label),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_base()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(color))
                            .child(value),
                    )
                    .when_some(progress, |content, progress| {
                        content.child(
                            div()
                                .mt_1()
                                .h(px(4.))
                                .w_full()
                                .rounded_full()
                                .overflow_hidden()
                                .bg(rgb(OUTLINE).opacity(0.18))
                                .child(
                                    div()
                                        .h_full()
                                        .w(gpui_kit::relative(progress))
                                        .rounded_full()
                                        .bg(rgb(color)),
                                ),
                        )
                    }),
            )
    }

    fn sony_info_panel(&self) -> impl IntoElement {
        let info = [
            (
                self.t("Firmware", "固件版本"),
                self.sony
                    .firmware
                    .as_deref()
                    .unwrap_or(self.t("Reading…", "读取中…")),
            ),
            (
                self.t("Codec", "音频编码"),
                self.sony
                    .codec
                    .as_deref()
                    .unwrap_or(self.t("Reading…", "读取中…")),
            ),
            (
                self.t("Power", "电源"),
                if self.sony.charging {
                    self.t("Charging", "充电中")
                } else {
                    self.t("On battery", "使用电池")
                },
            ),
        ];
        let mut cards = div().grid().grid_cols(3).gap_3().mt_5();
        for (label, value) in info {
            cards = cards.child(
                div()
                    .p_4()
                    .rounded_2xl()
                    .bg(rgb(SURFACE_HIGH))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(ON_SURFACE_VARIANT))
                            .child(label),
                    )
                    .child(
                        div()
                            .mt_2()
                            .text_xl()
                            .font_weight(FontWeight::BOLD)
                            .child(value.to_owned()),
                    ),
            );
        }
        self.panel(
            self.t("Device status", "设备状态"),
            self.t(
                "Live values reported by the headphones",
                "耳机实时回报的状态",
            ),
            cards,
        )
    }

    fn airpods_hero(&self) -> impl IntoElement {
        let (left_state, right_state) = self
            .airpods_wearing_by_side()
            .map_or((None, None), |(left, right)| (Some(left), Some(right)));

        div()
            .w_full()
            .h(px(300.))
            .rounded_3xl()
            .overflow_hidden()
            .bg(rgb(SURFACE_CONTAINER))
            .p_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(PRIMARY))
                                    .child(self.t("LIVE AAP STATUS", "AAP 实时状态")),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .text_sm()
                                    .text_color(rgb(ON_SURFACE_VARIANT))
                                    .child(self.t(
                                        "Battery and wearing state at a glance",
                                        "一眼查看电量与佩戴状态",
                                    )),
                            ),
                    )
                    .child(Self::chip(self.t("Connected", "已连接"), AIRPODS_WORN)),
            )
            .child(
                div()
                    .mt_3()
                    .flex()
                    .items_end()
                    .justify_between()
                    .child(self.airpod_status_node(
                        self.t("Left AirPod", "左耳"),
                        airpods_left_image(),
                        self.airpods.battery.left,
                        self.airpods.battery.left_charging,
                        left_state,
                    ))
                    .child(self.airpods_case_node())
                    .child(self.airpod_status_node(
                        self.t("Right AirPod", "右耳"),
                        airpods_right_image(),
                        self.airpods.battery.right,
                        self.airpods.battery.right_charging,
                        right_state,
                    )),
            )
    }

    fn airpod_status_node(
        &self,
        label: &'static str,
        image: Arc<GpuiImage>,
        level: Option<u8>,
        charging: bool,
        state: Option<PodState>,
    ) -> impl IntoElement {
        let color = if charging {
            AIRPODS_CHARGING
        } else {
            match state {
                Some(PodState::InEar) => AIRPODS_WORN,
                Some(PodState::InCase) => AIRPODS_CASE,
                Some(PodState::OutOfEar | PodState::Unknown(_)) | None => OUTLINE,
            }
        };
        let icon = if charging {
            IconName::BatteryCharging
        } else {
            match state {
                Some(PodState::InEar) => IconName::Ear,
                Some(PodState::OutOfEar) => IconName::EarOff,
                Some(PodState::InCase) => IconName::PackageOpen,
                Some(PodState::Unknown(_)) | None => IconName::CircleOff,
            }
        };
        let status = if charging {
            match state {
                Some(PodState::InCase) => self.t("In case · Charging", "盒中 · 充电中"),
                _ => self.t("Charging", "充电中"),
            }
        } else {
            state.map_or(
                self.t("Reading status…", "正在读取状态…"),
                |state| self.pod_state_label(state),
            )
        };

        div()
            .w(px(150.))
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .relative()
                    .size(px(112.))
                    .rounded_full()
                    .border_2()
                    .border_color(rgb(color).opacity(0.78))
                    .bg(rgb(CANVAS).opacity(0.26))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(img(image).size(px(100.)).object_fit(ObjectFit::Contain))
                    .child(
                        div()
                            .absolute()
                            .top(px(4.))
                            .right(px(4.))
                            .size(px(30.))
                            .rounded_full()
                            .border_2()
                            .border_color(rgb(SURFACE_CONTAINER))
                            .bg(rgb(color))
                            .text_color(rgb(CANVAS))
                            .text_sm()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(Icon::new(icon).text_color(rgb(CANVAS))),
                    ),
            )
            .child(div().mt_2().font_weight(FontWeight::SEMIBOLD).child(label))
            .child(
                div()
                    .mt_1()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(color))
                    .child(status),
            )
            .child(
                div()
                    .mt_1()
                    .text_xl()
                    .font_weight(FontWeight::BOLD)
                    .child(level.map_or_else(|| "—".into(), |level| format!("{level}%"))),
            )
    }

    fn airpods_case_node(&self) -> impl IntoElement {
        let level = self.airpods.battery.case;
        let color = if self.airpods.battery.case_charging {
            AIRPODS_CHARGING
        } else {
            AIRPODS_CASE
        };
        div()
            .w(px(180.))
            .flex()
            .flex_col()
            .items_center()
            .child(
                img(airpods_case_image())
                    .size(px(145.))
                    .object_fit(ObjectFit::Contain),
            )
            .child(
                div()
                    .mt_neg_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .rounded_full()
                    .bg(rgb(color).opacity(0.14))
                    .text_color(rgb(color))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(Icon::new(IconName::Battery).text_color(rgb(color)))
                    .child(self.t("Case", "充电盒"))
                    .child(level.map_or_else(|| "—".into(), |level| format!("{level}%"))),
            )
    }

    fn airpods_wearing_by_side(&self) -> Option<(PodState, PodState)> {
        let wearing = self.airpods.wearing?;
        match self.airpods.battery.primary? {
            AirPodsSide::Left => Some((wearing.primary, wearing.secondary)),
            AirPodsSide::Right => Some((wearing.secondary, wearing.primary)),
        }
    }

    fn pod_state_label(&self, state: PodState) -> &'static str {
        match state {
            PodState::InEar => self.t("In ear", "已佩戴"),
            PodState::OutOfEar => self.t("Out of ear", "已摘下"),
            PodState::InCase => self.t("In case", "盒中"),
            PodState::Unknown(_) => self.t("Unknown", "未知"),
        }
    }

    fn airpods_settings_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let options = [
            (AirPodsMicrophone::Automatic, self.t("Automatic", "自动")),
            (AirPodsMicrophone::Left, self.t("Always left", "始终左耳")),
            (AirPodsMicrophone::Right, self.t("Always right", "始终右耳")),
        ];
        let mut controls = div().flex().flex_wrap().gap_2().mt_3();
        for (microphone, label) in options {
            controls = controls.child(Self::choice_button(
                SharedString::from(format!("airpods-microphone-{microphone:?}")),
                SharedString::from(label),
                self.airpods.microphone == Some(microphone),
                move |this| {
                    this.airpods.microphone = Some(microphone);
                    this.send_airpods_setting(AirPodsSetting::Microphone(microphone));
                },
                cx,
            ));
        }
        self.panel(
            self.t("Microphone", "麦克风"),
            self.t(
                "Choose which AirPod provides microphone input",
                "选择由哪只 AirPods 提供麦克风输入",
            ),
            controls,
        )
    }

    fn eq_action(index: usize, delta: i8, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id(SharedString::from(format!("eq-{index}-{delta}")))
            .size(px(32.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .bg(rgb(SECONDARY_CONTAINER))
            .text_color(rgb(ON_SECONDARY_CONTAINER))
            .on_click(cx.listener(move |this, _, _, cx| {
                let Some(mut equalizer) = this.sony.equalizer else {
                    return;
                };
                equalizer.bands[index] =
                    equalizer.bands[index].saturating_add(delta).clamp(-10, 10);
                equalizer.preset = EqPreset::Manual;
                this.sony.equalizer = Some(equalizer);
                this.devices[0].equalizer_enabled = true;
                this.send_setting(Setting::EqualizerBands(equalizer.bands));
                cx.notify();
            }))
            .child(if delta < 0 { "−" } else { "+" })
    }

    fn equalizer_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let equalizer = self.sony.equalizer.unwrap_or(Equalizer {
            preset: EqPreset::Off,
            bands: [0; 6],
        });
        let mut presets = div().flex().flex_wrap().gap_2().mt_3();
        for preset in EqPreset::CHOICES {
            let selected = equalizer.preset == preset;
            presets = presets.child(Self::choice_button(
                SharedString::from(format!("eq-preset-{preset:?}")),
                self.preset_label(preset).into(),
                selected,
                move |this| {
                    if let Some(equalizer) = &mut this.sony.equalizer {
                        equalizer.preset = preset;
                    }
                    this.devices[0].equalizer_enabled = preset != EqPreset::Off;
                    this.send_setting(Setting::EqualizerPreset(preset));
                },
                cx,
            ));
        }

        let labels = [
            self.t("Clear Bass", "清晰低音"),
            "400 Hz",
            "1 kHz",
            "2.5 kHz",
            "6.3 kHz",
            "16 kHz",
        ];
        let mut bands = div().grid().grid_cols(3).gap_2().mt_3();
        for (index, label) in labels.into_iter().enumerate() {
            bands = bands.child(
                div()
                    .p_2()
                    .rounded_2xl()
                    .bg(rgb(SURFACE_HIGH))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(ON_SURFACE_VARIANT))
                            .child(label),
                    )
                    .child(
                        div()
                            .mt_2()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(Self::eq_action(index, -1, cx))
                            .child(
                                div()
                                    .font_weight(FontWeight::BOLD)
                                    .child(format!("{:+}", equalizer.bands[index])),
                            )
                            .child(Self::eq_action(index, 1, cx)),
                    ),
            );
        }

        self.panel(
            self.t("Equalizer", "均衡器"),
            self.t(
                "Five bands and Clear Bass · changes switch to Manual",
                "五段均衡与清晰低音 · 调节后切换为手动模式",
            ),
            div().child(presets).child(bands),
        )
    }

    fn sony_settings_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut auto_off = div().flex().flex_wrap().gap_2().mt_2();
        for choice in AutoOff::XM5_CHOICES {
            let label = match choice {
                AutoOff::WhenTakenOff => self.t("When taken off", "摘下耳机时"),
                AutoOff::Never => self.t("Never", "永不"),
                AutoOff::Minutes(_) => self.t("After a delay", "延时后"),
            };
            auto_off = auto_off.child(Self::choice_button(
                SharedString::from(format!("auto-off-{choice:?}")),
                label.into(),
                self.sony.auto_off == Some(choice),
                move |this| {
                    this.sony.auto_off = Some(choice);
                    this.send_setting(Setting::AutoOff(choice));
                },
                cx,
            ));
        }

        let mut noise_button = div().flex().flex_wrap().gap_2().mt_2();
        for choice in NoiseButton::ALL {
            let label = match choice {
                NoiseButton::NoiseCancellingAmbientOff => {
                    self.t("ANC · Ambient · Off", "降噪 · 环境声 · 关闭")
                }
                NoiseButton::NoiseCancellingAmbient => self.t("ANC · Ambient", "降噪 · 环境声"),
                NoiseButton::NoiseCancellingOff => self.t("ANC · Off", "降噪 · 关闭"),
                NoiseButton::AmbientOff => self.t("Ambient · Off", "环境声 · 关闭"),
            };
            noise_button = noise_button.child(Self::choice_button(
                SharedString::from(format!("noise-button-{choice:?}")),
                label.into(),
                self.sony.noise_button == Some(choice),
                move |this| {
                    this.sony.noise_button = Some(choice);
                    this.send_setting(Setting::NoiseButton(choice));
                },
                cx,
            ));
        }

        let mut sensitivity = div().flex().flex_wrap().gap_2().mt_2();
        for choice in VoiceSensitivity::ALL {
            let label = match choice {
                VoiceSensitivity::Auto => self.t("Automatic", "自动"),
                VoiceSensitivity::High => self.t("High", "高"),
                VoiceSensitivity::Low => self.t("Low", "低"),
            };
            sensitivity = sensitivity.child(Self::choice_button(
                SharedString::from(format!("voice-sensitivity-{choice:?}")),
                label.into(),
                self.sony.voice_sensitivity == Some(choice),
                move |this| {
                    this.sony.voice_sensitivity = Some(choice);
                    this.send_setting(Setting::VoiceSensitivity(choice));
                },
                cx,
            ));
        }

        let mut end = div().flex().flex_wrap().gap_2().mt_2();
        for choice in SpeakToChatEnd::ALL {
            let label = match choice {
                SpeakToChatEnd::Short => self.t("Short", "短"),
                SpeakToChatEnd::Standard => self.t("Standard", "标准"),
                SpeakToChatEnd::Long => self.t("Long", "长"),
                SpeakToChatEnd::Never => self.t("Never", "永不"),
            };
            end = end.child(Self::choice_button(
                SharedString::from(format!("speak-end-{choice:?}")),
                label.into(),
                self.sony.speak_to_chat_end == Some(choice),
                move |this| {
                    this.sony.speak_to_chat_end = Some(choice);
                    this.send_setting(Setting::SpeakToChatEnd(choice));
                },
                cx,
            ));
        }

        let section = |label: &'static str, choices| {
            div()
                .mt_3()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(ON_SURFACE_VARIANT))
                        .child(label),
                )
                .child(choices)
        };
        self.panel(
            self.t("Headphone settings", "耳机设置"),
            self.t(
                "Physical controls and Speak-to-Chat behaviour",
                "实体按键与说话聊天行为",
            ),
            div()
                .child(section(self.t("AUTO POWER OFF", "自动关机"), auto_off))
                .child(section(
                    self.t("NC / AMBIENT BUTTON", "降噪 / 环境声按键"),
                    noise_button,
                ))
                .child(section(
                    self.t("VOICE SENSITIVITY", "语音灵敏度"),
                    sensitivity,
                ))
                .child(section(self.t("CLOSE SPEAK-TO-CHAT", "结束说话聊天"), end)),
        )
    }

    fn connections_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut devices = div().flex().flex_col().gap_2().mt_3();
        if self.sony.connected_devices.is_empty() {
            devices = devices.child(
                div()
                    .p_3()
                    .rounded_2xl()
                    .bg(rgb(SURFACE_HIGH))
                    .text_color(rgb(ON_SURFACE_VARIANT))
                    .child(self.t(
                        "No multipoint devices reported yet.",
                        "耳机尚未回报多点连接设备。",
                    )),
            );
        }
        for device in &self.sony.connected_devices {
            let id = device.id.clone();
            devices = devices.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .p_3()
                    .rounded_2xl()
                    .bg(rgb(SURFACE_HIGH))
                    .child(
                        div()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(device.name.clone()),
                            )
                            .child(div().text_sm().text_color(rgb(ON_SURFACE_VARIANT)).child(
                                if device.playing {
                                    self.t("Currently playing", "当前正在播放")
                                } else {
                                    self.t("Connected", "已连接")
                                },
                            )),
                    )
                    .when(!device.playing, |row| {
                        row.child(Self::choice_button(
                            SharedString::from(format!("play-from-{id}")),
                            self.t("Play from here", "从此设备播放").into(),
                            false,
                            move |this| this.send_setting(Setting::PlayFrom(id.clone())),
                            cx,
                        ))
                    }),
            );
        }

        self.panel(
            self.t("Multipoint", "多点连接"),
            self.t(
                "Choose which connected device provides audio",
                "选择由哪台已连接设备播放音频",
            ),
            devices,
        )
    }

    fn power_off_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        self.panel(
            self.t("Power", "电源"),
            self.t(
                "The headphones can be turned back on with their physical button",
                "关闭后可使用耳机实体按键重新开机",
            ),
            div()
                .id("power-off-headphones")
                .mt_3()
                .px_4()
                .py_2()
                .rounded_full()
                .cursor_pointer()
                .bg(rgb(TERTIARY_CONTAINER))
                .text_color(rgb(ON_TERTIARY_CONTAINER))
                .font_weight(FontWeight::SEMIBOLD)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.send_setting(Setting::PowerOff);
                    this.backend_status = match this.language {
                        Language::English => "Power-off command sent to WH-1000XM5.".into(),
                        Language::SimplifiedChinese => "已向 WH-1000XM5 发送关机命令。".into(),
                    };
                    cx.notify();
                }))
                .child(self.t("Power off headphones", "关闭耳机")),
        )
    }

    fn settings_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let language = div()
            .flex()
            .gap_2()
            .mt_5()
            .child(Self::choice_button(
                "language-en".into(),
                "English".into(),
                self.language == Language::English,
                |this| this.set_language(Language::English),
                cx,
            ))
            .child(Self::choice_button(
                "language-zh-cn".into(),
                "简体中文".into(),
                self.language == Language::SimplifiedChinese,
                |this| this.set_language(Language::SimplifiedChinese),
                cx,
            ));

        div()
            .flex_1()
            .h_full()
            .overflow_y_scrollbar()
            .p_5()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(PRIMARY))
                    .child(self.t("PREFERENCES", "偏好设置")),
            )
            .child(
                div()
                    .mt_1()
                    .text_2xl()
                    .font_weight(FontWeight::BOLD)
                    .child(self.t("Settings", "设置")),
            )
            .child(
                div()
                    .mt_2()
                    .text_color(rgb(ON_SURFACE_VARIANT))
                    .child(self.t(
                        "Personalize iHeadphone without changing your headphone settings.",
                        "个性化 iHeadphone，不会修改耳机本身的设置。",
                    )),
            )
            .child(div().mt_4().child(self.panel(
                self.t("Language", "语言"),
                self.t(
                    "Choose the language used throughout the application",
                    "选择整个应用使用的语言",
                ),
                language,
            )))
            .child(
                div().mt_3().child(
                    self.panel(
                        self.t("About", "关于"),
                        self.t(
                            "Native GPUI controller for Sony headphones and AirPods",
                            "使用 GPUI 构建的 Sony 耳机与 AirPods 原生控制器",
                        ),
                        div()
                            .mt_5()
                            .p_4()
                            .rounded_2xl()
                            .bg(rgb(SURFACE_HIGH))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("iHeadphone 0.1.0"),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .text_sm()
                                    .text_color(rgb(ON_SURFACE_VARIANT))
                                    .child(self.t(
                                        "Linux and macOS desktop preview",
                                        "Linux 与 macOS 桌面预览版",
                                    )),
                            ),
                    ),
                ),
            )
    }

    fn set_language(&mut self, language: Language) {
        self.language = language;
        language.save();
        self.backend_status = if self.requests.is_none() {
            self.t(
                "Controls are running in preview mode.",
                "控制功能正在预览模式下运行。",
            )
            .into()
        } else if self.devices[0].connected {
            self.t(
                "WH-1000XM5 hardware controls are live.",
                "WH-1000XM5 硬件控制已启用。",
            )
            .into()
        } else {
            self.t("Searching for WH-1000XM5…", "正在查找 WH-1000XM5…")
                .into()
        };
    }

    fn panel(
        &self,
        title: &'static str,
        subtitle: &'static str,
        content: impl IntoElement,
    ) -> impl IntoElement {
        div()
            .flex_1()
            .p_3()
            .rounded_2xl()
            .bg(rgb(SURFACE_CONTAINER))
            .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
            .child(
                div()
                    .mt_1()
                    .text_xs()
                    .text_color(rgb(ON_SURFACE_VARIANT))
                    .child(subtitle),
            )
            .child(content)
    }
}

impl Render for HeadphoneApp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let device = self.selected_device().clone();
        let selected = self.selected;
        let page = self.detail_page;
        let sony_over_ear = device.kind.uses_sony_over_ear_layout();
        let show_features = (page == DetailPage::Overview
            && device.kind != DeviceKind::AirPodsPro2)
            || (page == DetailPage::Device && device.kind == DeviceKind::AirPodsPro2);
        let mut features = div().grid().grid_cols(2).gap_2().mt_2();
        if device.capabilities.equalizer && device.kind != DeviceKind::SonyWh1000Xm5 {
            features = features.child(self.feature_card(
                self.t("Equalizer", "均衡器"),
                self.t("Warm detail preset", "温暖细腻预设"),
                device.equalizer_enabled,
                move |this| {
                    this.devices[selected].equalizer_enabled =
                        !this.devices[selected].equalizer_enabled
                },
                cx,
            ));
        }
        if device.capabilities.dsee {
            features = features.child(self.feature_card(
                "DSEE Extreme",
                self.t("Restore compressed detail", "恢复压缩音频的细节"),
                device.dsee_enabled,
                move |this| {
                    let enabled = !this.devices[selected].dsee_enabled;
                    this.devices[selected].dsee_enabled = enabled;
                    if this.devices[selected].kind == DeviceKind::SonyWh1000Xm5 {
                        this.send_setting(Setting::Feature(Feature::Dsee, enabled));
                    }
                },
                cx,
            ));
        }
        if device.capabilities.speak_to_chat {
            features = features.child(self.feature_card(
                self.t("Speak-to-Chat", "说话聊天"),
                self.t("Pause when you start speaking", "检测到说话时暂停播放"),
                device.speak_to_chat,
                move |this| {
                    let enabled = !this.devices[selected].speak_to_chat;
                    this.devices[selected].speak_to_chat = enabled;
                    if this.devices[selected].kind == DeviceKind::SonyWh1000Xm5 {
                        this.send_setting(Setting::Feature(Feature::SpeakToChat, enabled));
                    }
                },
                cx,
            ));
        }
        if device.kind == DeviceKind::SonyWh1000Xm5 {
            features = features.child(self.feature_card(
                self.t("Focus on Voice", "聚焦人声"),
                self.t(
                    "Prioritize speech while Ambient Sound is active",
                    "环境声开启时优先突出人声",
                ),
                self.sony.focus_on_voice,
                |this| {
                    let enabled = !this.sony.focus_on_voice;
                    this.sony.focus_on_voice = enabled;
                    this.send_setting(Setting::FocusOnVoice(enabled));
                },
                cx,
            ));
            features = features.child(self.feature_card(
                self.t("Pause when taken off", "摘下时暂停"),
                self.t(
                    "Resume automatically when you put them back on",
                    "重新佩戴时自动恢复播放",
                ),
                self.sony.pause_when_taken_off,
                |this| {
                    let enabled = !this.sony.pause_when_taken_off;
                    this.sony.pause_when_taken_off = enabled;
                    this.send_setting(Setting::Feature(Feature::PauseWhenTakenOff, enabled));
                },
                cx,
            ));
        }
        if device.capabilities.conversation_awareness {
            features = features.child(self.feature_card(
                self.t("Conversation awareness", "对话感知"),
                self.t("Lower media when you speak", "说话时降低媒体音量"),
                device.conversation_awareness,
                move |this| {
                    let enabled = !this.devices[selected].conversation_awareness;
                    this.devices[selected].conversation_awareness = enabled;
                    if this.devices[selected].kind == DeviceKind::AirPodsPro2 {
                        this.send_airpods_setting(AirPodsSetting::ConversationAwareness(enabled));
                    }
                },
                cx,
            ));
        }
        if device.kind == DeviceKind::AirPodsPro2 {
            features = features.child(self.feature_card(
                self.t("Automatic ear detection", "自动入耳检测"),
                self.t(
                    "Pause when both AirPods are removed",
                    "摘下两只 AirPods 时暂停",
                ),
                self.airpods.ear_detection,
                |this| {
                    let enabled = !this.airpods.ear_detection;
                    this.airpods.ear_detection = enabled;
                    this.send_airpods_setting(AirPodsSetting::EarDetection(enabled));
                },
                cx,
            ));
            features = features.child(self.feature_card(
                self.t("Noise cancellation with one AirPod", "单只 AirPods 降噪"),
                self.t(
                    "Allow ANC while wearing one earbud",
                    "仅佩戴一只耳机时也允许降噪",
                ),
                self.airpods.one_bud_anc,
                |this| {
                    let enabled = !this.airpods.one_bud_anc;
                    this.airpods.one_bud_anc = enabled;
                    this.send_airpods_setting(AirPodsSetting::OneBudNoiseCancelling(enabled));
                },
                cx,
            ));
        }

        div()
            .size_full()
            .flex()
            .bg(rgb(CANVAS))
            .text_color(rgb(ON_SURFACE))
            .child(self.sidebar(cx))
            .when(self.show_settings, |root| {
                root.child(self.settings_view(cx))
            })
            .when(!self.show_settings, |root| {
                root.child(
                    div()
                        .flex_1()
                        .h_full()
                        .overflow_y_scrollbar()
                        .p_4()
                        .child(self.app_header(&device))
                        .child(self.detail_tabs(cx))
                        .when(page == DetailPage::Overview && sony_over_ear, |content| {
                            content.child(self.sony_over_ear_hero(&device)).child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .mt_2()
                                    .child(self.mode_controls(&device, cx))
                                    .when(device.capabilities.ambient_level, |row| {
                                        row.child(self.ambient_control(&device, cx))
                                    }),
                            )
                        })
                        .when(
                            page == DetailPage::Overview && device.kind == DeviceKind::AirPodsPro2,
                            |content| {
                                content
                                    .child(self.airpods_hero())
                                    .child(div().mt_3().child(self.mode_controls(&device, cx)))
                            },
                        )
                        .when(
                            page == DetailPage::Overview
                                && !sony_over_ear
                                && device.kind != DeviceKind::AirPodsPro2,
                            |content| content.child(self.hero(&device)),
                        )
                        .when(page == DetailPage::Sound && sony_over_ear, |content| {
                            content
                                .child(
                                    div()
                                        .flex()
                                        .gap_3()
                                        .child(self.mode_controls(&device, cx))
                                        .when(device.capabilities.ambient_level, |row| {
                                            row.child(self.ambient_control(&device, cx))
                                        }),
                                )
                                .child(div().mt_3().child(self.equalizer_panel(cx)))
                        })
                        .when(
                            page == DetailPage::Sound && device.kind == DeviceKind::AirPodsPro2,
                            |content| {
                                content
                                    .child(self.mode_controls(&device, cx))
                                    .child(div().mt_3().child(self.airpods_settings_panel(cx)))
                            },
                        )
                        .when(
                            page == DetailPage::Device && device.kind == DeviceKind::SonyWh1000Xm5,
                            |content| {
                                content.child(
                                    div()
                                        .flex()
                                        .gap_3()
                                        .child(self.sony_settings_panel(cx))
                                        .child(
                                            div()
                                                .flex_1()
                                                .flex()
                                                .flex_col()
                                                .gap_3()
                                                .child(self.sony_info_panel())
                                                .child(self.connections_panel(cx))
                                                .child(self.power_off_panel(cx)),
                                        ),
                                )
                            },
                        )
                        .when(
                            page == DetailPage::Device && device.kind == DeviceKind::AirPodsPro2,
                            |content| content.child(self.airpods_settings_panel(cx)),
                        )
                        .when(show_features, |content| content.child(features)),
                )
            })
    }
}

fn main() {
    gpui_kit::application().run(|cx: &mut App| {
        gpui_kit::init(cx);
        let bounds = Bounds::centered(None, size(px(1240.), px(800.)), cx);
        let demo = std::env::var_os("IHEADPHONE_DEMO").is_some();
        let (requests, events) = if demo {
            (None, None)
        } else {
            let link = iheadphone_transport::start(Some("WH-1000XM5".into()));
            (Some(link.requests), Some(link.events))
        };
        let app: Entity<HeadphoneApp> = cx.new(|_| {
            let mut app = HeadphoneApp::new(requests);
            #[cfg(target_os = "linux")]
            {
                app._media_bridge = Some(MediaBridge::start());
            }
            app
        });

        if let Some(events) = events {
            let model = app.clone();
            cx.spawn(async move |cx| {
                while let Ok(event) = events.recv().await {
                    model.update(cx, |app, cx| {
                        app.apply_bluetooth_event(event);
                        cx.notify();
                    });
                }
            })
            .detach();
        }

        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                app_id: Some(APP_ID.into()),
                icon: window_icon(),
                ..Default::default()
            },
            cx,
            |_, _| app,
        )
        .expect("failed to open iHeadphone window");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use iheadphone_sony_protocol::Battery;

    #[test]
    fn applies_sony_hardware_events_to_the_view_model() {
        let (requests, _) = mpsc::channel();
        let mut app = HeadphoneApp::new(Some(requests));

        app.apply_bluetooth_event(BluetoothEvent::Connected {
            device: BluetoothDevice::SonyWh1000Xm5,
            name: "WH-1000XM5".into(),
        });
        app.apply_bluetooth_event(BluetoothEvent::SonyUpdate(Update::Battery(Battery {
            level: 63,
            charging: false,
        })));
        app.apply_bluetooth_event(BluetoothEvent::SonyUpdate(Update::Mode(
            ListeningMode::Ambient,
        )));
        app.apply_bluetooth_event(BluetoothEvent::SonyUpdate(Update::AmbientLevel(14)));
        app.apply_bluetooth_event(BluetoothEvent::SonyUpdate(Update::Feature(
            Feature::Dsee,
            false,
        )));

        let sony = app.selected_device();
        assert!(sony.connected);
        assert_eq!(sony.battery_percent, 63);
        assert_eq!(sony.noise_mode, NoiseMode::Ambient);
        assert_eq!(sony.ambient_level, 14);
        assert!(!sony.dsee_enabled);
    }

    #[test]
    fn forwards_user_settings_to_the_bluetooth_link() {
        let (requests, receiver) = mpsc::channel();
        let app = HeadphoneApp::new(Some(requests));

        app.send_setting(Setting::Mode(ListeningMode::NoiseCancelling));

        assert_eq!(
            receiver.try_recv(),
            Ok(BluetoothRequest::Sony(Setting::Mode(
                ListeningMode::NoiseCancelling
            )))
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn applies_airpods_hardware_events_and_forwards_modes() {
        let (requests, receiver) = mpsc::channel();
        let mut app = HeadphoneApp::new(Some(requests));

        app.apply_bluetooth_event(BluetoothEvent::Connected {
            device: BluetoothDevice::AirPods,
            name: "AirPods Pro".into(),
        });
        app.apply_bluetooth_event(BluetoothEvent::AirPodsUpdate(AirPodsUpdate::Battery(
            AirPodsBattery {
                primary: Some(AirPodsSide::Left),
                left: Some(80),
                right: Some(60),
                case: Some(42),
                ..AirPodsBattery::default()
            },
        )));
        app.apply_bluetooth_event(BluetoothEvent::AirPodsUpdate(AirPodsUpdate::Mode(
            AirPodsListeningMode::Adaptive,
        )));
        app.apply_bluetooth_event(BluetoothEvent::AirPodsUpdate(AirPodsUpdate::Wearing(
            AirPodsWearing {
                primary: PodState::InCase,
                secondary: PodState::InEar,
            },
        )));

        let airpods = app
            .devices
            .iter()
            .find(|device| device.kind == DeviceKind::AirPodsPro2)
            .unwrap();
        assert!(airpods.connected);
        assert_eq!(airpods.battery_percent, 70);
        assert_eq!(airpods.noise_mode, NoiseMode::Adaptive);
        assert_eq!(
            app.airpods_wearing_by_side(),
            Some((PodState::InCase, PodState::InEar))
        );

        app.send_airpods_setting(AirPodsSetting::Mode(AirPodsListeningMode::Transparency));
        assert_eq!(
            receiver.try_recv(),
            Ok(BluetoothRequest::AirPods(AirPodsSetting::Mode(
                AirPodsListeningMode::Transparency
            )))
        );
    }

    #[test]
    fn translates_interface_labels_to_simplified_chinese() {
        let mut app = HeadphoneApp::new(None);
        app.language = Language::SimplifiedChinese;

        assert_eq!(app.t("Settings", "设置"), "设置");
        assert_eq!(app.noise_mode_label(NoiseMode::Ambient), "环境声");
        assert_eq!(app.preset_label(EqPreset::BassBoost), "低音增强");
    }
}
