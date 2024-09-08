use crate::widget::BarWidget;
use eframe::egui::Context;
use eframe::egui::Label;
use eframe::egui::Sense;
use eframe::egui::Ui;
use windows::Media::Control::GlobalSystemMediaTransportControlsSessionManager;

#[derive(Copy, Clone, Debug)]
pub struct MediaConfig {
    pub enable: bool,
}

impl From<MediaConfig> for Media {
    fn from(value: MediaConfig) -> Self {
        Self::new(value.enable)
    }
}

#[derive(Clone, Debug)]
pub struct Media {
    pub enable: bool,
    pub session_manager: GlobalSystemMediaTransportControlsSessionManager,
}

impl Media {
    pub fn new(enable: bool) -> Self {
        Self {
            enable,
            session_manager: GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                .unwrap()
                .get()
                .unwrap(),
        }
    }

    pub fn toggle(&self) {
        if let Ok(session) = self.session_manager.GetCurrentSession() {
            if let Ok(op) = session.TryTogglePlayPauseAsync() {
                op.get().unwrap_or_default();
            }
        }
    }

    fn output(&mut self) -> Vec<String> {
        if let Ok(session) = self.session_manager.GetCurrentSession() {
            if let Ok(operation) = session.TryGetMediaPropertiesAsync() {
                if let Ok(properties) = operation.get() {
                    if let (Ok(artist), Ok(title)) = (properties.Artist(), properties.Title()) {
                        if artist.is_empty() {
                            return vec![format!("{title}")];
                        }

                        if title.is_empty() {
                            return vec![format!("{artist}")];
                        }

                        return vec![format!("{artist} - {title}")];
                    }
                }
            }
        }

        vec![]
    }
}

impl BarWidget for Media {
    fn render(&mut self, _ctx: &Context, ui: &mut Ui) {
        if self.enable {
            for output in self.output() {
                if ui
                    .add(
                        Label::new(format!("{} {output}", egui_phosphor::regular::HEADPHONES))
                            .selectable(false)
                            .sense(Sense::click()),
                    )
                    .clicked()
                {
                    self.toggle();
                }

                ui.add_space(10.0);
            }
        }
    }
}
