//! Native macOS menu-bar companion for the main GPUI window.

#![allow(unexpected_cfgs)]

use std::sync::atomic::{AtomicPtr, Ordering};
use std::{ffi::CString, process::Command, ptr};

use async_channel::Sender;
use block::ConcreteBlock;
use iheadphone_domain::{DeviceKind, NoiseMode};
use iheadphone_sony_protocol::{ConnectedDevice, EqPreset};
use objc::declare::ClassDecl;
use objc::runtime::{BOOL, Class, NO, Object, Sel, YES};
use objc::{Encode, Encoding, class, msg_send, sel, sel_impl};

const ACTION_OPEN: isize = 1;
const ACTION_QUIT: isize = 2;
const ACTION_VOLUME: isize = 3;
const ACTION_MULTIPOINT: isize = 4;
const ACTION_EQUALIZER: isize = 5;
const ACTION_DSEE: isize = 10;
const ACTION_SPEAK: isize = 11;
const ACTION_PAUSE_OFF_HEAD: isize = 12;
const ACTION_TOGGLE_POPOVER: isize = 20;
const ACTION_PRESET_BASE: isize = 200;
const ACTION_PLAY_FROM_BASE: isize = 300;
const ACTION_MODE_OFF: isize = 100;
const ACTION_MODE_ANC: isize = 101;
const ACTION_MODE_TRANSPARENCY: isize = 102;
const ACTION_MODE_ADAPTIVE: isize = 103;
const ACTION_MODE_AMBIENT: isize = 104;
// All geometry is in AppKit points. Never scale the root bounds: native cells
// and layer-backed symbols do not share the same scaling behavior.
const MENU_WIDTH: f64 = 300.0;
const ROW_HEIGHT: f64 = 30.0;

static MENU_POPOVER: AtomicPtr<Object> = AtomicPtr::new(ptr::null_mut());

