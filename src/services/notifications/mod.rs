use super::{ReadOnlyService, Service, ServiceEvent};
use dbus::{BUS_NAME, NotificationDaemon, OBJECT_PATH};
use iced::{
    Subscription, Task,
    futures::{SinkExt, StreamExt, channel::mpsc::Sender, stream::pending},
    stream::channel,
};
use log::{error, info};
use std::any::TypeId;

pub mod dbus;

#[derive(Debug, Clone)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub app_icon: String,
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
#[allow(dead_code)]
pub enum CloseReason {
    Expired = 1,
    Dismissed = 2,
    ByApi = 3,
}

#[derive(Debug, Clone)]
pub enum NotificationEvent {
    Notify(Notification),
    Closed(u32, CloseReason),
}

#[derive(Debug, Clone)]
pub enum NotificationCommand {
    Close(u32),
}

#[derive(Debug, Clone)]
pub struct NotificationService {
    pub notifications: Vec<Notification>,
    pub max_notifications: usize,
    pub default_timeout: i32,
}

impl NotificationService {
    fn new(max_notifications: usize, default_timeout: i32) -> Self {
        Self {
            notifications: Vec::new(),
            max_notifications,
            default_timeout,
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
                let daemon = NotificationDaemon::new(tx.clone());

                match zbus::connection::Connection::session().await {
                    Ok(conn) => {
                        if let Err(e) = conn.object_server().at(OBJECT_PATH, daemon).await {
                            error!("Failed to register notification interface: {e}");
                            return State::Error;
                        }

                        match conn.request_name(BUS_NAME).await {
                            Ok(_) => {
                                info!("Notification service registered as {BUS_NAME}");

                                // Keep connection alive by spawning a task that holds it
                                tokio::spawn(async move {
                                    let _conn = conn;
                                    pending::<u8>().next().await;
                                });

                                let _ = output
                                    .send(ServiceEvent::Init(NotificationService::new(
                                        max_notifications,
                                        default_timeout,
                                    )))
                                    .await;

                                State::Active(rx)
                            }
                            Err(e) => {
                                error!("Failed to acquire bus name {BUS_NAME}: {e}");
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
                let default_timeout = self.default_timeout;

                // If replaces_id, remove old
                if let Some(pos) = self
                    .notifications
                    .iter()
                    .position(|n| n.id == notification.id)
                {
                    self.notifications.remove(pos);
                }

                // Handle auto-expiry via timeout
                if notification.urgency != Urgency::Critical {
                    let timeout = if notification.expire_timeout < 0 {
                        default_timeout
                    } else if notification.expire_timeout == 0 {
                        default_timeout
                    } else {
                        notification.expire_timeout
                    };

                    if timeout > 0 && notification.transient {
                        // Transient notifications are not stored
                        return;
                    }
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

impl Service for NotificationService {
    type Command = NotificationCommand;

    fn command(&mut self, command: Self::Command) -> Task<ServiceEvent<Self>> {
        match command {
            NotificationCommand::Close(id) => {
                self.notifications.retain(|n| n.id != id);
                Task::none()
            }
        }
    }
}
