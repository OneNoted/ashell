use log::{debug, info};
use std::collections::HashMap;
use tokio::sync::mpsc::Sender;
use zbus::{interface, object_server::SignalEmitter, zvariant::Value};

use super::{CloseReason, Notification, NotificationEvent, Urgency};

pub const BUS_NAME: &str = "org.freedesktop.Notifications";
pub const OBJECT_PATH: &str = "/org/freedesktop/Notifications";

pub struct NotificationDaemon {
    next_id: u32,
    sender: Sender<NotificationEvent>,
}

impl NotificationDaemon {
    pub fn new(sender: Sender<NotificationEvent>) -> Self {
        Self {
            next_id: 1,
            sender,
        }
    }
}

#[interface(name = "org.freedesktop.Notifications")]
impl NotificationDaemon {
    fn get_capabilities(&self) -> Vec<&str> {
        vec!["body", "body-markup", "actions"]
    }

    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &mut self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<&str>,
        hints: HashMap<&str, Value<'_>>,
        expire_timeout: i32,
        #[zbus(signal_emitter)] _emitter: SignalEmitter<'_>,
    ) -> u32 {
        let id = if replaces_id > 0 {
            replaces_id
        } else {
            let id = self.next_id;
            self.next_id = self.next_id.wrapping_add(1).max(1);
            id
        };

        let urgency = hints
            .get("urgency")
            .and_then(|v| match v {
                Value::U8(u) => Some(*u),
                _ => None,
            })
            .map(|u| match u {
                0 => Urgency::Low,
                2 => Urgency::Critical,
                _ => Urgency::Normal,
            })
            .unwrap_or(Urgency::Normal);

        let transient = hints
            .get("transient")
            .and_then(|v| match v {
                Value::Bool(b) => Some(*b),
                _ => None,
            })
            .unwrap_or(false);

        let parsed_actions: Vec<(String, String)> = actions
            .chunks(2)
            .filter_map(|chunk| {
                if chunk.len() == 2 {
                    Some((chunk[0].to_string(), chunk[1].to_string()))
                } else {
                    None
                }
            })
            .collect();

        let notification = Notification {
            id,
            app_name: app_name.to_string(),
            app_icon: app_icon.to_string(),
            summary: summary.to_string(),
            body: body.to_string(),
            actions: parsed_actions,
            urgency,
            expire_timeout,
            timestamp: chrono::Local::now(),
            transient,
        };

        info!("Notification received: id={id}, summary={summary}");
        debug!("Notification details: {notification:?}");

        let _ = self
            .sender
            .send(NotificationEvent::Notify(notification))
            .await;

        id
    }

    async fn close_notification(
        &self,
        id: u32,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) {
        info!("CloseNotification called for id={id}");
        let _ = self
            .sender
            .send(NotificationEvent::Closed(id, CloseReason::ByApi))
            .await;
        let _ = Self::notification_closed(&emitter, id, CloseReason::ByApi as u32).await;
    }

    fn get_server_information(&self) -> (&str, &str, &str, &str) {
        ("ashell", "ashell", env!("CARGO_PKG_VERSION"), "1.2")
    }

    #[zbus(signal)]
    async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;
}
