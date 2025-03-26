#![allow(dead_code)]
#![allow(unused_imports)]

use crate::config::LabelPrefix;
use crate::render::RenderConfig;
use crate::selected_frame::SelectableFrame;
use crate::widgets::widget::BarWidget;
use eframe::egui::text::LayoutJob;
use eframe::egui::Align;
use eframe::egui::Context;
use eframe::egui::Label;
use eframe::egui::TextFormat;
use eframe::egui::Ui;
use num_derive::FromPrimitive;
use serde::Deserialize;
use serde::Serialize;
use std::fmt;
use std::process::Command;
use std::time::Duration;
use std::time::Instant;
use sysinfo::Networks;
use windows::core::Result;
use windows::Win32::UI::Input::KeyboardAndMouse::SendInput;
use windows::Win32::UI::Input::KeyboardAndMouse::INPUT;
use windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0;
use windows::Win32::UI::Input::KeyboardAndMouse::INPUT_KEYBOARD;
use windows::Win32::UI::Input::KeyboardAndMouse::KEYBDINPUT;
use windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS;
use windows::Win32::UI::Input::KeyboardAndMouse::KEYEVENTF_KEYUP;
use windows::Win32::UI::Input::KeyboardAndMouse::VK_LWIN;

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct StartMenuConfig {
    pub enable: bool,
    pub label_prefix: Option<LabelPrefix>,
}

impl From<StartMenuConfig> for StartMenu {
    fn from(value: StartMenuConfig) -> Self {
        Self {
            enable: value.enable,
            label_prefix: value.label_prefix.unwrap_or(LabelPrefix::Icon),
        }
    }
}

pub struct StartMenu {
    pub enable: bool,
    label_prefix: LabelPrefix,
}

impl StartMenu {
    pub fn toggle_start_menu() {
        // Prepare the inputs
        let inputs = [
            // Press the left windows key
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_LWIN,
                        wScan: 0,
                        dwFlags: KEYBD_EVENT_FLAGS(0), // key down
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            // Release the left windows key
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_LWIN,
                        wScan: 0,
                        dwFlags: KEYEVENTF_KEYUP, // key up
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];

        // Send the inputs
        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
    }
}

impl BarWidget for StartMenu {
    fn render(&mut self, ctx: &Context, ui: &mut Ui, config: &mut RenderConfig) {
        if self.enable {
            // widget spacing: make sure to use the same config to call the apply_on_widget function
            let mut render_config = config.clone();
            let mut layout_job = LayoutJob::simple(
                match self.label_prefix {
                    LabelPrefix::Icon | LabelPrefix::IconAndText => {
                        egui_phosphor::regular::YIN_YANG.to_string()
                    }
                    LabelPrefix::None | LabelPrefix::Text => String::new(),
                },
                config.icon_font_id.clone(),
                ctx.style().visuals.selection.stroke.color,
                100.0,
            );

            layout_job.append(
                &String::from("Start"),
                10.0,
                TextFormat {
                    font_id: config.text_font_id.clone(),
                    color: ctx.style().visuals.text_color(),
                    valign: Align::Center,
                    ..Default::default()
                },
            );

            render_config.apply_on_widget(false, ui, |ui| {
                if SelectableFrame::new(false)
                    .show(ui, |ui| ui.add(Label::new(layout_job).selectable(false)))
                    .clicked()
                {
                    StartMenu::toggle_start_menu();
                }
            });

            // widget spacing: pass on the config that was use for calling the apply_on_widget function
            *config = render_config.clone();
        }
    }
}
