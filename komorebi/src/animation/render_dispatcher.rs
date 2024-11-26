use color_eyre::Result;

pub trait RenderDispatcher {
    fn get_animation_key(&self) -> String;
    fn pre_render(&self) -> Result<()>;
    fn render(&self, delta: f64) -> Result<()>;
    fn post_render(&self) -> Result<()>;
}
