use as_any::AsAny;
use color_eyre::Result;

#[derive(Debug, Clone)]
pub enum Output {
    SingleBox(String),
    MultiBox(Vec<String>),
}

#[derive(Debug, Copy, Clone)]
pub enum RepaintStrategy {
    Default,
    Constant,
}

pub trait Widget: AsAny {
    fn output(&mut self) -> Result<Output>;
    fn repaint_strategy(&self) -> RepaintStrategy {
        RepaintStrategy::Default
    }
}

pub trait BarWidget {
    fn output(&mut self) -> Vec<String>;
}
