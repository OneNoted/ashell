use crate::{
    HEIGHT,
    config::{self, AppearanceStyle, Config, Modules, Position},
    get_log_spec,
    menu::MenuType,
    modules::{
        self,
        clock::Clock,
        custom_module::{self, Custom},
        keyboard_layout::KeyboardLayout,
        keyboard_submap::KeyboardSubmap,
        media_player::MediaPlayer,
        notifications::Notifications,
        privacy::Privacy,
        settings::Settings,
        system_info::SystemInfo,
        tempo::Tempo,
        tray::TrayModule,
        updates::Updates,
        window_title::WindowTitle,
        workspaces::Workspaces,
    },
    outputs::{HasOutput, Outputs},
    popup::PopupState,
    services::ReadOnlyService,
    theme::{AshellTheme, backdrop_color, darken_color},
    widgets::{ButtonUIRef, Centerbox},
};
use flexi_logger::LoggerHandle;
use iced::{
    Alignment, Color, Element, Gradient, Length, Radians, Subscription, Task, Theme,
    daemon::Appearance,
    event::{
        listen_with,
        wayland::{Event as WaylandEvent, OutputEvent},
    },
    gradient::Linear,
    keyboard,
    widget::{Row, container, mouse_area},
    window::Id,
};
use log::{debug, info, warn};
use std::{collections::HashMap, f32::consts::PI, path::PathBuf, time::{Duration, Instant}};
use wayland_client::protocol::wl_output::WlOutput;

pub struct GeneralConfig {
    outputs: config::Outputs,
    pub modules: Modules,
    pub layer: config::Layer,
    enable_esc_key: bool,
}

pub struct App {
    config_path: PathBuf,
    pub theme: AshellTheme,
    logger: LoggerHandle,
    pub general_config: GeneralConfig,
    pub outputs: Outputs,
    pub custom: HashMap<String, Custom>,
    pub updates: Option<Updates>,
    pub workspaces: Workspaces,
    pub window_title: WindowTitle,
    pub system_info: SystemInfo,
    pub keyboard_layout: KeyboardLayout,
    pub keyboard_submap: KeyboardSubmap,
    pub tray: TrayModule,
    pub clock: Clock,
    pub tempo: Tempo,
    pub privacy: Privacy,
    pub notifications: Notifications,
    pub settings: Settings,
    pub media_player: MediaPlayer,
    pub popup_state: PopupState,
}

#[derive(Debug, Clone)]
pub enum Message {
    ConfigChanged(Box<Config>),
    ToggleMenu(MenuType, Id, ButtonUIRef),
    CloseMenu(Id),
    Custom(String, custom_module::Message),
    Updates(modules::updates::Message),
    Workspaces(modules::workspaces::Message),
    WindowTitle(modules::window_title::Message),
    SystemInfo(modules::system_info::Message),
    KeyboardLayout(modules::keyboard_layout::Message),
    KeyboardSubmap(modules::keyboard_submap::Message),
    Tray(modules::tray::Message),
    Clock(modules::clock::Message),
    Tempo(modules::tempo::Message),
    Privacy(modules::privacy::Message),
    Settings(modules::settings::Message),
    Notifications(modules::notifications::Message),
    MediaPlayer(modules::media_player::Message),
    OutputEvent((OutputEvent, WlOutput)),
    PopupTick,
    PopupDismiss(u32),
    PopupClicked(u32),
    CloseAllMenus,
    ResumeFromSleep,
    None,
}

impl App {
    pub fn new(
        (logger, config, config_path): (LoggerHandle, Config, PathBuf),
    ) -> impl FnOnce() -> (Self, Task<Message>) {
        move || {
            let (outputs, task) = Outputs::new(
                config.appearance.style,
                config.position,
                config.layer,
                config.appearance.scale_factor,
            );

            let custom = config
                .custom_modules
                .clone()
                .into_iter()
                .map(|o| (o.name.clone(), Custom::new(o)))
                .collect();

            (
                App {
                    config_path,
                    theme: AshellTheme::new(config.position, &config.appearance),
                    logger,
                    general_config: GeneralConfig {
                        outputs: config.outputs,
                        modules: config.modules,
                        layer: config.layer,
                        enable_esc_key: config.enable_esc_key,
                    },
                    outputs,
                    custom,
                    updates: config.updates.map(Updates::new),
                    workspaces: Workspaces::new(config.workspaces),
                    window_title: WindowTitle::new(config.window_title),
                    system_info: SystemInfo::new(config.system_info),
                    keyboard_layout: KeyboardLayout::new(config.keyboard_layout),
                    keyboard_submap: KeyboardSubmap::default(),
                    popup_state: PopupState::new(&config.notifications),
                    notifications: Notifications::new(config.notifications.clone()),
                    tray: TrayModule::default(),
                    clock: Clock::new(config.clock),
                    tempo: Tempo::new(config.tempo),
                    privacy: Privacy::default(),
                    settings: Settings::new(config.settings),
                    media_player: MediaPlayer::new(config.media_player),
                },
                task,
            )
        }
    }

