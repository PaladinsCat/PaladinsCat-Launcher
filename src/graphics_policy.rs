//! Public hardware selection and an explicit per-user developer GPU pin.
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{ErrorKind, Read},
    path::Path,
};

pub const NVIDIA_VENDOR_ID: u32 = 0x10de;
pub const RTX_5080_DEVICE_ID: u32 = 0x2c02;
pub const POLICY_FILE: &str = "graphics-policy.json";
const MAX_POLICY_BYTES: u64 = 4096;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphicsPolicy {
    #[default]
    PublicHardware,
    #[serde(rename = "rtx_5080")]
    Rtx5080,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PolicyConfig {
    schema_version: u32,
    selection: GraphicsPolicy,
}

/// Missing configuration is the public default. Invalid configuration never
/// turns an explicit developer pin into an unpinned hardware selection.
pub fn load_policy(path: &Path) -> Result<GraphicsPolicy, String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(GraphicsPolicy::default());
        }
        Err(error) => return Err(format!("Read graphics policy: {error}")),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("Graphics policy must be an ordinary file".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("Graphics policy cannot be a reparse point".into());
        }
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| format!("Open graphics policy: {error}"))?
        .take(MAX_POLICY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Read graphics policy: {error}"))?;
    if bytes.len() as u64 > MAX_POLICY_BYTES {
        return Err("Graphics policy exceeds its size limit".into());
    }
    let config: PolicyConfig = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Invalid graphics policy: {error}"))?;
    if config.schema_version != 1 {
        return Err("Unsupported graphics policy schema".into());
    }
    Ok(config.selection)
}

/// Explicit CPU-only configuration action. Adapter selection never writes a
/// developer pin or changes the user's configuration.
pub fn write_rtx_5080_policy(path: &Path) -> Result<(), String> {
    crate::engine::save_settings(
        path,
        &PolicyConfig {
            schema_version: 1,
            selection: GraphicsPolicy::Rtx5080,
        },
    )?;
    if load_policy(path)? != GraphicsPolicy::Rtx5080 {
        return Err("Written graphics policy did not retain the RTX 5080 pin".into());
    }
    Ok(())
}

/// Stable hardware preference, independent of driver enumeration order.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HardwareKind {
    Discrete,
    Integrated,
    Virtual,
    Other,
}

#[derive(Clone, Copy, Debug)]
pub struct GraphicsAdapterIdentity<'a> {
    pub name: &'a str,
    pub vendor: u32,
    pub device: u32,
    pub software: bool,
    pub pci_bus_id: &'a str,
    pub kind: HardwareKind,
}

fn hardware(adapter: &GraphicsAdapterIdentity<'_>) -> bool {
    let name = adapter.name.to_ascii_lowercase();
    !adapter.software
        && adapter.vendor != 0
        && adapter.device != 0
        && !name.trim().is_empty()
        && ![
            "software",
            "llvmpipe",
            "softpipe",
            "swiftshader",
            "basic render",
            "warp",
        ]
        .iter()
        .any(|marker| name.contains(marker))
}

pub fn is_rtx_5080(adapter: &GraphicsAdapterIdentity<'_>) -> bool {
    hardware(adapter)
        && adapter.vendor == NVIDIA_VENDOR_ID
        && adapter.device == RTX_5080_DEVICE_ID
        && adapter.name.contains("RTX 5080")
}

/// Refuse missing and ambiguous pinned hardware without any public fallback.
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

pub fn select_adapter(
    policy: GraphicsPolicy,
    adapters: &[GraphicsAdapterIdentity<'_>],
) -> Result<usize, &'static str> {
    if policy == GraphicsPolicy::Rtx5080 {
        return select_rtx_5080(adapters);
    }
    adapters
        .iter()
        .enumerate()
        .filter(|(_, adapter)| hardware(adapter))
        .min_by_key(|(_, adapter)| {
            (
                adapter.kind,
                adapter.vendor,
                adapter.device,
                adapter.name,
                adapter.pci_bus_id,
            )
        })
        .map(|(index, _)| index)
        .ok_or("No supported Windows DirectX 12 hardware adapter is available")
}

