use super::{ReadOnlyService, ServiceEvent};
use dbus::{BUS_NAME, NotificationDaemon, OBJECT_PATH};
use freedesktop_icons::lookup;
use iced::{
    Subscription,
    futures::{SinkExt, StreamExt, channel::mpsc::Sender, stream::pending},
    stream::channel,
    widget::{image, svg},
};
use linicon_theme::get_icon_theme;
use log::{debug, error, info, warn};
use std::{any::TypeId, path::Path};
use zbus::fdo::RequestNameFlags;

pub mod dbus;

#[derive(Debug, Clone)]
pub enum NotificationIcon {
    Image(image::Handle),
    Svg(svg::Handle),
}

#[derive(Debug, Clone)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub app_icon: String,
    pub icon: Option<NotificationIcon>,
    pub summary: String,
    pub body: String,
    pub actions: Vec<(String, String)>,
    pub urgency: Urgency,
    pub expire_timeout: i32,
    pub timestamp: chrono::DateTime<chrono::Local>,
    pub transient: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Low,
    Normal,
    Critical,
}

#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum CloseReason {
    Expired = 1,
    Dismissed = 2,
    ByApi = 3,
}

pub fn resolve_icon(app_icon: &str) -> Option<NotificationIcon> {
    if app_icon.is_empty() {
        return None;
    }

    if app_icon.starts_with('/') {
        let path = Path::new(app_icon);
        if !path.exists() {
            return None;
        }
        return if path.extension().is_some_and(|ext| ext == "svg") {
            debug!("notification svg icon from path: {path:?}");
            Some(NotificationIcon::Svg(svg::Handle::from_path(path)))
        } else {
            debug!("notification raster icon from path: {path:?}");
            Some(NotificationIcon::Image(image::Handle::from_path(path)))
        };
    }

    // Freedesktop icon lookup
    let base_lookup = lookup(app_icon).with_cache();
    let found = match get_icon_theme() {
        Some(theme) => base_lookup.with_theme(&theme).find().or_else(|| {
            let fallback = lookup(app_icon).with_cache();
            fallback.find()
        }),
        None => base_lookup.find(),
    };

    found.map(|path| {
        if path.extension().is_some_and(|ext| ext == "svg") {
            debug!("notification svg icon found: {path:?}");
            NotificationIcon::Svg(svg::Handle::from_path(path))
        } else {
            debug!("notification raster icon found: {path:?}");
            NotificationIcon::Image(image::Handle::from_path(path))
        }
    })
}

#[derive(Debug, Clone)]
pub enum NotificationEvent {
    Notify(Notification),
    Closed(u32, CloseReason),
}

#[derive(Debug, Clone)]
pub struct NotificationService {
    pub notifications: Vec<Notification>,
    pub max_notifications: usize,
    pub default_timeout: i32,
    conn: Option<zbus::Connection>,
}

impl NotificationService {
    fn new(max_notifications: usize, default_timeout: i32, conn: zbus::Connection) -> Self {
        Self {
            notifications: Vec::new(),
            max_notifications,
            default_timeout,
            conn: Some(conn),
        }
    }

    pub async fn emit_action_invoked_signal(&self, id: u32, action_key: &str) {
        if let Some(conn) = &self.conn {
            let _ = conn
                .emit_signal(
                    None::<zbus::names::BusName>,
                    OBJECT_PATH,
                    "org.freedesktop.Notifications",
                    "ActionInvoked",
                    &(id, action_key),
                )
                .await;
        }
    }

    pub async fn emit_closed_signal(&self, id: u32, reason: CloseReason) {
        if let Some(conn) = &self.conn {
            let _ = conn
                .emit_signal(
                    None::<zbus::names::BusName>,
                    OBJECT_PATH,
                    "org.freedesktop.Notifications",
                    "NotificationClosed",
                    &(id, reason as u32),
                )
                .await;
        }
    }
}

enum State {
    Init {
        max_notifications: usize,
        default_timeout: i32,
    },
    Active(tokio::sync::mpsc::Receiver<NotificationEvent>),
    Error,
}

impl NotificationService {
    async fn start_listening(state: State, output: &mut Sender<ServiceEvent<Self>>) -> State {
        match state {
            State::Init {
                max_notifications,
                default_timeout,
            } => {
                info!("Initializing notification service");

                let (tx, rx) = tokio::sync::mpsc::channel::<NotificationEvent>(100);
                let daemon = NotificationDaemon::new(tx, default_timeout);

                match zbus::connection::Connection::session().await {
                    Ok(conn) => {
                        if let Err(e) = conn.object_server().at(OBJECT_PATH, daemon).await {
                            error!("Failed to register notification interface: {e}");
                            return State::Error;
                        }

                        let flags = RequestNameFlags::DoNotQueue
                            | RequestNameFlags::ReplaceExisting
                            | RequestNameFlags::AllowReplacement;

                        match conn.request_name_with_flags(BUS_NAME, flags).await {
                            Ok(_) => {
                                info!("Notification service registered as {BUS_NAME}");

                                let service_conn = conn.clone();

                                // Keep connection alive by spawning a task that holds it
                                tokio::spawn(async move {
                                    let _conn = conn;
                                    pending::<u8>().next().await;
                                });

                                let _ = output
                                    .send(ServiceEvent::Init(NotificationService::new(
                                        max_notifications,
                                        default_timeout,
                                        service_conn,
                                    )))
                                    .await;

                                State::Active(rx)
                            }
                            Err(e) => {
                                warn!("Failed to acquire bus name {BUS_NAME}: {e}. Another notification daemon may be running.");
                                State::Error
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to connect to session bus: {e}");
                        State::Error
                    }
                }
            }
            State::Active(mut rx) => {
                info!("Listening for notification events");

                while let Some(event) = rx.recv().await {
                    let _ = output.send(ServiceEvent::Update(event)).await;
                }

                error!("Notification event channel closed");
                State::Error
            }
            State::Error => {
                error!("Notification service error");
                let _ = pending::<u8>().next().await;
                State::Error
            }
        }
    }

    pub fn subscribe_with_config(
        max_notifications: usize,
        default_timeout: i32,
    ) -> Subscription<ServiceEvent<Self>> {
        let id = TypeId::of::<Self>();

        Subscription::run_with_id(
            id,
            channel(100, async move |mut output| {
                let mut state = State::Init {
                    max_notifications,
                    default_timeout,
                };

                loop {
                    state = NotificationService::start_listening(state, &mut output).await;
                }
            }),
        )
    }
}

impl ReadOnlyService for NotificationService {
    type UpdateEvent = NotificationEvent;
    type Error = ();

    fn update(&mut self, event: Self::UpdateEvent) {
        match event {
            NotificationEvent::Notify(notification) => {
                // If replaces_id, remove old
                if let Some(pos) = self
                    .notifications
                    .iter()
                    .position(|n| n.id == notification.id)
                {
                    self.notifications.remove(pos);
                }

                // Transient notifications with a timeout are not stored in the list
                if notification.transient && notification.urgency != Urgency::Critical {
                    return;
                }

                self.notifications.insert(0, notification);

                // Trim to max
                if self.notifications.len() > self.max_notifications {
                    self.notifications.truncate(self.max_notifications);
                }
            }
            NotificationEvent::Closed(id, _reason) => {
                self.notifications.retain(|n| n.id != id);
            }
        }
    }

    fn subscribe() -> Subscription<ServiceEvent<Self>> {
        Self::subscribe_with_config(50, 5000)
    }
}