    fn refresh_config(&mut self, config: Box<Config>) {
        self.general_config = GeneralConfig {
            outputs: config.outputs,
            modules: config.modules,
            layer: config.layer,
            enable_esc_key: config.enable_esc_key,
        };
        self.theme = AshellTheme::new(config.position, &config.appearance);
        let custom = config
            .custom_modules
            .into_iter()
            .map(|o| (o.name.clone(), Custom::new(o)))
            .collect();

        self.custom = custom;
        self.updates = config.updates.map(Updates::new);

        // ignore task, since config change should not generate any
        let _ = self
            .workspaces
            .update(modules::workspaces::Message::ConfigReloaded(
                config.workspaces,
            ))
            .map(Message::Workspaces);

        self.window_title
            .update(modules::window_title::Message::ConfigReloaded(
                config.window_title,
            ));

        self.system_info = SystemInfo::new(config.system_info);

        let _ = self
            .keyboard_layout
            .update(modules::keyboard_layout::Message::ConfigReloaded(
                config.keyboard_layout,
            ))
            .map(Message::KeyboardLayout);

        self.notifications.config = config.notifications.clone();
        self.popup_state.update_config(&config.notifications);
        self.keyboard_submap = KeyboardSubmap::default();
        self.clock = Clock::new(config.clock);
        self.tempo = Tempo::new(config.tempo);
        self.settings
            .update(modules::settings::Message::ConfigReloaded(config.settings));
        self.media_player
            .update(modules::media_player::Message::ConfigReloaded(
                config.media_player,
            ));
    }

    pub fn title(&self, _id: Id) -> String {
        String::from("ashell")
    }

    pub fn theme(&self, _id: Id) -> Theme {
        self.theme.get_theme().clone()
    }

    pub fn style(&self, theme: &Theme) -> Appearance {
        Appearance {
            background_color: Color::TRANSPARENT,
            text_color: theme.palette().text,
            icon_color: theme.palette().text,
        }
    }

