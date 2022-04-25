use crate::widget::BarWidget;
use local_ip_address::find_ifa;
use local_ip_address::local_ip;

pub struct IpAddress {
    pub interface: String,
}

impl IpAddress {
    pub fn init(interface: String) -> Self {
        IpAddress { interface }
    }
}

impl BarWidget for IpAddress {
    fn output(&mut self) -> Vec<String> {
        if let Ok(interfaces) = local_ip_address::list_afinet_netifas() {
            if let Some((interface, ip_address)) =
                local_ip_address::find_ifa(interfaces, &self.interface)
            {
                return vec![format!("{}: {}", interface, ip_address)];
            }
        }

        vec![format!("{}: disconnected", self.interface)]
    }
}
