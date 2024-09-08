use crate::img_to_texture;
use crate::widget::BarWidget;
use crate::KomorebiNotificationState;
use eframe::egui::Context;
use eframe::egui::Image;
use eframe::egui::Label;
use eframe::egui::SelectableLabel;
use eframe::egui::Sense;
use eframe::egui::Ui;
use komorebi_client::CycleDirection;
use komorebi_client::SocketMessage;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Copy, Clone, Debug)]
pub struct KomorebiConfig {
    pub enable: bool,
    pub monitor_index: usize,
    pub workspaces: KomorebiWorkspacesConfig,
    pub layout: KomorebiLayoutConfig,
    pub focused_window: KomorebiFocusedWindowConfig,
}

#[derive(Copy, Clone, Debug)]
pub struct KomorebiWorkspacesConfig {
    pub enable: bool,
    pub hide_empty_workspaces: bool,
}

#[derive(Copy, Clone, Debug)]
pub struct KomorebiLayoutConfig {
    pub enable: bool,
}

#[derive(Copy, Clone, Debug)]
pub struct KomorebiFocusedWindowConfig {
    pub enable: bool,
    pub show_icon: bool,
}

impl From<KomorebiConfig> for Komorebi {
    fn from(value: KomorebiConfig) -> Self {
        Self {
            enable: value.enable,
            komorebi_notification_state: Rc::new(RefCell::new(KomorebiNotificationState {
                selected_workspace: String::new(),
                focused_window_title: String::new(),
                focused_window_pid: None,
                focused_window_icon: None,
                layout: String::new(),
                workspaces: vec![],
                monitor_index: value.monitor_index,
                hide_empty_workspaces: value.workspaces.hide_empty_workspaces,
            })),
            workspaces: value.workspaces,
            layout: value.layout,
            focused_window: value.focused_window,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Komorebi {
    pub enable: bool,
    pub komorebi_notification_state: Rc<RefCell<KomorebiNotificationState>>,
    pub workspaces: KomorebiWorkspacesConfig,
    pub layout: KomorebiLayoutConfig,
    pub focused_window: KomorebiFocusedWindowConfig,
}

impl BarWidget for Komorebi {
    fn render(&mut self, ctx: &Context, ui: &mut Ui) {
        if self.enable {
            let mut komorebi_notification_state = self.komorebi_notification_state.borrow_mut();
            let mut update = None;

            if self.workspaces.enable {
                for (i, ws) in komorebi_notification_state.workspaces.iter().enumerate() {
                    if ui
                        .add(SelectableLabel::new(
                            komorebi_notification_state.selected_workspace.eq(ws),
                            ws.to_string(),
                        ))
                        .clicked()
                    {
                        update = Some(ws.to_string());
                        komorebi_client::send_message(&SocketMessage::MouseFollowsFocus(false))
                            .unwrap();
                        komorebi_client::send_message(&SocketMessage::FocusWorkspaceNumber(i))
                            .unwrap();
                        // TODO: store MFF value from state and restore that here instead of "true"
                        komorebi_client::send_message(&SocketMessage::MouseFollowsFocus(true))
                            .unwrap();
                        komorebi_client::send_message(&SocketMessage::Retile).unwrap();
                    }
                }

                if let Some(update) = update {
                    komorebi_notification_state.selected_workspace = update;
                }

                ui.add_space(10.0);
            }

            if self.layout.enable {
                if ui
                    .add(
                        Label::new(&komorebi_notification_state.layout)
                            .selectable(false)
                            .sense(Sense::click()),
                    )
                    .clicked()
                {
                    komorebi_client::send_message(&SocketMessage::CycleLayout(
                        CycleDirection::Next,
                    ))
                    .unwrap();
                }

                ui.add_space(10.0);
            }

            if self.focused_window.enable {
                if self.focused_window.show_icon {
                    if let Some(img) = &komorebi_notification_state.focused_window_icon {
                        ui.add(
                            Image::from(&img_to_texture(ctx, img))
                                .maintain_aspect_ratio(true)
                                .max_height(15.0),
                        );
                    }
                }

                ui.add(
                    Label::new(&komorebi_notification_state.focused_window_title).selectable(false),
                );

                ui.add_space(10.0);
            }
        }
    }
}
