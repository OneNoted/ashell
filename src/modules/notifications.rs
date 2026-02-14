use crate::{
    components::icons::{StaticIcon, icon, icon_button},
    config::NotificationsModuleConfig,
    menu::MenuSize,
    services::{
        ReadOnlyService, Service, ServiceEvent,
        notifications::{NotificationCommand, NotificationEvent, NotificationService},
    },
    theme::AshellTheme,
};
use iced::{
    Alignment, Element, Length, Subscription,
    widget::{button, column, container, horizontal_rule, row, scrollable, text, Column},
    window::Id,
};

#[derive(Debug, Clone)]
pub enum Message {
    Event(ServiceEvent<NotificationService>),
    Dismiss(u32),
    ClearAll,
    MenuOpened,
}

pub enum Action {
    None,
    Command(iced::Task<Message>),
}

#[derive(Debug, Clone)]
pub struct Notifications {
    config: NotificationsModuleConfig,
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
                        if matches!(notification_event, NotificationEvent::Notify(_)) {
                            self.unread_count += 1;
                        }
                        service.update(notification_event);
                    }
                    Action::None
                }
                ServiceEvent::Error(_) => Action::None,
            },
            Message::Dismiss(id) => {
                if let Some(service) = self.service.as_mut() {
                    let _ = service.command(NotificationCommand::Close(id));
                }
                Action::None
            }
            Message::ClearAll => {
                if let Some(service) = self.service.as_mut() {
                    service.notifications.clear();
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

                                    container(
                                        row!(
                                            column!(
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
                                                if !body.is_empty() {
                                                    std::convert::Into::<Element<'_, _, _>>::into(
                                                        text({
                                                            let mut b = body;
                                                            b.truncate(200);
                                                            b
                                                        })
                                                        .size(theme.font_size.xs),
                                                    )
                                                } else {
                                                    std::convert::Into::<Element<'_, _, _>>::into(
                                                        row!(),
                                                    )
                                                },
                                            )
                                            .spacing(2)
                                            .width(Length::Fill),
                                            icon_button::<Message>(theme, StaticIcon::Close)
                                                .on_press(Message::Dismiss(id)),
                                        )
                                        .align_y(Alignment::Center)
                                        .spacing(theme.space.xs),
                                    )
                                    .padding([theme.space.xs, 0])
                                    .into()
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
