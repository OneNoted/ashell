use crate::{
    components::icons::{StaticIcon, icon, icon_button},
    config::NotificationsModuleConfig,
    menu::MenuSize,
    services::{
        ReadOnlyService, ServiceEvent,
        notifications::{
            CloseReason, Notification, NotificationEvent, NotificationIcon, NotificationService,
        },
    },
    theme::AshellTheme,
    utils::truncate_chars,
};
use iced::{
    Alignment, Element, Length, Subscription, Task,
    widget::{
        Image, Row, Svg, button, column, container, horizontal_rule, mouse_area, row, scrollable,
        text, Column,
    },
    window::Id,
};

#[derive(Debug, Clone)]
pub enum Message {
    Event(ServiceEvent<NotificationService>),
    Dismiss(u32),
    DismissSignalSent,
    InvokeAction(u32, String),
    ActionSignalSent,
    ClearAll,
    ClearAllSignalsSent,
    MenuOpened,
}

pub enum Action {
    None,
    EmitSignal(Task<Message>),
    ShowPopup(Notification),
}

#[derive(Debug, Clone)]
pub struct Notifications {
    pub(crate) config: NotificationsModuleConfig,
    service: Option<NotificationService>,
    unread_count: usize,
}


impl Notifications {
    pub fn new(config: NotificationsModuleConfig) -> Self {
        Self {
            config,
            service: None,
            unread_count: 0,
        }
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::Event(event) => match event {
                ServiceEvent::Init(service) => {
                    self.service = Some(service);
                    Action::None
                }
                ServiceEvent::Update(notification_event) => {
                    if let Some(service) = self.service.as_mut() {
                        let popup_notification = match &notification_event {
                            NotificationEvent::Notify(n) => {
                                // Only increment unread for genuinely new notifications,
                                // not replacements of existing ones
                                let is_replacement = service
                                    .notifications
                                    .iter()
                                    .any(|existing| existing.id == n.id);
                                if !is_replacement {
                                    self.unread_count += 1;
                                }
                                Some(n.clone())
                            }
                            NotificationEvent::Closed(_, _) => None,
                        };
                        service.update(notification_event);
                        if let Some(n) = popup_notification {
                            return Action::ShowPopup(n);
                        }
                    }
                    Action::None
                }
                ServiceEvent::Error(_) => Action::None,
            },
            Message::Dismiss(id) => {
                if let Some(service) = self.service.as_mut() {
                    service.notifications.retain(|n| n.id != id);

                    // Emit NotificationClosed D-Bus signal (reason: dismissed by user)
                    let service_clone = service.clone();
                    return Action::EmitSignal(Task::perform(
                        async move {
                            service_clone
                                .emit_closed_signal(id, CloseReason::Dismissed)
                                .await;
                        },
                        |_| Message::DismissSignalSent,
                    ));
                }
                Action::None
            }
            Message::InvokeAction(id, action_key) => {
                if let Some(service) = self.service.as_mut() {
                    service.notifications.retain(|n| n.id != id);

                    let service_clone = service.clone();
                    return Action::EmitSignal(Task::perform(
                        async move {
                            service_clone
                                .emit_action_invoked_signal(id, &action_key)
                                .await;
                            service_clone
                                .emit_closed_signal(id, CloseReason::Dismissed)
                                .await;
                        },
                        |_| Message::ActionSignalSent,
                    ));
                }
                Action::None
            }
            Message::DismissSignalSent
            | Message::ActionSignalSent
            | Message::ClearAllSignalsSent => Action::None,
            Message::ClearAll => {
                if let Some(service) = self.service.as_mut() {
                    let ids: Vec<u32> = service.notifications.iter().map(|n| n.id).collect();
                    service.notifications.clear();
                    self.unread_count = 0;

                    // Emit NotificationClosed D-Bus signal for each dismissed notification
                    let service_clone = service.clone();
                    return Action::EmitSignal(Task::perform(
                        async move {
                            for id in ids {
                                service_clone
                                    .emit_closed_signal(id, CloseReason::Dismissed)
                                    .await;
                            }
                        },
                        |_| Message::ClearAllSignalsSent,
                    ));
                }
                self.unread_count = 0;
                Action::None
            }
            Message::MenuOpened => {
                self.unread_count = 0;
                Action::None
            }
        }
    }

    pub fn view(&self, theme: &AshellTheme) -> Element<'_, Message> {
        let has_notifications = self
            .service
            .as_ref()
            .is_some_and(|s| !s.notifications.is_empty());

        let mut content = row!(container(icon(if has_notifications {
            StaticIcon::BellAlert
        } else {
            StaticIcon::Bell
        })))
        .align_y(Alignment::Center)
        .spacing(theme.space.xxs);

        if self.unread_count > 0 {
            content = content.push(text(self.unread_count));
        }

        content.into()
    }

    pub fn menu_view<'a>(&'a self, _id: Id, theme: &'a AshellTheme) -> Element<'a, Message> {
        let notifications = self
            .service
            .as_ref()
            .map(|s| s.notifications.as_slice())
            .unwrap_or(&[]);

        column!(
            if notifications.is_empty() {
                std::convert::Into::<Element<'_, _, _>>::into(
                    container(text("No notifications")).padding(theme.space.xs),
                )
            } else {
                column!(
                    row!(
                        text(format!("{} Notifications", notifications.len()))
                            .width(Length::Fill),
                        button("Clear all")
                            .style(theme.ghost_button_style())
                            .padding([2, theme.space.xs])
                            .on_press(Message::ClearAll)
                    )
                    .align_y(Alignment::Center)
                    .padding(theme.space.xs),
                    horizontal_rule(1),
                    container(scrollable(
                        Column::with_children(
                            notifications
                                .iter()
                                .map(|n| {
                                    let time = n.timestamp.format("%H:%M").to_string();
                                    let summary = n.summary.clone();
                                    let body = n.body.clone();
                                    let app = n.app_name.clone();
                                    let id = n.id;

                                    // Partition actions: default vs visible
                                    let has_default_action =
                                        n.actions.iter().any(|(k, _)| k == "default");
                                    let visible_actions: Vec<_> = n
                                        .actions
                                        .iter()
                                        .filter(|(k, _)| k != "default")
                                        .collect();

                                    // Icon element
                                    let icon_element: Option<Element<'_, _, _>> =
                                        n.icon.as_ref().map(|icon| match icon {
                                            NotificationIcon::Image(handle) => {
                                                Image::new(handle.clone())
                                                    .height(Length::Fixed(24.))
                                                    .into()
                                            }
                                            NotificationIcon::Svg(handle) => Svg::new(handle.clone())
                                                .height(Length::Fixed(24.))
                                                .width(Length::Fixed(24.))
                                                .into(),
                                        });

                                    // Text content column
                                    let mut text_col = column!(
                                        row!(
                                            text(app).size(theme.font_size.xs),
                                            text(time)
                                                .size(theme.font_size.xs)
                                                .color(
                                                    theme
                                                        .get_theme()
                                                        .extended_palette()
                                                        .secondary
                                                        .base
                                                        .text
                                                ),
                                        )
                                        .spacing(theme.space.xs),
                                        text(summary).size(theme.font_size.sm),
                                    )
                                    .spacing(2)
                                    .width(Length::Fill);

                                    if !body.is_empty() {
                                        text_col = text_col.push(
                                            text(truncate_chars(&body, 200).to_owned())
                                                .size(theme.font_size.xs),
                                        );
                                    }

                                    // Action buttons row
                                    if !visible_actions.is_empty() {
                                        let action_buttons: Vec<Element<'_, _, _>> =
                                            visible_actions
                                                .iter()
                                                .map(|(key, label)| {
                                                    button(
                                                        text(label.clone())
                                                            .size(theme.font_size.xs),
                                                    )
                                                    .style(theme.ghost_button_style())
                                                    .padding([2, theme.space.xs])
                                                    .on_press(Message::InvokeAction(
                                                        id,
                                                        key.clone(),
                                                    ))
                                                    .into()
                                                })
                                                .collect();
                                        text_col = text_col.push(
                                            Row::with_children(action_buttons)
                                                .spacing(theme.space.xxs),
                                        );
                                    }

                                    // Build the main row with optional icon
                                    let mut content_row = row!().spacing(theme.space.xs).align_y(Alignment::Center);
                                    if let Some(icon_el) = icon_element {
                                        content_row = content_row.push(icon_el);
                                    }
                                    content_row = content_row
                                        .push(text_col)
                                        .push(
                                            icon_button::<Message>(theme, StaticIcon::Close)
                                                .on_press(Message::Dismiss(id)),
                                        );

                                    let notification_content: Element<'_, _, _> =
                                        container(content_row)
                                            .padding([theme.space.xs, 0])
                                            .into();

                                    // Wrap with mouse_area for default action click
                                    if has_default_action {
                                        mouse_area(notification_content)
                                            .on_press(Message::InvokeAction(
                                                id,
                                                "default".to_string(),
                                            ))
                                            .into()
                                    } else {
                                        notification_content
                                    }
                                })
                                .collect::<Vec<Element<'_, _, _>>>(),
                        )
                        .spacing(2)
                        .padding([0, theme.space.xs]),
                    ))
                    .max_height(400),
                )
                .into()
            },
        )
        .spacing(theme.space.xs)
        .max_width(MenuSize::Medium)
        .into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        NotificationService::subscribe_with_config(
            self.config.max_notifications,
            self.config.default_timeout,
        )
        .map(Message::Event)
    }
}
