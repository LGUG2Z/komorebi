use crate::komorebi::KomorebiLayout;
use crate::selected_frame::SelectableFrame;
use eframe::egui::text::LayoutJob;
use eframe::egui::vec2;
use eframe::egui::Context;
use eframe::egui::FontId;
use eframe::egui::Label;
use eframe::egui::Response;
use eframe::egui::Rounding;
use eframe::egui::Sense;
use eframe::egui::Stroke;
use eframe::egui::TextFormat;
use eframe::egui::Ui;
use eframe::egui::Vec2;

impl KomorebiLayout {
    pub fn show_icon(&mut self, font_id: FontId, ctx: &Context, ui: &mut Ui) -> Option<LayoutJob> {
        match self {
            KomorebiLayout::Default(layout) => {
                let rounding = Rounding::same(1.0);

                // paint custom icons for the layout
                let size = Vec2::splat(font_id.size);
                let (response, painter) = ui.allocate_painter(size, Sense::hover());
                let color = ctx.style().visuals.selection.stroke.color;
                let stroke = Stroke::new(1.0, color);
                let mut rect = response.rect;
                rect = rect.shrink(stroke.width);
                let c = rect.center();
                let r = rect.width() / 2.0;

                // TODO: add tooltip to the icon if no text is shown
                //response.on_hover_text(layout.to_string());

                match layout {
                    komorebi_client::DefaultLayout::BSP => {
                        painter.rect_stroke(rect, rounding, stroke);
                        painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], stroke);
                        painter.line_segment([c, c + vec2(r, 0.0)], stroke);
                        painter
                            .line_segment([c + vec2(r / 2.0, 0.0), c + vec2(r / 2.0, r)], stroke);
                    }
                    komorebi_client::DefaultLayout::Columns => {
                        painter.rect_stroke(rect, rounding, stroke);
                        painter.line_segment([c - vec2(r / 2.0, r), c + vec2(-r / 2.0, r)], stroke);
                        painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], stroke);
                        painter.line_segment([c - vec2(-r / 2.0, r), c + vec2(r / 2.0, r)], stroke);
                    }
                    komorebi_client::DefaultLayout::Rows => {
                        painter.rect_stroke(rect, rounding, stroke);
                        painter.line_segment([c - vec2(r, r / 2.0), c + vec2(r, -r / 2.0)], stroke);
                        painter.line_segment([c - vec2(r, 0.0), c + vec2(r, 0.0)], stroke);
                        painter.line_segment([c - vec2(r, -r / 2.0), c + vec2(r, r / 2.0)], stroke);
                    }
                    komorebi_client::DefaultLayout::VerticalStack => {
                        painter.rect_stroke(rect, rounding, stroke);
                        painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], stroke);
                        painter.line_segment([c, c + vec2(r, 0.0)], stroke);
                    }
                    komorebi_client::DefaultLayout::RightMainVerticalStack => {
                        painter.rect_stroke(rect, rounding, stroke);
                        painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], stroke);
                        painter.line_segment([c - vec2(r, 0.0), c], stroke);
                    }
                    komorebi_client::DefaultLayout::HorizontalStack => {
                        painter.rect_stroke(rect, rounding, stroke);
                        painter.line_segment([c - vec2(r, 0.0), c + vec2(r, 0.0)], stroke);
                        painter.line_segment([c, c + vec2(0.0, r)], stroke);
                    }
                    komorebi_client::DefaultLayout::UltrawideVerticalStack => {
                        painter.rect_stroke(rect, rounding, stroke);
                        painter.line_segment([c - vec2(r / 2.0, r), c + vec2(-r / 2.0, r)], stroke);
                        painter.line_segment([c + vec2(r / 2.0, 0.0), c + vec2(r, 0.0)], stroke);
                        painter.line_segment([c - vec2(-r / 2.0, r), c + vec2(r / 2.0, r)], stroke);
                    }
                    komorebi_client::DefaultLayout::Grid => {
                        painter.rect_stroke(rect, rounding, stroke);
                        painter.line_segment([c - vec2(r, 0.0), c + vec2(r, 0.0)], stroke);
                        painter.line_segment([c - vec2(0.0, r), c + vec2(0.0, r)], stroke);
                    }
                }

                None
            }
            KomorebiLayout::Floating => Some(LayoutJob::simple(
                egui_phosphor::regular::BROWSERS.to_string(),
                font_id.clone(),
                ctx.style().visuals.selection.stroke.color,
                100.0,
            )),
            KomorebiLayout::Paused => Some(LayoutJob::simple(
                egui_phosphor::regular::PAUSE_CIRCLE.to_string(),
                font_id.clone(),
                ctx.style().visuals.selection.stroke.color,
                100.0,
            )),
            KomorebiLayout::Custom => Some(LayoutJob::simple(
                egui_phosphor::regular::USER_SQUARE.to_string(),
                font_id.clone(),
                ctx.style().visuals.selection.stroke.color,
                100.0,
            )),
        }
    }

    pub fn show(&mut self, font_id: FontId, ctx: &Context, ui: &mut Ui) -> Response {
        //let mut font_icon: Option<LayoutJob> = None;
        //let mut radio = "A".to_string();
        //eframe::egui::ComboBox::from_label("Take your pick")
        //    .selected_text(format!("{radio}"))
        //    .show_ui(ui, |ui| {
        //        ui.selectable_value(&mut radio, "A".to_string(), "First");
        //        ui.selectable_value(&mut radio, "B".to_string(), "Second");
        //        ui.selectable_value(&mut radio, "C".to_string(), "Third");
        //    });

        //ui.scope(|ui| {
        //    ui.visuals_mut().widgets.inactive.expansion = -100.0;
        //    ui.add(eframe::egui::Slider::new(&mut 50, 0..=120).show_value(false));
        //});
        SelectableFrame::new(false).show(ui, |ui| {
            let widget = match self.show_icon(font_id.clone(), ctx, ui) {
                Some(mut layout_job) => {
                    layout_job.append(
                        &self.to_string(),
                        ctx.style().spacing.item_spacing.x,
                        TextFormat::simple(font_id.clone(), ctx.style().visuals.text_color()),
                    );

                    Label::new(layout_job)
                }
                None => Label::new(self.to_string()),
            };

            // TODO: only add the tooltip if the text is not added
            ui.add(widget.selectable(false))
                .on_hover_text(self.to_string())
            //    .hovered()
            //{
            //    eframe::egui::Frame::popup(&ui.style()).show(ui, |ui| {
            //        ui.label("This frame only appears on hover!");
            //    });
            //}
            //.on_hover_ui(|ui| {
            //    ui.add(Label::new("TEST".to_string()));
            //})
        })
    }
}
