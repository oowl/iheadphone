//! One AVRCP target player for headset media buttons, registered with BlueZ
//! in place of `mpris-proxy`.
//!
//! Headsets choose between Play and Pause from playback state they track
//! themselves. AirPods keep sending Play once that state goes stale, so a
//! stem press could pause but never resume. Every Play, Pause or PlayPause
//! here therefore toggles what is actually playing on this computer.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use zbus::zvariant::{ObjectPath, Value};

const PLAYER_PATH: &str = "/io/github/iheadphone/player";
const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const MPRIS_PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const POLL_EVERY: Duration = Duration::from_secs(1);
const RETRY_EVERY: Duration = Duration::from_secs(5);
/// AirPods may send Play or Pause right after being put in or taken out;
/// the wear handling in the app already covers those moments.
const WEAR_GRACE_MS: u64 = 2_000;

static LAST_WEAR_CHANGE_MS: AtomicU64 = AtomicU64::new(0);

/// Called when a headset reports that it was put on or taken off.
pub fn note_wear_change() {
    LAST_WEAR_CHANGE_MS.store(now_ms(), Ordering::Relaxed);
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64)
}

pub fn run() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to start media player runtime");
    runtime.block_on(async {
        loop {
            if let Err(error) = serve().await {
                eprintln!("media: headset button player unavailable: {error}");
            }
            tokio::time::sleep(RETRY_EVERY).await;
        }
    });
}

async fn serve() -> zbus::Result<()> {
    let session = zbus::Connection::session().await?;
    let system = zbus::Connection::system().await?;
    let player = HeadsetPlayer {
        session: session.clone(),
        resume: Arc::new(Mutex::new(Vec::new())),
    };
    let mut status = player.aggregate_status().await;
    system.object_server().at(PLAYER_PATH, player).await?;

    let adapter = default_adapter()
        .await
        .map_err(|error| zbus::Error::Failure(error.to_string()))?;
    let media = zbus::Proxy::new(
        &system,
        "org.bluez",
        format!("/org/bluez/{adapter}"),
        "org.bluez.Media1",
    )
    .await?;
    let mut properties = HashMap::new();
    properties.insert("PlaybackStatus", Value::from(status));
    properties.insert("Identity", Value::from("iHeadphone"));
    for flag in [
        "CanPlay",
        "CanPause",
        "CanGoNext",
        "CanGoPrevious",
        "CanControl",
    ] {
        properties.insert(flag, Value::from(true));
    }
    media
        .call_method(
            "RegisterPlayer",
            &(ObjectPath::try_from(PLAYER_PATH)?, properties),
        )
        .await?;
    eprintln!("media: headset buttons registered with BlueZ on {adapter}");

    let interface = system
        .object_server()
        .interface::<_, HeadsetPlayer>(PLAYER_PATH)
        .await?;
    loop {
        tokio::time::sleep(POLL_EVERY).await;
        let current = interface.get().await.aggregate_status().await;
        if current != status {
            status = current;
            interface
                .get()
                .await
                .playback_status_changed(interface.signal_emitter())
                .await?;
        }
        // BlueZ drops the registration when it restarts; register again.
        if !system
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "NameHasOwner",
                &("org.bluez",),
            )
            .await?
            .body()
            .deserialize::<bool>()?
        {
            system
                .object_server()
                .remove::<HeadsetPlayer, _>(PLAYER_PATH)
                .await?;
            return Err(zbus::Error::Failure("bluetoothd went away".into()));
        }
    }
}

async fn default_adapter() -> bluer::Result<String> {
    let adapter = bluer::Session::new().await?.default_adapter().await?;
    Ok(adapter.name().to_owned())
}

struct HeadsetPlayer {
    session: zbus::Connection,
    /// Players paused by a button press, to resume on the next one.
    resume: Arc<Mutex<Vec<String>>>,
}

impl HeadsetPlayer {
    async fn players(&self) -> Vec<(String, String)> {
        let Ok(bus) = zbus::fdo::DBusProxy::new(&self.session).await else {
            return Vec::new();
        };
        let Ok(names) = bus.list_names().await else {
            return Vec::new();
        };
        let mut players = Vec::new();
        for name in names {
            let name = name.as_str();
            // playerctld mirrors another player; counting it would act twice.
            if !name.starts_with(MPRIS_PREFIX) || name.ends_with(".playerctld") {
                continue;
            }
            if let Some(status) = self.status_of(name).await {
                players.push((name.to_owned(), status));
            }
        }
        players
    }

    async fn status_of(&self, name: &str) -> Option<String> {
        let proxy = self.proxy(name).await?;
        proxy.get_property::<String>("PlaybackStatus").await.ok()
    }

    async fn proxy(&self, name: &str) -> Option<zbus::Proxy<'static>> {
        zbus::Proxy::new(&self.session, name.to_owned(), MPRIS_PATH, MPRIS_PLAYER)
            .await
            .ok()
    }

    async fn send(&self, name: &str, method: &str) {
        if let Some(proxy) = self.proxy(name).await
            && let Err(error) = proxy.call_method(method, &()).await
        {
            eprintln!("media: {name} rejected {method}: {error}");
        }
    }

    async fn aggregate_status(&self) -> &'static str {
        if self
            .players()
            .await
            .iter()
            .any(|(_, status)| status == "Playing")
        {
            "Playing"
        } else {
            "Paused"
        }
    }

    async fn toggle(&self) {
        if now_ms().saturating_sub(LAST_WEAR_CHANGE_MS.load(Ordering::Relaxed)) < WEAR_GRACE_MS {
            return;
        }
        let players = self.players().await;
        let playing: Vec<String> = players
            .iter()
            .filter(|(_, status)| status == "Playing")
            .map(|(name, _)| name.clone())
            .collect();
        if !playing.is_empty() {
            for name in &playing {
                self.send(name, "Pause").await;
            }
            if let Ok(mut resume) = self.resume.lock() {
                *resume = playing;
            }
            return;
        }
        let remembered = self
            .resume
            .lock()
            .map(|mut resume| std::mem::take(&mut *resume))
            .unwrap_or_default();
        let targets: Vec<&str> = remembered
            .iter()
            .filter(|name| players.iter().any(|(player, _)| player == *name))
            .map(String::as_str)
            .collect();
        if targets.is_empty() {
            if let Some((name, _)) = players.iter().find(|(_, status)| status == "Paused") {
                self.send(name, "Play").await;
            }
        } else {
            for name in targets {
                self.send(name, "Play").await;
            }
        }
    }

    async fn active_player(&self) -> Option<String> {
        let players = self.players().await;
        players
            .iter()
            .find(|(_, status)| status == "Playing")
            .or_else(|| players.iter().find(|(_, status)| status == "Paused"))
            .map(|(name, _)| name.clone())
    }

    async fn forward(&self, method: &str) {
        if let Some(name) = self.active_player().await {
            self.send(&name, method).await;
        }
    }
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl HeadsetPlayer {
    async fn play(&self) {
        self.toggle().await;
    }

    async fn pause(&self) {
        self.toggle().await;
    }

    async fn play_pause(&self) {
        self.toggle().await;
    }

    async fn stop(&self) {
        for (name, status) in self.players().await {
            if status == "Playing" {
                self.send(&name, "Pause").await;
            }
        }
    }

    async fn next(&self) {
        self.forward("Next").await;
    }

    async fn previous(&self) {
        self.forward("Previous").await;
    }

    #[zbus(property)]
    async fn playback_status(&self) -> String {
        self.aggregate_status().await.to_owned()
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}