/// Verify actual device identity before reporting renderer initialization.
pub fn verify_selected_identity(
    expected: &GraphicsAdapterIdentity<'_>,
    actual: &GraphicsAdapterIdentity<'_>,
) -> Result<(), &'static str> {
    if !hardware(actual)
        || actual.vendor != expected.vendor
        || actual.device != expected.device
        || actual.name != expected.name
        || actual.pci_bus_id != expected.pci_bus_id
        || actual.kind != expected.kind
    {
        return Err("Renderer did not retain the selected hardware identity");
    }
    Ok(())
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
            kind: HardwareKind::Discrete,
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

    #[test]
    fn public_default_accepts_other_vendors_and_prefers_discrete_hardware() {
        let mut amd = adapter("AMD Radeon RX 7900 XT", 0x744c);
        amd.vendor = 0x1002;
        let mut intel = adapter("Intel Iris Xe Graphics", 0x9a49);
        intel.vendor = 0x8086;
        intel.kind = HardwareKind::Integrated;
        assert_eq!(
            select_adapter(GraphicsPolicy::PublicHardware, &[intel, amd]),
            Ok(1)
        );
        assert_eq!(
            select_adapter(GraphicsPolicy::PublicHardware, &[amd, intel]),
            Ok(0)
        );
        // PCI identity is not universally supplied by consumer drivers.
        intel.pci_bus_id = "";
        assert_eq!(
            select_adapter(GraphicsPolicy::PublicHardware, &[intel]),
            Ok(0)
        );
        assert!(select_adapter(GraphicsPolicy::Rtx5080, &[amd, intel]).is_err());
    }

    #[test]
    fn public_tie_breaking_is_independent_of_enumeration_order() {
        let first = adapter("NVIDIA GeForce RTX 4070", 0x2786);
        let mut second = first;
        second.pci_bus_id = "0000:04:00.0";
        assert_eq!(
            select_adapter(GraphicsPolicy::PublicHardware, &[first, second]),
            Ok(0)
        );
        assert_eq!(
            select_adapter(GraphicsPolicy::PublicHardware, &[second, first]),
            Ok(1)
        );
    }

    #[test]
    fn public_policy_never_selects_cpu_or_named_software_renderers() {
        let mut software = adapter("Microsoft Basic Render Driver", 0x008c);
        software.vendor = 0x1414;
        assert!(select_adapter(GraphicsPolicy::PublicHardware, &[software]).is_err());
        software.name = "NVIDIA GeForce RTX 5080";
        software.software = true;
        assert!(select_adapter(GraphicsPolicy::PublicHardware, &[software]).is_err());
        assert!(select_adapter(GraphicsPolicy::PublicHardware, &[]).is_err());
        let hardware = adapter("NVIDIA GeForce RTX 4070", 0x2786);
        assert_eq!(
            select_adapter(GraphicsPolicy::PublicHardware, &[software, hardware]),
            Ok(1)
        );
    }

    #[test]
    fn renderer_evidence_refuses_changed_or_software_identity() {
        let selected = adapter("NVIDIA GeForce RTX 5080", RTX_5080_DEVICE_ID);
        assert_eq!(verify_selected_identity(&selected, &selected), Ok(()));
        for field in 0..6 {
            let mut actual = selected;
            match field {
                0 => actual.name = "NVIDIA GeForce RTX 5090",
                1 => actual.vendor = 0x1002,
                2 => actual.device = 0x2b85,
                3 => actual.pci_bus_id = "0000:04:00.0",
                4 => actual.software = true,
                _ => actual.kind = HardwareKind::Virtual,
            }
            assert!(verify_selected_identity(&selected, &actual).is_err());
        }
    }

    #[test]
    fn config_defaults_public_and_explicit_local_pin_round_trips() {
        let path = std::env::temp_dir().join(format!(
            "paladinscat-graphics-policy-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert_eq!(load_policy(&path).unwrap(), GraphicsPolicy::PublicHardware);
        write_rtx_5080_policy(&path).unwrap();
        assert_eq!(load_policy(&path).unwrap(), GraphicsPolicy::Rtx5080);
        for bytes in [
            r#"{"schema_version":2,"selection":"rtx_5080"}"#,
            r#"{"schema_version":1,"selection":"unknown"}"#,
            r#"{"schema_version":1,"selection":"rtx_5080","fallback":true}"#,
            r#"{"schema_version":1}"#,
            "invalid JSON",
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert!(load_policy(&path).is_err());
        }
        std::fs::write(&path, vec![b' '; MAX_POLICY_BYTES as usize + 1]).unwrap();
        assert!(load_policy(&path).is_err());
        std::fs::write(
            &path,
            r#"{"schema_version":1,"selection":"public_hardware"}"#,
        )
        .unwrap();
        assert_eq!(load_policy(&path).unwrap(), GraphicsPolicy::PublicHardware);
    }
}
