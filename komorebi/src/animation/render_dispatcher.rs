use color_eyre::eyre;

pub trait RenderDispatcher {
    fn get_animation_key(&self) -> String;
    fn pre_render(&mut self) -> eyre::Result<()>;
    fn render(&mut self, delta: f64) -> eyre::Result<()>;
    fn post_render(&mut self) -> eyre::Result<()>;
    fn on_cancle(&mut self);
}
