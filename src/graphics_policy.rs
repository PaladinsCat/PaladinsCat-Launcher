//! Exact hardware policy for WatchCat graphics. Enumeration order is not a device identity.

pub const NVIDIA_VENDOR_ID: u32 = 0x10de;
pub const RTX_5080_DEVICE_ID: u32 = 0x2c02;

#[derive(Clone, Copy, Debug)]
pub struct GraphicsAdapterIdentity<'a> {
    pub name: &'a str,
    pub vendor: u32,
    pub device: u32,
    pub software: bool,
    pub pci_bus_id: &'a str,
}

pub fn is_rtx_5080(adapter: &GraphicsAdapterIdentity<'_>) -> bool {
    !adapter.software
        && adapter.vendor == NVIDIA_VENDOR_ID
        && adapter.device == RTX_5080_DEVICE_ID
        && adapter.name.contains("RTX 5080")
}

/// Refuse missing and ambiguous hardware instead of falling back to another adapter.
pub fn select_rtx_5080(adapters: &[GraphicsAdapterIdentity<'_>]) -> Result<usize, &'static str> {
    let mut matching = adapters.iter().enumerate().filter(|(_, a)| is_rtx_5080(a));
    let (index, first) = matching
        .next()
        .ok_or("Required NVIDIA RTX 5080 is unavailable")?;
    if first.pci_bus_id.is_empty() {
        return Err("RTX 5080 physical PCI identity is unavailable");
    }
    if matching.any(|(_, adapter)| adapter.pci_bus_id != first.pci_bus_id) {
        return Err("Multiple RTX 5080 adapters found; physical device identity is ambiguous");
    }
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter(name: &str, device: u32) -> GraphicsAdapterIdentity<'_> {
        GraphicsAdapterIdentity {
            name,
            vendor: NVIDIA_VENDOR_ID,
            device,
            software: false,
            pci_bus_id: "0000:01:00.0",
        }
    }

    #[test]
    fn selection_survives_adapter_reordering() {
        let target = adapter("NVIDIA GeForce RTX 5080", RTX_5080_DEVICE_ID);
        let other = adapter("NVIDIA GeForce RTX 5090", 0x2b85);
        assert_eq!(select_rtx_5080(&[other, target]), Ok(1));
        assert_eq!(select_rtx_5080(&[target, other]), Ok(0));
    }

    #[test]
    fn no_fallback_to_wrong_or_software_device() {
        assert!(select_rtx_5080(&[]).is_err());
        assert!(select_rtx_5080(&[adapter("NVIDIA GeForce RTX 5090", 0x2b85)]).is_err());
        let mut candidate = adapter("NVIDIA GeForce RTX 5080", RTX_5080_DEVICE_ID);
        candidate.software = true;
        assert!(select_rtx_5080(&[candidate]).is_err());
        candidate.software = false;
        candidate.vendor = 0x1002;
        assert!(select_rtx_5080(&[candidate]).is_err());
        candidate.vendor = NVIDIA_VENDOR_ID;
        candidate.device = 0x2b85;
        assert!(select_rtx_5080(&[candidate]).is_err());
        candidate.device = RTX_5080_DEVICE_ID;
        candidate.name = "NVIDIA GeForce RTX 5090";
        assert!(select_rtx_5080(&[candidate]).is_err());
    }

    #[test]
    fn driver_aliases_are_one_physical_device_but_distinct_devices_are_ambiguous() {
        let target = adapter("NVIDIA GeForce RTX 5080", RTX_5080_DEVICE_ID);
        assert_eq!(select_rtx_5080(&[target, target]), Ok(0));
        let mut second = target;
        second.pci_bus_id = "0000:04:00.0";
        assert!(select_rtx_5080(&[target, second]).is_err());
        second.pci_bus_id = "";
        assert!(select_rtx_5080(&[second]).is_err());
    }
}
