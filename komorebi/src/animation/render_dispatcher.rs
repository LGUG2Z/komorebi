use color_eyre::eyre;

pub trait RenderDispatcher {
    fn get_animation_key(&self) -> String;
    fn pre_render(&self) -> eyre::Result<()>;
    fn render(&self, delta: f64) -> eyre::Result<()>;
    fn post_render(&self) -> eyre::Result<()>;
}