#[repr(C)]
#[derive(Clone, Copy)]
struct NSPoint {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct NSSize {
    width: f64,
    height: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct NSRect {
    origin: NSPoint,
    size: NSSize,
}

// NSRect uses CGFloat (f64) on supported 64-bit macOS targets.
unsafe impl Encode for NSRect {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("{CGRect={CGPoint=dd}{CGSize=dd}}") }
    }
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect {
        origin: NSPoint { x, y },
        size: NSSize { width, height },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuBarLanguage {
    English,
    SimplifiedChinese,
}

#[derive(Clone, Debug)]
pub struct MenuBarDevice {
    pub kind: DeviceKind,
    pub name: String,
    pub connected: bool,
    pub battery_percent: Option<u8>,
    pub codec: Option<String>,
    pub noise_mode: NoiseMode,
    pub noise_modes: Vec<NoiseMode>,
    pub dsee_enabled: bool,
    pub speak_to_chat: bool,
    pub pause_when_taken_off: bool,
    pub multipoint_summary: String,
    pub equalizer_summary: String,
    pub equalizer_preset: Option<EqPreset>,
    pub equalizer_choices: Vec<(EqPreset, String)>,
    pub connected_devices: Vec<ConnectedDevice>,
}

#[derive(Clone, Debug)]
pub struct MenuBarSnapshot {
    pub language: MenuBarLanguage,
    pub device: Option<MenuBarDevice>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmartFeature {
    Dsee,
    SpeakToChat,
    PauseWhenTakenOff,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuBarSection {
    Multipoint,
    Equalizer,
}

#[derive(Clone, Debug)]
pub enum MenuBarAction {
    Open,
    SetVolume(u8),
    SetNoiseMode(NoiseMode),
    SetSmartFeature(SmartFeature, bool),
    ToggleSection(MenuBarSection),
    SetEqualizer(EqPreset),
    PlayFrom(String),
    Quit,
}

struct ActionTarget {
    sender: Sender<MenuBarAction>,
    playback_devices: Vec<String>,
}

extern "C" fn perform_action(this: &Object, _: Sel, sender: *mut Object) {
    unsafe {
        let state = *this.get_ivar::<usize>("rustState") as *const ActionTarget;
        if state.is_null() || sender.is_null() {
            return;
        }
        let state = &*state;
        let tag: isize = msg_send![sender, tag];
        if tag == ACTION_TOGGLE_POPOVER {
            let popover = MENU_POPOVER.load(Ordering::Acquire);
            if popover.is_null() {
                return;
            }
            let shown: BOOL = msg_send![popover, isShown];
            if shown == YES {
                let _: () = msg_send![popover, performClose: sender];
            } else {
                show_popover(popover, sender);
            }
            return;
        }
        let action = match tag {
            ACTION_OPEN => MenuBarAction::Open,
            ACTION_QUIT => MenuBarAction::Quit,
            ACTION_VOLUME => {
                let value: f64 = msg_send![sender, doubleValue];
                MenuBarAction::SetVolume(value.round().clamp(0.0, 100.0) as u8)
            }
            tag if (ACTION_PRESET_BASE..ACTION_PRESET_BASE + EqPreset::CHOICES.len() as isize)
                .contains(&tag) =>
            {
                MenuBarAction::SetEqualizer(EqPreset::CHOICES[(tag - ACTION_PRESET_BASE) as usize])
            }
            tag if tag >= ACTION_PLAY_FROM_BASE => {
                let Some(id) = state
                    .playback_devices
                    .get((tag - ACTION_PLAY_FROM_BASE) as usize)
                else {
                    return;
                };
                MenuBarAction::PlayFrom(id.clone())
            }
            ACTION_MULTIPOINT => MenuBarAction::ToggleSection(MenuBarSection::Multipoint),
            ACTION_EQUALIZER => MenuBarAction::ToggleSection(MenuBarSection::Equalizer),
            ACTION_DSEE | ACTION_SPEAK | ACTION_PAUSE_OFF_HEAD => {
                let enabled: isize = msg_send![sender, state];
                let feature = match tag {
                    ACTION_DSEE => SmartFeature::Dsee,
                    ACTION_SPEAK => SmartFeature::SpeakToChat,
                    _ => SmartFeature::PauseWhenTakenOff,
                };
                MenuBarAction::SetSmartFeature(feature, enabled != 0)
            }
            ACTION_MODE_OFF => MenuBarAction::SetNoiseMode(NoiseMode::Off),
            ACTION_MODE_ANC => MenuBarAction::SetNoiseMode(NoiseMode::NoiseCancelling),
            ACTION_MODE_TRANSPARENCY => MenuBarAction::SetNoiseMode(NoiseMode::Transparency),
            ACTION_MODE_ADAPTIVE => MenuBarAction::SetNoiseMode(NoiseMode::Adaptive),
            ACTION_MODE_AMBIENT => MenuBarAction::SetNoiseMode(NoiseMode::Ambient),
            _ => return,
        };
        if matches!(action, MenuBarAction::Open | MenuBarAction::Quit) {
            close_popover();
        }
        let _ = state.sender.try_send(action);
    }
}

unsafe fn close_popover() {
    unsafe {
        let popover = MENU_POPOVER.load(Ordering::Acquire);
        if !popover.is_null() {
            let shown: BOOL = msg_send![popover, isShown];
            if shown == YES {
                let _: () = msg_send![popover, performClose: ptr::null_mut::<Object>()];
            }
        }
    }
}

unsafe fn show_popover(popover: *mut Object, anchor: *mut Object) {
    unsafe {
        let application: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![application, activateIgnoringOtherApps: YES];
        let bounds: NSRect = msg_send![anchor, bounds];
        let _: () =
            msg_send![popover, showRelativeToRect: bounds ofView: anchor preferredEdge: 1isize];
        let controller: *mut Object = msg_send![popover, contentViewController];
        let view: *mut Object = msg_send![controller, view];
        let window: *mut Object = msg_send![view, window];
        // Status-item popovers do not automatically become key. Without this,
        // AppKit renders NSSlider/NSSwitch in their inactive (gray) appearance
        // until the first click, and transient dismissal is unreliable.
        let _: () = msg_send![window, makeKeyWindow];
        let _: () = msg_send![view, setNeedsDisplay: YES];
    }
}

extern "C" fn application_resigned(_: &Object, _: Sel, _: *mut Object) {
    unsafe {
        close_popover();
    }
}

fn action_target_class() -> &'static Class {
    const NAME: &str = "IHeadphoneMenuBarActionTarget";
    if let Some(class) = Class::get(NAME) {
        return class;
    }
    let mut declaration = ClassDecl::new(NAME, class!(NSObject)).expect("menu target class");
    declaration.add_ivar::<usize>("rustState");
    unsafe {
        declaration.add_method(
            sel!(performMenuBarAction:),
            perform_action as extern "C" fn(&Object, Sel, *mut Object),
        );
    }
    unsafe {
        declaration.add_method(
            sel!(applicationResigned:),
            application_resigned as extern "C" fn(&Object, Sel, *mut Object),
        );
    }
    declaration.register()
}

unsafe fn ns_string(value: &str) -> *mut Object {
    let value = CString::new(value.replace('\0', " ")).expect("sanitized NSString");
    unsafe { msg_send![class!(NSString), stringWithUTF8String: value.as_ptr()] }
}

unsafe fn font(size: f64, weight: f64) -> *mut Object {
    unsafe { msg_send![class!(NSFont), systemFontOfSize: size weight: weight] }
}

unsafe fn add(parent: *mut Object, child: *mut Object) {
    unsafe {
        let _: () = msg_send![parent, addSubview: child];
    }
}

unsafe fn label(
    parent: *mut Object,
    frame: NSRect,
    text: &str,
    size: f64,
    weight: f64,
    secondary: bool,
) -> *mut Object {
    unsafe {
        let view: *mut Object = msg_send![class!(NSTextField), labelWithString: ns_string(text)];
        let _: () = msg_send![view, setFrame: frame];
        let _: () = msg_send![view, setFont: font(size, weight)];
        let color: *mut Object = if secondary {
            msg_send![class!(NSColor), secondaryLabelColor]
        } else {
            msg_send![class!(NSColor), labelColor]
        };
        let _: () = msg_send![view, setTextColor: color];
        let _: () = msg_send![view, setLineBreakMode: 4isize];
        add(parent, view);
        view
    }
}

unsafe fn divider(parent: *mut Object, y: f64) {
    unsafe {
        let view: *mut Object = msg_send![class!(NSBox), alloc];
        let view: *mut Object =
            msg_send![view, initWithFrame: rect(14.0, y, MENU_WIDTH - 28.0, 1.0)];
        let _: () = msg_send![view, setBoxType: 2isize];
        add(parent, view);
        let _: () = msg_send![view, release];
    }
}

unsafe fn symbol(name: &str, description: &str) -> *mut Object {
    unsafe {
        msg_send![class!(NSImage), imageWithSystemSymbolName: ns_string(name) accessibilityDescription: ns_string(description)]
    }
}

unsafe fn image_view(parent: *mut Object, frame: NSRect, image: *mut Object) -> *mut Object {
    unsafe {
        let view: *mut Object = msg_send![class!(NSImageView), alloc];
        let view: *mut Object = msg_send![view, initWithFrame: frame];
        let _: () = msg_send![view, setImage: image];
        let _: () = msg_send![view, setImageScaling: 3isize];
        add(parent, view);
        let _: () = msg_send![view, release];
        view
    }
}

extern "C" fn is_flipped(_: &Object, _: Sel) -> BOOL {
    YES
}

fn flipped_class(name: &str, superclass: &Class) -> &'static Class {
    if let Some(class) = Class::get(name) {
        return class;
    }
    let mut declaration = ClassDecl::new(name, superclass).expect("menu view class");
    unsafe {
        declaration.add_method(
            sel!(isFlipped),
            is_flipped as extern "C" fn(&Object, Sel) -> BOOL,
        );
    }
    declaration.register()
}

// Draw with semantic NSColors instead of caching CGColors in a layer. AppKit
// resolves these in the current appearance, including increased contrast.
extern "C" fn draw_circle(this: &Object, _: Sel, _: NSRect) {
    unsafe {
        let selected = *this.get_ivar::<BOOL>("selected") == YES;
        let fill: *mut Object = if selected {
            msg_send![class!(NSColor), controlAccentColor]
        } else {
            msg_send![class!(NSColor), quaternaryLabelColor]
        };
        let _: () = msg_send![fill, setFill];
        let bounds: NSRect = msg_send![this, bounds];
        let path: *mut Object = msg_send![class!(NSBezierPath), bezierPathWithOvalInRect: bounds];
        let _: () = msg_send![path, fill];
    }
}

fn circle_class() -> &'static Class {
    const NAME: &str = "IHeadphoneMenuCircle";
    if let Some(class) = Class::get(NAME) {
        return class;
    }
    let mut declaration = ClassDecl::new(NAME, class!(NSView)).expect("circle view class");
    declaration.add_ivar::<BOOL>("selected");
    unsafe {
        declaration.add_method(
            sel!(drawRect:),
            draw_circle as extern "C" fn(&Object, Sel, NSRect),
        );
    }
    declaration.register()
}

unsafe fn set_circle_style(circle: *mut Object, icon: *mut Object, selected: bool) {
    unsafe {
        (*circle).set_ivar("selected", if selected { YES } else { NO });
        let _: () = msg_send![circle, setNeedsDisplay: YES];
        let tint: *mut Object = if selected {
            msg_send![class!(NSColor), whiteColor]
        } else {
            msg_send![class!(NSColor), secondaryLabelColor]
        };
        let _: () = msg_send![icon, setContentTintColor: tint];
    }
}

unsafe fn circular_symbol(
    parent: *mut Object,
    frame: NSRect,
    symbol_name: &str,
    description: &str,
    selected: bool,
) -> (*mut Object, *mut Object) {
    unsafe {
        let circle: *mut Object = msg_send![circle_class(), alloc];
        let circle: *mut Object = msg_send![circle, initWithFrame: frame];
        let inset = 6.0;
        let icon = image_view(
            circle,
            rect(
                inset,
                inset,
                frame.size.width - inset * 2.0,
                frame.size.height - inset * 2.0,
            ),
            symbol(symbol_name, description),
        );
        set_circle_style(circle, icon, selected);
        add(parent, circle);
        let _: () = msg_send![circle, release];
        (circle, icon)
    }
}

unsafe fn action_button(
    parent: *mut Object,
    target: *mut Object,
    frame: NSRect,
    title: &str,
    tag: isize,
    size: f64,
) -> *mut Object {
    unsafe {
        let button: *mut Object = msg_send![class!(NSButton), buttonWithTitle: ns_string(title) target: target action: sel!(performMenuBarAction:)];
        let _: () = msg_send![button, setFrame: frame];
        let _: () = msg_send![button, setTag: tag];
        let _: () = msg_send![button, setBordered: NO];
        let _: () = msg_send![button, setButtonType: 0isize];
        let tint: *mut Object = msg_send![class!(NSColor), labelColor];
        let _: () = msg_send![button, setContentTintColor: tint];
        let _: () = msg_send![button, setAlignment: 0isize];
        let _: () = msg_send![button, setFont: font(size, 0.0)];
        add(parent, button);
        button
    }
}

unsafe fn toggle(
    parent: *mut Object,
    target: *mut Object,
    frame: NSRect,
    tag: isize,
) -> *mut Object {
    unsafe {
        let control: *mut Object = msg_send![class!(NSSwitch), alloc];
        let control: *mut Object = msg_send![control, initWithFrame: frame];
        let _: () = msg_send![control, setControlSize: 1isize];
        let _: () = msg_send![control, setTag: tag];
        let _: () = msg_send![control, setTarget: target];
        let _: () = msg_send![control, setAction: sel!(performMenuBarAction:)];
        add(parent, control);
        let _: () = msg_send![control, release];
        control
    }
}

unsafe fn set_text(view: *mut Object, value: &str) {
    unsafe {
        let _: () = msg_send![view, setStringValue: ns_string(value)];
    }
}
unsafe fn set_title(view: *mut Object, value: &str) {
    unsafe {
        let _: () = msg_send![view, setTitle: ns_string(value)];
    }
}
unsafe fn set_hidden(view: *mut Object, hidden: bool) {
    unsafe {
        let value: BOOL = if hidden { YES } else { NO };
        let _: () = msg_send![view, setHidden: value];
    }
}
unsafe fn set_enabled(view: *mut Object, enabled: bool) {
    unsafe {
        let value: BOOL = if enabled { YES } else { NO };
        let _: () = msg_send![view, setEnabled: value];
    }
}
unsafe fn set_state(view: *mut Object, enabled: bool) {
    unsafe {
        let value: isize = if enabled { 1 } else { 0 };
        let _: () = msg_send![view, setState: value];
    }
}

fn mode_tag(mode: NoiseMode) -> isize {
    match mode {
        NoiseMode::Off => ACTION_MODE_OFF,
        NoiseMode::NoiseCancelling => ACTION_MODE_ANC,
        NoiseMode::Transparency => ACTION_MODE_TRANSPARENCY,
        NoiseMode::Adaptive => ACTION_MODE_ADAPTIVE,
        NoiseMode::Ambient => ACTION_MODE_AMBIENT,
    }
}

pub fn set_output_volume(volume: u8) {
    let script = format!("set volume output volume {}", volume.min(100));
    let _ = Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .status();
}

fn output_volume() -> f64 {
    Command::new("/usr/bin/osascript")
        .args(["-e", "output volume of (get volume settings)"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(50.0)
}

struct ModeRow {
    mode: NoiseMode,
    view: *mut Object,
    button: *mut Object,
    title: *mut Object,
    check: *mut Object,
    circle: *mut Object,
    icon: *mut Object,
}

struct FeatureRow {
    view: *mut Object,
    title: *mut Object,
    control: *mut Object,
}

struct ActionRow {
    chevron: *mut Object,
    view: *mut Object,
    button: *mut Object,
    title: *mut Object,
    summary: *mut Object,
}

unsafe fn group(parent: *mut Object, height: f64) -> *mut Object {
    unsafe {
        let view: *mut Object =
            msg_send![flipped_class("IHeadphoneMenuGroup", class!(NSView)), alloc];
        let view: *mut Object = msg_send![view, initWithFrame: rect(0.0, 0.0, MENU_WIDTH, height)];
        add(parent, view);
        let _: () = msg_send![view, release];
        view
    }
}

unsafe fn place(view: *mut Object, y: f64) {
    unsafe {
        let _: () = msg_send![view, setFrameOrigin: NSPoint { x: 0.0, y }];
    }
}

unsafe fn feature_row(
    parent: *mut Object,
    target: *mut Object,
    icon: &str,
    title: &str,
    tag: isize,
) -> FeatureRow {
    unsafe {
        let view = group(parent, ROW_HEIGHT);
        circular_symbol(view, rect(16.0, 3.0, 24.0, 24.0), icon, title, false);
        let title = label(view, rect(49.0, 6.0, 194.0, 19.0), title, 13.0, 0.0, false);
        let control = toggle(view, target, rect(247.0, 4.0, 38.0, 22.0), tag);
        FeatureRow {
            view,
            title,
            control,
        }
    }
}

unsafe fn action_row(
    parent: *mut Object,
    target: *mut Object,
    icon: &str,
    title: &str,
    tag: isize,
    disclosure: bool,
) -> ActionRow {
    unsafe {
        let view = group(parent, 28.0);
        image_view(view, rect(20.0, 6.0, 16.0, 16.0), symbol(icon, title));
        let title = label(
            view,
            rect(48.0, 5.0, if disclosure { 116.0 } else { 236.0 }, 20.0),
            title,
            13.0,
            0.0,
            false,
        );
        let summary = label(view, rect(164.0, 6.0, 102.0, 18.0), "", 11.0, 0.0, true);
        let _: () = msg_send![summary, setAlignment: 2isize];
        let chevron = if disclosure {
            image_view(
                view,
                rect(272.0, 9.0, 10.0, 10.0),
                symbol("chevron.right", ""),
            )
        } else {
            ptr::null_mut()
        };
        // A transparent button covers the whole row, including its symbol and
        // trailing value, so all of the visible affordance is clickable.
        let button = action_button(
            view,
            target,
            rect(8.0, 0.0, MENU_WIDTH - 16.0, 28.0),
            "",
            tag,
            13.0,
        );
        ActionRow {
            chevron,
            view,
            button,
            title,
            summary,
        }
    }
}

pub struct MacMenuBar {
    status_bar: *mut Object,
    status_item: *mut Object,
    popover: *mut Object,
    root: *mut Object,
    overview: *mut Object,
    detail: *mut Object,
    detail_scroll: *mut Object,
    section: Option<MenuBarSection>,
    target: *mut Object,
    action_target: *mut ActionTarget,
    button: *mut Object,
    device_name: *mut Object,
    connection: *mut Object,
    battery: *mut Object,
    battery_icon: *mut Object,
    codec: *mut Object,
    headphone: *mut Object,
    volume_heading: *mut Object,
    noise_heading: *mut Object,
    mode_items: Vec<ModeRow>,
    lower: *mut Object,
    smart_heading: *mut Object,
    dsee: FeatureRow,
    speak: FeatureRow,
    pause: FeatureRow,
    footer: *mut Object,
    multipoint: ActionRow,
    equalizer: ActionRow,
    actions: *mut Object,
    open: ActionRow,
    quit: ActionRow,
    device_kind: Option<DeviceKind>,
    local_monitor: *mut Object,
    global_monitor: *mut Object,
}

impl MacMenuBar {
    pub fn new(sender: Sender<MenuBarAction>) -> Self {
        unsafe {
            let action_target = Box::into_raw(Box::new(ActionTarget {
                sender,
                playback_devices: Vec::new(),
            }));
            let target: *mut Object = msg_send![action_target_class(), new];
            (*target).set_ivar("rustState", action_target as usize);
            let status_bar: *mut Object = msg_send![class!(NSStatusBar), systemStatusBar];
            let status_item: *mut Object = msg_send![status_bar, statusItemWithLength: -1.0f64];
            let _: *mut Object = msg_send![status_item, retain];
            let _: () = msg_send![status_item, setVisible: YES];
            let button: *mut Object = msg_send![status_item, button];
            let icon = symbol("headphones", "iHeadphone");
            let _: () = msg_send![icon, setTemplate: YES];
            let _: () = msg_send![button, setImage: icon];
            let _: () = msg_send![button, setToolTip: ns_string("iHeadphone")];
            let _: () = msg_send![button, setTarget: target];
            let _: () = msg_send![button, setAction: sel!(performMenuBarAction:)];
            let _: () = msg_send![button, setTag: ACTION_TOGGLE_POPOVER];

            let root: *mut Object = msg_send![
                flipped_class("IHeadphoneMenuSurface", class!(NSVisualEffectView)),
                alloc
            ];
            let root: *mut Object =
                msg_send![root, initWithFrame: rect(0.0, 0.0, MENU_WIDTH, 562.0)];
            // NSVisualEffectMaterialPopover / behind-window / active. Leave
            // appearance inherited so native switches, text and vibrancy agree.
            let _: () = msg_send![root, setMaterial: 6isize];
            let _: () = msg_send![root, setBlendingMode: 0isize];
            let _: () = msg_send![root, setState: 1isize];

            let surface = root;
            let overview = group(surface, 534.0);
            let root = overview;
            let device_name = label(
                root,
                rect(16.0, 14.0, 200.0, 22.0),
                "iHeadphone",
                15.0,
                0.3,
                false,
            );
            let connection = label(root, rect(16.0, 38.0, 192.0, 18.0), "", 12.0, 0.0, true);
            let battery_icon = image_view(
                root,
                rect(16.0, 62.0, 22.0, 14.0),
                symbol("battery.100percent", "Battery"),
            );
            let battery = label(root, rect(43.0, 59.0, 50.0, 19.0), "—", 12.0, 0.0, true);
            let codec = label(root, rect(99.0, 60.0, 98.0, 18.0), "", 11.0, 0.2, true);
            let headphone = image_view(
                root,
                rect(220.0, 12.0, 64.0, 68.0),
                symbol("headphones", "Headphones"),
            );

            divider(root, 90.0);
            let volume_heading = label(root, rect(16.0, 99.0, 180.0, 18.0), "", 12.0, 0.3, true);
            image_view(
                root,
                rect(16.0, 125.0, 16.0, 16.0),
                symbol("speaker.fill", "Quiet"),
            );
            image_view(
                root,
                rect(262.0, 123.0, 22.0, 20.0),
                symbol("speaker.wave.3.fill", "Loud"),
            );
            let volume: *mut Object = msg_send![class!(NSSlider), sliderWithValue: output_volume() minValue: 0.0f64 maxValue: 100.0f64 target: target action: sel!(performMenuBarAction:)];
            let _: () = msg_send![volume, setFrame: rect(40.0, 119.0, 214.0, 28.0)];
            let _: () = msg_send![volume, setTag: ACTION_VOLUME];
            // Commit on release to avoid launching concurrent system-volume
            // writes while the thumb is tracking the pointer.
            let _: () = msg_send![volume, setContinuous: NO];
            let _: () = msg_send![volume, setAccessibilityLabel: ns_string("Volume")];
            add(root, volume);
            divider(root, 155.0);
            let noise_heading = label(root, rect(16.0, 164.0, 250.0, 18.0), "", 12.0, 0.3, true);
            let mode_items = [
                NoiseMode::NoiseCancelling,
                NoiseMode::Ambient,
                NoiseMode::Transparency,
                NoiseMode::Adaptive,
                NoiseMode::Off,
            ]
            .into_iter()
            .map(|mode| {
                let view = group(root, ROW_HEIGHT);
                let check = image_view(
                    view,
                    rect(18.0, 9.0, 12.0, 12.0),
                    symbol("checkmark", "Selected"),
                );
                let symbol_name = match mode {
                    NoiseMode::NoiseCancelling => "person.crop.circle.fill",
                    NoiseMode::Ambient | NoiseMode::Transparency => "person.wave.2.fill",
                    NoiseMode::Adaptive => "waveform",
                    NoiseMode::Off => "person.fill",
                };
                let (circle, icon) = circular_symbol(
                    view,
                    rect(39.0, 2.0, 26.0, 26.0),
                    symbol_name,
                    mode.label(),
                    false,
                );
                let title = label(
                    view,
                    rect(77.0, 6.0, 208.0, 20.0),
                    mode.label(),
                    13.0,
                    0.0,
                    false,
                );
                let button = action_button(
                    view,
                    target,
                    rect(8.0, 0.0, MENU_WIDTH - 16.0, ROW_HEIGHT),
                    "",
                    mode_tag(mode),
                    13.0,
                );
                ModeRow {
                    mode,
                    view,
                    button,
                    title,
                    check,
                    circle,
                    icon,
                }
            })
            .collect();

            let lower = group(root, 240.0);
            divider(lower, 0.0);
            let smart_heading = label(lower, rect(16.0, 9.0, 200.0, 18.0), "", 12.0, 0.3, true);
            let dsee = feature_row(lower, target, "waveform", "DSEE Extreme", ACTION_DSEE);
            let speak = feature_row(lower, target, "bubble.left", "Speak-to-Chat", ACTION_SPEAK);
            let pause = feature_row(
                lower,
                target,
                "pause.fill",
                "Pause when taken off",
                ACTION_PAUSE_OFF_HEAD,
            );
            let footer = group(lower, 150.0);
            divider(footer, 0.0);
            let multipoint = action_row(
                footer,
                target,
                "macbook.and.iphone",
                "Multipoint",
                ACTION_MULTIPOINT,
                true,
            );
            place(multipoint.view, 3.0);
            let equalizer = action_row(
                footer,
                target,
                "slider.horizontal.3",
                "Equalizer",
                ACTION_EQUALIZER,
                true,
            );
            place(equalizer.view, 31.0);
            let actions = group(footer, 66.0);
            divider(actions, 0.0);
            let open = action_row(
                actions,
                target,
                "arrow.up.forward.app",
                "Open iHeadphone…",
                ACTION_OPEN,
                false,
            );
            place(open.view, 3.0);
            let quit = action_row(
                actions,
                target,
                "power",
                "Quit iHeadphone",
                ACTION_QUIT,
                false,
            );
            place(quit.view, 31.0);

            let root = surface;
            let detail = group(footer, 180.0);
            set_hidden(detail, true);
            let controller: *mut Object = msg_send![class!(NSViewController), new];
            let _: () = msg_send![controller, setView: root];
            let _: () = msg_send![root, release];
            let popover: *mut Object = msg_send![class!(NSPopover), new];
            let _: () = msg_send![popover, setContentViewController: controller];
            let _: () = msg_send![controller, release];
            let _: () = msg_send![popover, setBehavior: 1isize];
            let _: () = msg_send![popover, setAnimates: YES];
            MENU_POPOVER.store(popover, Ordering::Release);
            let center: *mut Object = msg_send![class!(NSNotificationCenter), defaultCenter];
            let _: () = msg_send![center, addObserver: target selector: sel!(applicationResigned:) name: ns_string("NSApplicationDidResignActiveNotification") object: ptr::null_mut::<Object>()];
            // Transient NSPopover alone does not cover clicks in non-activating
            // panels and empty menu-bar space. Mouse-only monitors need no
            // accessibility permission and never consume another app's click.
            let mouse_down_mask = (1usize << 1) | (1usize << 3) | (1usize << 25);
            let global_handler = ConcreteBlock::new(|_: *mut Object| {
                close_popover();
            })
            .copy();
            let global_monitor: *mut Object = msg_send![class!(NSEvent), addGlobalMonitorForEventsMatchingMask: mouse_down_mask handler: &*global_handler];
            let _: *mut Object = msg_send![global_monitor, retain];
            let local_handler = ConcreteBlock::new(move |event: *mut Object| -> *mut Object {
                let shown: BOOL = msg_send![popover, isShown];
                if shown == YES {
                    let event_window: *mut Object = msg_send![event, window];
                    let popup_window: *mut Object = msg_send![root, window];
                    let anchor: *mut Object = msg_send![popover, positioningView];
                    let anchor_window: *mut Object = msg_send![anchor, window];
                    if event_window != popup_window && event_window != anchor_window {
                        close_popover();
                    } else if event_window == anchor_window {
                        let point: NSPoint = msg_send![event, locationInWindow];
                        let point: NSPoint = msg_send![anchor, convertPoint: point fromView: ptr::null_mut::<Object>()];
                        let bounds: NSRect = msg_send![anchor, bounds];
                        if point.x < bounds.origin.x || point.y < bounds.origin.y || point.x > bounds.origin.x + bounds.size.width || point.y > bounds.origin.y + bounds.size.height {
                            close_popover();
                        }
                    }
                }
                event
            }).copy();
            let local_monitor: *mut Object = msg_send![class!(NSEvent), addLocalMonitorForEventsMatchingMask: mouse_down_mask handler: &*local_handler];
            let _: *mut Object = msg_send![local_monitor, retain];

            Self {
                status_bar,
                status_item,
                popover,
                root,
                overview,
                detail,
                detail_scroll: ptr::null_mut(),
                section: None,
                target,
                action_target,
                button,
                device_name,
                connection,
                battery,
                battery_icon,
                codec,
                headphone,
                volume_heading,
                noise_heading,
                mode_items,
                lower,
                smart_heading,
                dsee,
                speak,
                pause,
                footer,
                multipoint,
                equalizer,
                actions,
                open,
                quit,
                device_kind: None,
                local_monitor,
                global_monitor,
            }
        }
    }

    pub fn update(&mut self, snapshot: &MenuBarSnapshot) {
        unsafe {
            let chinese = snapshot.language == MenuBarLanguage::SimplifiedChinese;
            let text = |en, zh| if chinese { zh } else { en };
            let device = snapshot.device.as_ref();
            let connected = device.is_some_and(|device| device.connected);
            (*self.action_target).playback_devices = device.map_or_else(Vec::new, |device| {
                device
                    .connected_devices
                    .iter()
                    .map(|device| device.id.clone())
                    .collect()
            });
            let sony = device.is_some_and(|device| device.kind == DeviceKind::SonyWh1000Xm5);
            set_text(self.volume_heading, text("Volume", "音量"));
            set_text(self.noise_heading, text("Noise Control", "噪声控制"));
            set_text(self.smart_heading, text("Smart Sound", "智能声音"));
            set_text(
                self.device_name,
                device.map_or("iHeadphone", |device| device.name.as_str()),
            );
            set_text(
                self.connection,
                match device {
                    None => text("Searching for headphones…", "正在查找耳机…"),
                    Some(_) if connected => text("Connected", "已连接"),
                    Some(_) => text("Disconnected", "未连接"),
                },
            );
            let battery = device
                .filter(|_| connected)
                .and_then(|device| device.battery_percent);
            set_text(
                self.battery,
                &battery.map_or_else(|| "—".into(), |level| format!("{level}%")),
            );
            let battery_symbol = match battery {
                Some(0..=10) => "battery.0percent",
                Some(11..=37) => "battery.25percent",
                Some(38..=62) => "battery.50percent",
                Some(63..=87) => "battery.75percent",
                _ => "battery.100percent",
            };
            let _: () = msg_send![self.battery_icon, setImage: symbol(battery_symbol, text("Battery", "电量"))];
            set_text(
                self.codec,
                device
                    .filter(|_| connected)
                    .and_then(|device| device.codec.as_deref())
                    .unwrap_or(""),
            );
            set_title(
                self.button,
                &battery.map_or_else(String::new, |level| format!(" {level}%")),
            );
            let tooltip = device.map_or_else(
                || "iHeadphone".to_owned(),
                |device| {
                    format!(
                        "{} · {}",
                        device.name,
                        battery.map_or_else(
                            || text("Disconnected", "未连接").into(),
                            |level| format!("{level}%")
                        )
                    )
                },
            );
            let _: () = msg_send![self.button, setToolTip: ns_string(&tooltip)];
            let kind = device.map(|device| device.kind);
            if kind != self.device_kind {
                if sony {
                    let bytes = include_bytes!("../assets/sony-wh1000xm5.png");
                    let data: *mut Object = msg_send![class!(NSData), dataWithBytes: bytes.as_ptr() length: bytes.len()];
                    let image: *mut Object = msg_send![class!(NSImage), alloc];
                    let image: *mut Object = msg_send![image, initWithData: data];
                    let _: () = msg_send![self.headphone, setImage: image];
                    let _: () = msg_send![image, release];
                } else {
                    let _: () =
                        msg_send![self.headphone, setImage: symbol("airpodspro", "AirPods")];
                }
                let icon = symbol(if sony { "headphones" } else { "airpodspro" }, "iHeadphone");
                let _: () = msg_send![icon, setTemplate: YES];
                let _: () = msg_send![self.button, setImage: icon];
                self.device_kind = kind;
            }

            // Pack supported modes in display order. Off and Adaptive must not
            // share a slot when a device exposes both.
            let mut y = 186.0;
            let mut mode_count = 0;
            for row in &self.mode_items {
                let available = device.is_some_and(|device| device.noise_modes.contains(&row.mode));
                set_hidden(row.view, !available);
                if !available {
                    continue;
                }
                place(row.view, y);
                y += ROW_HEIGHT;
                mode_count += 1;
                let selected = device.is_some_and(|device| device.noise_mode == row.mode);
                set_hidden(row.check, !selected);
                set_circle_style(row.circle, row.icon, selected && connected);
                set_text(row.title, mode_label(row.mode, chinese));
                set_enabled(row.button, connected);
                let _: () = msg_send![row.button, setAccessibilityLabel: ns_string(mode_label(row.mode, chinese))];
                let _: () = msg_send![row.button, setAccessibilityValue: ns_string(if selected { text("Selected", "已选择") } else { "" })];
            }
            set_hidden(self.noise_heading, mode_count == 0);
            if mode_count == 0 {
                y = 164.0;
            }
            place(self.lower, y + 6.0);
            set_hidden(self.smart_heading, device.is_none());
            set_hidden(self.dsee.view, !sony);
            set_hidden(self.speak.view, !sony);
            set_hidden(self.pause.view, device.is_none());
            let mut feature_y = if device.is_some() { 29.0 } else { 0.0 };
            for (row, visible, title, value) in [
                (
                    &self.dsee,
                    sony,
                    "DSEE Extreme",
                    device.is_some_and(|d| d.dsee_enabled),
                ),
                (
                    &self.speak,
                    sony,
                    text("Speak-to-Chat", "说话聊天"),
                    device.is_some_and(|d| d.speak_to_chat),
                ),
                (
                    &self.pause,
                    device.is_some(),
                    text("Pause when taken off", "摘下耳机时暂停"),
                    device.is_some_and(|d| d.pause_when_taken_off),
                ),
            ] {
                set_text(row.title, title);
                set_enabled(row.control, connected);
                set_state(row.control, value);
                let _: () = msg_send![row.control, setAccessibilityLabel: ns_string(title)];
                if visible {
                    place(row.view, feature_y);
                    feature_y += ROW_HEIGHT;
                }
            }
            let footer_y = feature_y + if device.is_some() { 5.0 } else { 0.0 };
            place(self.footer, footer_y);
            set_hidden(self.multipoint.view, !sony);
            set_hidden(self.equalizer.view, !sony);
            place(self.actions, if sony { 62.0 } else { 0.0 });
            for (row, title) in [
                (&self.multipoint, text("Multipoint", "多设备连接")),
                (&self.equalizer, text("Equalizer", "均衡器")),
                (&self.open, text("Open iHeadphone…", "打开 iHeadphone…")),
                (&self.quit, text("Quit iHeadphone", "退出 iHeadphone")),
            ] {
                set_text(row.title, title);
                let _: () = msg_send![row.button, setAccessibilityLabel: ns_string(title)];
            }
            set_enabled(self.multipoint.button, connected);
            set_enabled(self.equalizer.button, connected);
            set_text(
                self.multipoint.summary,
                device.map_or("", |d| d.multipoint_summary.as_str()),
            );
            set_text(
                self.equalizer.summary,
                device.map_or("", |d| d.equalizer_summary.as_str()),
            );
            // Disclosure content stays inside the footer, directly below its
            // owning row. The overview never disappears and no window is added.
            if !sony {
                self.section = None;
            }
            set_hidden(self.detail, self.section.is_none());
            let expanded_height = self
                .section
                .map_or(0.0, |section| self.update_detail(snapshot, section));
            let multipoint_expanded = self.section == Some(MenuBarSection::Multipoint);
            place(
                self.equalizer.view,
                31.0 + if multipoint_expanded {
                    expanded_height
                } else {
                    0.0
                },
            );
            place(self.detail, if multipoint_expanded { 31.0 } else { 59.0 });
            place(
                self.actions,
                if sony { 62.0 + expanded_height } else { 0.0 },
            );
            for (row, section) in [
                (&self.multipoint, MenuBarSection::Multipoint),
                (&self.equalizer, MenuBarSection::Equalizer),
            ] {
                let expanded = self.section == Some(section);
                let _: () = msg_send![row.chevron, setImage: symbol(if expanded { "chevron.down" } else { "chevron.right" }, "")];
                let _: () = msg_send![row.button, setAccessibilityValue: ns_string(if expanded { text("Expanded", "已展开") } else { text("Collapsed", "已收起") })];
            }
            let footer_height = if sony { 128.0 + expanded_height } else { 66.0 };
            let lower_height = footer_y + footer_height;
            let _: () = msg_send![self.footer, setFrameSize: NSSize { width: MENU_WIDTH, height: footer_height }];
            let _: () = msg_send![self.lower, setFrameSize: NSSize { width: MENU_WIDTH, height: lower_height }];
            let size = NSSize {
                width: MENU_WIDTH,
                height: y + 6.0 + lower_height,
            };
            let _: () = msg_send![self.overview, setFrameSize: size];
            let _: () = msg_send![self.root, setFrameSize: size];
            let _: () = msg_send![self.popover, setContentSize: size];
        }
    }

    pub fn toggle_section(&mut self, section: MenuBarSection, snapshot: &MenuBarSnapshot) {
        self.section = if self.section == Some(section) {
            None
        } else {
            Some(section)
        };
        self.detail_scroll = ptr::null_mut();
        self.update(snapshot);
    }

    pub fn collapse_section(&mut self) {
        self.section = None;
        self.detail_scroll = ptr::null_mut();
    }

    unsafe fn update_detail(&mut self, snapshot: &MenuBarSnapshot, section: MenuBarSection) -> f64 {
        unsafe {
            let previous_offset = if self.detail_scroll.is_null() {
                None
            } else {
                let clip: *mut Object = msg_send![self.detail_scroll, contentView];
                let bounds: NSRect = msg_send![clip, bounds];
                Some(bounds.origin.y)
            };
            let children: *mut Object = msg_send![self.detail, subviews];
            let children: *mut Object = msg_send![children, copy];
            let count: usize = msg_send![children, count];
            for index in 0..count {
                let child: *mut Object = msg_send![children, objectAtIndex: index];
                let _: () = msg_send![child, removeFromSuperview];
            }
            let _: () = msg_send![children, release];
            let chinese = snapshot.language == MenuBarLanguage::SimplifiedChinese;
            let text = |en, zh| if chinese { zh } else { en };
            let document = group(self.detail, 360.0);
            let device = snapshot
                .device
                .as_ref()
                .filter(|device| device.connected && device.kind == DeviceKind::SonyWh1000Xm5);
            let mut y = 4.0;
            let mut selected_y = 0.0;
            if let Some(device) = device {
                match section {
                    MenuBarSection::Equalizer => {
                        for (preset, title) in &device.equalizer_choices {
                            let Some(index) =
                                EqPreset::CHOICES.iter().position(|choice| choice == preset)
                            else {
                                continue;
                            };
                            let selected = device.equalizer_preset == Some(*preset);
                            if selected {
                                selected_y = y;
                                image_view(
                                    document,
                                    rect(49.0, y + 9.0, 11.0, 11.0),
                                    symbol("checkmark", text("Selected", "已选择")),
                                );
                            }
                            label(
                                document,
                                rect(70.0, y + 5.0, 210.0, 20.0),
                                title,
                                13.0,
                                if selected { 0.3 } else { 0.0 },
                                false,
                            );
                            let button = action_button(
                                document,
                                self.target,
                                rect(40.0, y, MENU_WIDTH - 52.0, 29.0),
                                "",
                                ACTION_PRESET_BASE + index as isize,
                                13.0,
                            );
                            let _: () = msg_send![button, setAccessibilityLabel: ns_string(title)];
                            let _: () = msg_send![button, setAccessibilityValue: ns_string(if selected { text("Selected", "已选择") } else { "" })];
                            y += 29.0;
                        }
                    }
                    MenuBarSection::Multipoint => {
                        label(
                            document,
                            rect(16.0, y + 1.0, 268.0, 19.0),
                            text("Choose a device to play audio from", "选择音频播放设备"),
                            12.0,
                            0.0,
                            true,
                        );
                        y += 29.0;
                        for (index, device) in device.connected_devices.iter().enumerate() {
                            image_view(
                                document,
                                rect(18.0, y + 12.0, 22.0, 24.0),
                                symbol("hifispeaker", ""),
                            );
                            label(
                                document,
                                rect(51.0, y + 5.0, 210.0, 20.0),
                                &device.name,
                                13.0,
                                0.2,
                                false,
                            );
                            label(
                                document,
                                rect(51.0, y + 26.0, 228.0, 18.0),
                                if device.playing {
                                    text("Currently playing", "当前正在播放")
                                } else {
                                    text("Connected · Play from here", "已连接 · 从此设备播放")
                                },
                                11.0,
                                0.0,
                                true,
                            );
                            if device.playing {
                                image_view(
                                    document,
                                    rect(272.0, y + 12.0, 12.0, 12.0),
                                    symbol("checkmark", text("Currently playing", "当前正在播放")),
                                );
                            }
                            let button = action_button(
                                document,
                                self.target,
                                rect(8.0, y, MENU_WIDTH - 16.0, 50.0),
                                "",
                                ACTION_PLAY_FROM_BASE + index as isize,
                                13.0,
                            );
                            let _: () =
                                msg_send![button, setAccessibilityLabel: ns_string(&device.name)];
                            let _: () = msg_send![button, setAccessibilityValue: ns_string(if device.playing { text("Currently playing", "当前正在播放") } else { text("Play from here", "从此设备播放") })];
                            set_enabled(button, !device.playing);
                            y += 52.0;
                        }
                        if device.connected_devices.is_empty() {
                            label(
                                document,
                                rect(16.0, y, 268.0, 22.0),
                                text("No connected devices reported", "耳机尚未回报连接设备"),
                                12.0,
                                0.0,
                                true,
                            );
                            y += 28.0;
                        }
                    }
                }
            } else {
                label(
                    document,
                    rect(16.0, y + 5.0, 268.0, 22.0),
                    text("Headphones disconnected", "耳机已断开连接"),
                    13.0,
                    0.0,
                    true,
                );
                y += 36.0;
            }
            let document_height = y + 4.0;
            // Keep the popup compact even with twelve presets. Scrolling only
            // affects the inline choices, leaving the main controls visible.
            let height = document_height.min(182.0);
            let _: () = msg_send![document, setFrameSize: NSSize { width: MENU_WIDTH, height: document_height }];
            let scroll: *mut Object = msg_send![class!(NSScrollView), alloc];
            let scroll: *mut Object =
                msg_send![scroll, initWithFrame: rect(0.0, 0.0, MENU_WIDTH, height)];
            let _: () = msg_send![scroll, setDrawsBackground: NO];
            let _: () = msg_send![scroll, setBorderType: 0isize];
            let _: () = msg_send![scroll, setHasVerticalScroller: YES];
            let _: () = msg_send![scroll, setAutohidesScrollers: YES];
            let _: () = msg_send![scroll, setScrollerStyle: 1isize];
            let _: () = msg_send![scroll, setDocumentView: document];
            add(self.detail, scroll);
            self.detail_scroll = scroll;
            let clip: *mut Object = msg_send![scroll, contentView];
            let offset = previous_offset
                .unwrap_or((selected_y - height / 2.0).max(0.0))
                .min((document_height - height).max(0.0));
            let _: () = msg_send![clip, scrollToPoint: NSPoint { x: 0.0, y: offset }];
            let _: () = msg_send![scroll, reflectScrolledClipView: clip];
            let _: () = msg_send![scroll, release];
            let _: () = msg_send![self.detail, setFrameSize: NSSize { width: MENU_WIDTH, height }];
            height
        }
    }

    /// Also used by the opt-in visual QA path, exercising the production view.
    pub fn show(&self) {
        unsafe {
            let application: *mut Object = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![application, activateIgnoringOtherApps: YES];
            // QA uses a real NSPopover anchored inside an app-owned window;
            // captures and outside-click tests do not expose the desktop.
            if std::env::var_os("IHEADPHONE_MENU_PREVIEW_WINDOW").is_some() {
                let preview: *mut Object = msg_send![class!(NSWindow), alloc];
                let preview: *mut Object = msg_send![preview, initWithContentRect: rect(0.0, 0.0, 360.0, 180.0) styleMask: 3usize backing: 2usize defer: NO];
                let _: () = msg_send![preview, setTitle: ns_string("iHeadphone · Menu Preview")];
                let content: *mut Object = msg_send![preview, contentView];
                let anchor = action_button(
                    content,
                    self.target,
                    rect(110.0, 130.0, 140.0, 30.0),
                    "iHeadphone",
                    ACTION_TOGGLE_POPOVER,
                    14.0,
                );
                if let Ok(appearance) = std::env::var("IHEADPHONE_MENU_PREVIEW_APPEARANCE") {
                    let name = if appearance == "dark" {
                        "NSAppearanceNameDarkAqua"
                    } else {
                        "NSAppearanceNameAqua"
                    };
                    let appearance: *mut Object =
                        msg_send![class!(NSAppearance), appearanceNamed: ns_string(name)];
                    let _: () = msg_send![self.popover, setAppearance: appearance];
                }
                let _: () = msg_send![preview, center];
                let _: () = msg_send![preview, makeKeyAndOrderFront: ptr::null_mut::<Object>()];
                show_popover(self.popover, anchor);
                return;
            }
            show_popover(self.popover, self.button);
        }
    }
}

fn mode_label(mode: NoiseMode, chinese: bool) -> &'static str {
    match (mode, chinese) {
        (NoiseMode::Off, false) => "Off",
        (NoiseMode::NoiseCancelling, false) => "Noise Cancellation",
        (NoiseMode::Transparency, false) => "Transparency",
        (NoiseMode::Adaptive, false) => "Adaptive",
        (NoiseMode::Ambient, false) => "Ambient Sound",
        (NoiseMode::Off, true) => "关闭",
        (NoiseMode::NoiseCancelling, true) => "降噪",
        (NoiseMode::Transparency, true) => "通透模式",
        (NoiseMode::Adaptive, true) => "自适应",
        (NoiseMode::Ambient, true) => "环境声",
    }
}

impl Drop for MacMenuBar {
    fn drop(&mut self) {
        unsafe {
            let center: *mut Object = msg_send![class!(NSNotificationCenter), defaultCenter];
            let _: () = msg_send![center, removeObserver: self.target];
            for monitor in [self.local_monitor, self.global_monitor] {
                if !monitor.is_null() {
                    let _: () = msg_send![class!(NSEvent), removeMonitor: monitor];
                    let _: () = msg_send![monitor, release];
                }
            }
            let _: () = msg_send![self.popover, performClose: ptr::null_mut::<Object>()];
            let _ = MENU_POPOVER.compare_exchange(
                self.popover,
                ptr::null_mut(),
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            let _: () = msg_send![self.status_bar, removeStatusItem: self.status_item];
            let _: () = msg_send![self.status_item, release];
            let _: () = msg_send![self.popover, release];
            (*self.target).set_ivar("rustState", 0usize);
            let _: () = msg_send![self.target, release];
            drop(Box::from_raw(self.action_target));
        }
    }
}
