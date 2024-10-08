use crate::battery::Battery;
use crate::battery::BatteryConfig;
use crate::cpu::Cpu;
use crate::cpu::CpuConfig;
use crate::date::Date;
use crate::date::DateConfig;
use crate::komorebi::Komorebi;
use crate::komorebi::KomorebiConfig;
use crate::media::Media;
use crate::media::MediaConfig;
use crate::memory::Memory;
use crate::memory::MemoryConfig;
use crate::network::Network;
use crate::network::NetworkConfig;
use crate::storage::Storage;
use crate::storage::StorageConfig;
use crate::time::Time;
use crate::time::TimeConfig;
use eframe::egui::Context;
use eframe::egui::Ui;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

pub trait BarWidget {
    fn render(&mut self, ctx: &Context, ui: &mut Ui);
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub enum WidgetConfig {
    Battery(BatteryConfig),
    Cpu(CpuConfig),
    Date(DateConfig),
    Komorebi(KomorebiConfig),
    Media(MediaConfig),
    Memory(MemoryConfig),
    Network(NetworkConfig),
    Storage(StorageConfig),
    Time(TimeConfig),
}

impl WidgetConfig {
    pub fn as_boxed_bar_widget(&self) -> Box<dyn BarWidget> {
        match self {
            WidgetConfig::Battery(config) => Box::new(Battery::from(*config)),
            WidgetConfig::Cpu(config) => Box::new(Cpu::from(*config)),
            WidgetConfig::Date(config) => Box::new(Date::from(config.clone())),
            WidgetConfig::Komorebi(config) => Box::new(Komorebi::from(config)),
            WidgetConfig::Media(config) => Box::new(Media::from(*config)),
            WidgetConfig::Memory(config) => Box::new(Memory::from(*config)),
            WidgetConfig::Network(config) => Box::new(Network::from(*config)),
            WidgetConfig::Storage(config) => Box::new(Storage::from(*config)),
            WidgetConfig::Time(config) => Box::new(Time::from(config.clone())),
        }
    }
}