    pub fn scale_factor(&self, _id: Id) -> f64 {
        self.theme.scale_factor
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ConfigChanged(config) => {
                info!("New config: {config:?}");
                let mut tasks = Vec::new();
                info!(
                    "Current outputs: {:?}, new outputs: {:?}",
                    self.general_config.outputs, config.outputs
                );
                if self.general_config.outputs != config.outputs
                    || self.theme.bar_position != config.position
                    || self.theme.bar_style != config.appearance.style
                    || self.theme.scale_factor != config.appearance.scale_factor
                    || self.general_config.layer != config.layer
                {
                    warn!("Outputs changed, syncing");
                    tasks.push(self.outputs.sync(
                        config.appearance.style,
                        &config.outputs,
                        config.position,
                        config.layer,
                        config.appearance.scale_factor,
                    ));
                }

                self.logger.set_new_spec(get_log_spec(&config.log_level));
                self.refresh_config(config);

                Task::batch(tasks)
            }
            Message::ToggleMenu(menu_type, id, button_ui_ref) => {
                let mut cmd = vec![];
                match &menu_type {
                    MenuType::Updates => {
                        if let Some(updates) = self.updates.as_mut() {
                            updates.update(modules::updates::Message::MenuOpened);
                        }
                    }
                    MenuType::Tray(name) => {
                        self.tray
                            .update(modules::tray::Message::MenuOpened(name.clone()));
                    }
                    MenuType::Notifications => {
                        self.notifications
                            .update(modules::notifications::Message::MenuOpened);
                        self.popup_state.entries.clear();
                    }
                    MenuType::Settings => {
                        cmd.push(
                            match self.settings.update(modules::settings::Message::MenuOpened) {
                                modules::settings::Action::Command(task) => {
                                    task.map(Message::Settings)
                                }
                                _ => Task::none(),
                            },
                        );
                    }
                    _ => {}
                };
                cmd.push(self.outputs.toggle_menu(
                    id,
                    menu_type,
                    button_ui_ref,
                    self.general_config.enable_esc_key,
                ));

                Task::batch(cmd)
            }
            Message::CloseMenu(id) => self
                .outputs
                .close_menu(id, self.general_config.enable_esc_key),
            Message::Custom(name, msg) => {
                if let Some(custom) = self.custom.get_mut(&name) {
                    custom.update(msg);
                }

                Task::none()
            }
            Message::Updates(msg) => {
                if let Some(updates) = self.updates.as_mut() {
                    match updates.update(msg) {
                        modules::updates::Action::None => Task::none(),
                        modules::updates::Action::CheckForUpdates(task) => {
                            task.map(Message::Updates)
                        }
                        modules::updates::Action::CloseMenu(id, task) => Task::batch(vec![
                            task.map(Message::Updates),
                            self.outputs.close_menu_if(
                                id,
                                MenuType::Updates,
                                self.general_config.enable_esc_key,
                            ),
                        ]),
                    }
                } else {
                    Task::none()
                }
            }
            Message::Workspaces(msg) => self.workspaces.update(msg).map(Message::Workspaces),
            Message::WindowTitle(msg) => {
                self.window_title.update(msg);
                Task::none()
            }
            Message::SystemInfo(msg) => {
                self.system_info.update(msg);
                Task::none()
            }
            Message::KeyboardLayout(message) => self
                .keyboard_layout
                .update(message)
                .map(Message::KeyboardLayout),
            Message::KeyboardSubmap(message) => {
                self.keyboard_submap.update(message);
                Task::none()
            }
            Message::Tray(msg) => match self.tray.update(msg) {
                modules::tray::Action::None => Task::none(),
                modules::tray::Action::ToggleMenu(name, id, button_ui_ref) => {
                    self.outputs.toggle_menu(
                        id,
                        MenuType::Tray(name),
                        button_ui_ref,
                        self.general_config.enable_esc_key,
                    )
                }
                modules::tray::Action::TrayMenuCommand(task) => Task::batch(vec![
                    self.outputs
                        .close_all_menus(self.general_config.enable_esc_key),
                    task.map(Message::Tray),
                ]),
                modules::tray::Action::CloseTrayMenu(name) => self
                    .outputs
                    .close_all_menu_if(MenuType::Tray(name), self.general_config.enable_esc_key),
            },
            Message::Clock(message) => {
                self.clock.update(message);
                Task::none()
            }
            Message::Tempo(message) => match self.tempo.update(message) {
                modules::tempo::Action::None => Task::none(),
            },
            Message::Privacy(msg) => {
                self.privacy.update(msg);
                Task::none()
            }
            Message::Settings(message) => match self.settings.update(message) {
                modules::settings::Action::None => Task::none(),
                modules::settings::Action::Command(task) => task.map(Message::Settings),
                modules::settings::Action::CloseMenu(id) => self
                    .outputs
                    .close_menu(id, self.general_config.enable_esc_key),
                modules::settings::Action::RequestKeyboard(id) => self.outputs.request_keyboard(id),
                modules::settings::Action::ReleaseKeyboard(id) => self.outputs.release_keyboard(id),
                modules::settings::Action::ReleaseKeyboardWithCommand(id, task) => {
                    Task::batch(vec![
                        task.map(Message::Settings),
                        self.outputs.release_keyboard(id),
                    ])
                }
            },
            Message::OutputEvent((event, wl_output)) => match event {
                iced::event::wayland::OutputEvent::Created(info) => {
                    info!("Output created: {info:?}");
                    let name = info
                        .as_ref()
                        .and_then(|info| info.description.as_deref())
                        .unwrap_or("");

                    self.outputs.add(
                        self.theme.bar_style,
                        &self.general_config.outputs,
                        self.theme.bar_position,
                        self.general_config.layer,
                        name,
                        wl_output,
                        self.theme.scale_factor,
                    )
                }
                iced::event::wayland::OutputEvent::Removed => {
                    info!("Output destroyed");
                    self.outputs.remove(
                        self.theme.bar_style,
                        self.theme.bar_position,
                        self.general_config.layer,
                        wl_output,
                        self.theme.scale_factor,
                    )
                }
                _ => Task::none(),
            },
            Message::Notifications(msg) => match self.notifications.update(msg) {
                modules::notifications::Action::None => Task::none(),
                modules::notifications::Action::EmitSignal(task) => {
                    task.map(Message::Notifications)
                }
                modules::notifications::Action::ShowPopup(notification) => {
                    if !self.notifications.config.popup_enabled
                        || self.outputs.notification_menu_is_open()
                    {
                        return Task::none();
                    }
                    let duration =
                        Duration::from_millis(self.notifications.config.popup_duration_ms);
                    self.popup_state.enqueue(notification, duration);
                    Task::none()
                }
            },
            Message::MediaPlayer(msg) => match self.media_player.update(msg) {
                modules::media_player::Action::None => Task::none(),
                modules::media_player::Action::Command(task) => task.map(Message::MediaPlayer),
            },
            Message::PopupTick => {
                self.popup_state.tick();
                Task::none()
            }
            Message::PopupDismiss(id) => {
                self.popup_state.dismiss(id);
                // Also dismiss from notification service
                match self
                    .notifications
                    .update(modules::notifications::Message::Dismiss(id))
                {
                    modules::notifications::Action::EmitSignal(task) => {
                        task.map(Message::Notifications)
                    }
                    _ => Task::none(),
                }
            }
            Message::PopupClicked(id) => {
                // Check if notification has a default action
                let has_default = self
                    .popup_state
                    .entries
                    .iter()
                    .find(|e| e.notification.id == id)
                    .is_some_and(|e| e.notification.actions.iter().any(|(k, _)| k == "default"));

                if has_default {
                    self.popup_state.dismiss(id);
                    match self.notifications.update(
                        modules::notifications::Message::InvokeAction(id, "default".to_string()),
                    ) {
                        modules::notifications::Action::EmitSignal(task) => {
                            task.map(Message::Notifications)
                        }
                        _ => Task::none(),
                    }
                } else {
                    Task::none()
                }
            }
            Message::CloseAllMenus => {
                if self.outputs.menu_is_open() {
                    self.outputs
                        .close_all_menus(self.general_config.enable_esc_key)
                } else {
                    Task::none()
                }
            }
            Message::ResumeFromSleep => self.outputs.sync(
                self.theme.bar_style,
                &self.general_config.outputs,
                self.theme.bar_position,
                self.general_config.layer,
                self.theme.scale_factor,
            ),
            Message::None => Task::none(),
        }
    }

    pub fn view(&'_ self, id: Id) -> Element<'_, Message> {
        match self.outputs.has(id) {
            Some(HasOutput::Main) => {
                let [left, center, right] = self.modules_section(id, &self.theme);

                let centerbox = Centerbox::new([left, center, right])
                    .spacing(self.theme.space.xxs)
                    .width(Length::Fill)
                    .align_items(Alignment::Center)
                    .height(if self.theme.bar_style == AppearanceStyle::Islands {
                        HEIGHT
                    } else {
                        HEIGHT - 8.
                    } as f32)
                    .padding(if self.theme.bar_style == AppearanceStyle::Islands {
                        [self.theme.space.xxs, self.theme.space.xxs]
                    } else {
                        [0, 0]
                    });

                let status_bar = container(centerbox).style(|t: &Theme| container::Style {
                    background: match self.theme.bar_style {
                        AppearanceStyle::Gradient => Some({
                            let start_color =
                                t.palette().background.scale_alpha(self.theme.opacity);

                            let start_color = if self.outputs.menu_is_open() {
                                darken_color(start_color, self.theme.menu.backdrop)
                            } else {
                                start_color
                            };

                            let end_color = if self.outputs.menu_is_open() {
                                backdrop_color(self.theme.menu.backdrop)
                            } else {
                                Color::TRANSPARENT
                            };

                            Gradient::Linear(
                                Linear::new(Radians(PI))
                                    .add_stop(
                                        0.0,
                                        match self.theme.bar_position {
                                            Position::Top => start_color,
                                            Position::Bottom => end_color,
                                        },
                                    )
                                    .add_stop(
                                        1.0,
                                        match self.theme.bar_position {
                                            Position::Top => end_color,
                                            Position::Bottom => start_color,
                                        },
                                    ),
                            )
                            .into()
                        }),
                        AppearanceStyle::Solid => Some({
                            let bg = t.palette().background.scale_alpha(self.theme.opacity);
                            if self.outputs.menu_is_open() {
                                darken_color(bg, self.theme.menu.backdrop)
                            } else {
                                bg
                            }
                            .into()
                        }),
                        AppearanceStyle::Islands => {
                            if self.outputs.menu_is_open() {
                                Some(backdrop_color(self.theme.menu.backdrop).into())
                            } else {
                                None
                            }
                        }
                    },
                    ..Default::default()
                });

                if self.outputs.menu_is_open() {
                    mouse_area(status_bar)
                        .on_release(Message::CloseMenu(id))
                        .into()
                } else {
                    status_bar.into()
                }
            }
            Some(HasOutput::Menu(menu_info)) => match menu_info {
                Some((MenuType::Updates, button_ui_ref)) => {
                    if let Some(updates) = self.updates.as_ref() {
                        self.menu_wrapper(
                            id,
                            updates.menu_view(id, &self.theme).map(Message::Updates),
                            *button_ui_ref,
                        )
                    } else {
                        Row::new().into()
                    }
                }
                Some((MenuType::Tray(name), button_ui_ref)) => self.menu_wrapper(
                    id,
                    self.tray.menu_view(&self.theme, name).map(Message::Tray),
                    *button_ui_ref,
                ),
                Some((MenuType::Settings, button_ui_ref)) => self.menu_wrapper(
                    id,
                    self.settings
                        .menu_view(id, &self.theme, self.theme.bar_position)
                        .map(Message::Settings),
                    *button_ui_ref,
                ),
                Some((MenuType::MediaPlayer, button_ui_ref)) => self.menu_wrapper(
                    id,
                    self.media_player
                        .menu_view(&self.theme)
                        .map(Message::MediaPlayer),
                    *button_ui_ref,
                ),
                Some((MenuType::SystemInfo, button_ui_ref)) => self.menu_wrapper(
                    id,
                    self.system_info
                        .menu_view(&self.theme)
                        .map(Message::SystemInfo),
                    *button_ui_ref,
                ),

                Some((MenuType::Notifications, button_ui_ref)) => self.menu_wrapper(
                    id,
                    self.notifications
                        .menu_view(id, &self.theme)
                        .map(Message::Notifications),
                    *button_ui_ref,
                ),
                Some((MenuType::Tempo, button_ui_ref)) => self.menu_wrapper(
                    id,
                    self.tempo.menu_view(&self.theme).map(Message::Tempo),
                    *button_ui_ref,
                ),
                None => Row::new().into(),
            },
            Some(HasOutput::Popup) => self.render_popup_bubble(),
            None => Row::new().into(),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![
            Subscription::batch(self.modules_subscriptions(&self.general_config.modules.left)),
            Subscription::batch(self.modules_subscriptions(&self.general_config.modules.center)),
            Subscription::batch(self.modules_subscriptions(&self.general_config.modules.right)),
            config::subscription(&self.config_path),
            crate::services::logind::LogindService::subscribe().map(|event| match event {
                crate::services::ServiceEvent::Update(_) => Message::ResumeFromSleep,
                _ => Message::None,
            }),
            listen_with(move |evt, _, _| match evt {
                iced::Event::PlatformSpecific(iced::event::PlatformSpecific::Wayland(
                    WaylandEvent::Output(event, wl_output),
                )) => {
                    debug!("Wayland event: {event:?}");
                    Some(Message::OutputEvent((event, wl_output)))
                }
                iced::Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) => {
                    debug!("Keyboard event received: {key:?}");
                    if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
                        debug!("ESC key pressed, closing all menus");
                        Some(Message::CloseAllMenus)
                    } else {
                        None
                    }
                }
                _ => None,
            }),
        ];

        if self.popup_state.is_active() {
            subs.push(
                iced::time::every(Duration::from_millis(16)).map(|_| Message::PopupTick),
            );
        }

        Subscription::batch(subs)
    }

    fn render_popup_bubble(&self) -> Element<'_, Message> {
        use iced::widget::{Column, Image, Svg, column, container, horizontal_rule, row, text};
        use iced::Border;
        use crate::components::icons::{StaticIcon, icon_button};
        use crate::services::notifications::NotificationIcon;

        if self.popup_state.entries.is_empty() {
            return container(Row::new())
                .width(Length::Shrink)
                .height(Length::Shrink)
                .into();
        }

        let now = Instant::now(); // single timestamp for entire frame
        let bubble_progress = self.popup_state.bubble_progress_at(now);
        let theme = &self.theme;

        let mut items: Vec<Element<'_, Message>> = Vec::new();
        for (i, entry) in self.popup_state.entries.iter().enumerate() {
            let entry_progress = self.popup_state.entry_progress_staggered_at(entry, i, now);
            let entry_height = 80.0 * entry_progress.min(1.0); // clamp overshoot for clip

            let n = &entry.notification;
            let id = n.id;
            let time = n.timestamp.format("%H:%M").to_string();
            let has_default_action = n.actions.iter().any(|(k, _)| k == "default");

            // Icon element
            let icon_element: Option<Element<'_, Message>> =
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

            let mut text_col = column!(
                row!(
                    text(&n.app_name).size(theme.font_size.xs),
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
                text(&n.summary).size(theme.font_size.sm),
            )
            .spacing(2)
            .width(Length::Fill);

            if !n.body.is_empty() {
                let truncated = crate::utils::truncate_chars(&n.body, 100);
                text_col = text_col.push(text(truncated.to_owned()).size(theme.font_size.xs));
            }

            let mut content_row = row!()
                .spacing(theme.space.xs)
                .align_y(Alignment::Center);
            if let Some(icon_el) = icon_element {
                content_row = content_row.push(icon_el);
            }
            content_row = content_row
                .push(text_col)
                .push(
                    icon_button::<Message>(theme, StaticIcon::Close)
                        .on_press(Message::PopupDismiss(id)),
                );

            let notification_content: Element<'_, Message> = container(content_row)
                .padding([theme.space.xs, 0])
                .into();

            let notification_or_mouse_area: Element<'_, Message> = if has_default_action {
                iced::widget::mouse_area(notification_content)
                    .on_press(Message::PopupClicked(id))
                    .into()
            } else {
                notification_content
            };

            // Build per-entry column with separator (after first entry)
            let mut entry_col = Column::new();
            if i > 0 {
                entry_col = entry_col.push(horizontal_rule(1));
            }
            entry_col = entry_col.push(notification_or_mouse_area);

            // Per-entry clip wrapper for staggered reveal
            let clipped_entry = container(entry_col)
                .clip(true)
                .max_height(entry_height)
                .width(Length::Fill);

            items.push(clipped_entry.into());
        }

        let content = Column::with_children(items)
            .spacing(2)
            .padding([0, theme.space.xs]);

        // Animated horizontal padding: squeeze content narrow then expand to rest
        let width_progress = bubble_progress.min(1.0);
        let extra_h_pad = (1.0 - width_progress) * 40.0;

        // Styled bubble at full content height
        // Use tighter top padding and smaller top border radius for flush appearance
        let styled_bubble = container(content)
            .padding(iced::Padding {
                top: if theme.bar_style == AppearanceStyle::Islands {
                    theme.space.md as f32
                } else {
                    0.0
                },
                bottom: theme.space.md as f32,
                left: theme.space.md as f32 + extra_h_pad,
                right: theme.space.md as f32 + extra_h_pad,
            })
            .style(move |t: &iced::Theme| iced::widget::container::Style {
                background: Some(
                    t.palette()
                        .background
                        .scale_alpha(theme.menu.opacity)
                        .into(),
                ),
                border: Border {
                    color: t
                        .extended_palette()
                        .secondary
                        .base
                        .color
                        .scale_alpha(theme.menu.opacity),
                    width: 1.,
                    radius: if theme.bar_style == AppearanceStyle::Islands {
                        [theme.radius.lg as f32; 4].into()
                    } else {
                        [0.0, 0.0, theme.radius.lg as f32, theme.radius.lg as f32].into()
                    },
                },
                ..Default::default()
            })
            .width(Length::Fill);

        // Fixed surface height: locks the Wayland surface size to prevent per-frame resizes.
        // Content is aligned toward the bar edge; the transparent gap is invisible on overlay.
        let top_pad = if theme.bar_style == AppearanceStyle::Islands {
            theme.space.md as f32
        } else {
            0.0
        };
        let bottom_pad = theme.space.md as f32;
        let target_height = self.popup_state.target_surface_height(top_pad, bottom_pad);

        match self.theme.bar_position {
            Position::Top => container(styled_bubble)
                .clip(true)
                .align_top(target_height)
                .into(),
            Position::Bottom => container(styled_bubble)
                .clip(true)
                .align_bottom(target_height)
                .into(),
        }
    }
}
