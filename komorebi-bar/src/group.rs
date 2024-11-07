use eframe::egui::Frame;
use eframe::egui::InnerResponse;
use eframe::egui::Margin;
use eframe::egui::Rounding;
use eframe::egui::Ui;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum Grouping {
    /// No grouping is applied
    None,
    /// Widgets are grouped individually
    Widget(GroupingConfig),
    /// Widgets are grouped on each side
    Side(GroupingConfig),
}

impl Grouping {
    pub fn apply_on_widget<R>(
        &mut self,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<R> {
        match self {
            Self::None => InnerResponse {
                inner: add_contents(ui),
                response: ui.response().clone(),
            },
            Self::Widget(_config) => {
                Frame::none()
                    //.fill(Color32::from_black_alpha(255u8))
                    .outer_margin(Margin::symmetric(0.0, 0.0))
                    .inner_margin(Margin::symmetric(7.0, 2.0))
                    .rounding(Rounding::same(15.0))
                    .stroke(ui.style().visuals.widgets.noninteractive.bg_stroke)
                    .show(ui, add_contents)
            }
            Self::Side(_config) => InnerResponse {
                inner: add_contents(ui),
                response: ui.response().clone(),
            },
        }
    }
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct GroupingConfig {
    pub rounding: Option<BorderRadius>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct BorderRadius {
    /// Radius of the rounding of the North-West (left top) corner.
    pub nw: f32,
    /// Radius of the rounding of the North-East (right top) corner.
    pub ne: f32,
    /// Radius of the rounding of the South-West (left bottom) corner.
    pub sw: f32,
    /// Radius of the rounding of the South-East (right bottom) corner.
    pub se: f32,
}

impl From<BorderRadius> for Rounding {
    fn from(value: BorderRadius) -> Self {
        Self {
            nw: value.nw,
            ne: value.ne,
            sw: value.sw,
            se: value.se,
        }
    }
}
