//! Explicit VM-only renderer contract. The ordinary host renderer is unchanged.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_PROOF_BYTES: u64 = 64 * 1024;
const MAX_PROOF_LIFETIME_MS: u64 = 300_000;
const CLOCK_SKEW_MS: u64 = 5_000;
const BOOT_TOLERANCE_MS: u64 = 10_000;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestIdentity {
    pub smbios_uuid: String,
    pub boot_time_unix_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HostProof {
    pub vendor: u32,
    pub device: u32,
    pub name: String,
    pub pci_bus_id: String,
    pub mks_device_index: u32,
    pub mks_renderer: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Proof {
    pub schema_version: u32,
    pub issued_at_ms: u64,
    pub expires_at_ms: u64,
    pub guest: GuestIdentity,
    pub host: HostProof,
}

#[derive(Clone, Debug, Serialize)]
pub struct VirtualAdapter {
    pub name: String,
    pub vendor: u32,
    pub device: u32,
    pub software: bool,
}

#[derive(Clone)]
pub struct VmRenderer {
    proof: Proof,
    proof_sha256: String,
    adapter: VirtualAdapter,
}

pub fn now_ms() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_millis() as u64)
        .map_err(|e| e.to_string())
}

fn uuid_key(value: &str) -> Result<String, String> {
    if value.len() != 36
        || value.as_bytes().iter().enumerate().any(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                *b != b'-'
            } else {
                !b.is_ascii_hexdigit()
            }
        })
    {
        return Err("VM proof requires a canonical SMBIOS UUID".into());
    }
    let value = value.to_ascii_lowercase();
    if value == "00000000-0000-0000-0000-000000000000"
        || value == "ffffffff-ffff-ffff-ffff-ffffffffffff"
    {
        return Err("SMBIOS UUID is unspecified".into());
    }
    Ok(value)
}

fn physical_pci(value: &str) -> bool {
    let parts: Vec<_> = value.split(':').collect();
    let (domain, bus, endpoint) = match parts.as_slice() {
        [domain, bus, endpoint] => (Some(*domain), *bus, *endpoint),
        [bus, endpoint] => (None, *bus, *endpoint),
        _ => return false,
    };
    let hex = |v: &str, len| v.len() == len && v.bytes().all(|b| b.is_ascii_hexdigit());
    let Some((device, function)) = endpoint.split_once('.') else {
        return false;
    };
    domain.is_none_or(|v| hex(v, 4))
        && hex(bus, 2)
        && hex(device, 2)
        && u8::from_str_radix(device, 16).is_ok_and(|v| v <= 31)
        && function.len() == 1
        && matches!(function.as_bytes()[0], b'0'..=b'7')
}

fn validate_proof(proof: &Proof, guest: &GuestIdentity, now: u64) -> Result<(), String> {
    let lifetime = proof
        .expires_at_ms
        .checked_sub(proof.issued_at_ms)
        .ok_or("VM proof expiry precedes issue time")?;
    if proof.schema_version != 1
        || lifetime == 0
        || lifetime >= MAX_PROOF_LIFETIME_MS
        || proof.issued_at_ms > now.saturating_add(CLOCK_SKEW_MS)
        || now >= proof.expires_at_ms
    {
        return Err(
            "VM renderer proof is expired, future-dated or outside its freshness contract".into(),
        );
    }
    if proof.guest.boot_time_unix_ms == 0
        || proof.guest.boot_time_unix_ms > proof.issued_at_ms
        || uuid_key(&proof.guest.smbios_uuid)? != uuid_key(&guest.smbios_uuid)?
        || proof
            .guest
            .boot_time_unix_ms
            .abs_diff(guest.boot_time_unix_ms)
            > BOOT_TOLERANCE_MS
    {
        return Err("VM renderer proof does not match this guest UUID and boot".into());
    }
    let host = &proof.host;
    if host.vendor != 0x10de
        || host.device != 0x2c02
        || host.name != "NVIDIA GeForce RTX 5080"
        || !physical_pci(&host.pci_bus_id)
        || host.mks_device_index != 2
        || !host.mks_renderer.eq_ignore_ascii_case("Vulkan")
    {
        return Err(
            "VM renderer proof does not identify the required physical RTX 5080 and MKS device 2"
                .into(),
        );
    }
    Ok(())
}

fn select_virtual(adapters: &[VirtualAdapter]) -> Result<VirtualAdapter, String> {
    let hardware: Vec<_> = adapters.iter().filter(|a| !a.software).collect();
    if hardware.len() != 1 || hardware[0].vendor != 0x15ad || hardware[0].device != 0x0405 {
        return Err(
            "VM mode requires exactly one VMware SVGA 3D hardware adapter; no software fallback"
                .into(),
        );
    }
    Ok(hardware[0].clone())
}

fn verify_gl(vendor: &str, renderer: &str) -> Result<(), String> {
    let vendor = vendor.trim().to_ascii_lowercase().replace(' ', "");
    let renderer = renderer.trim().to_ascii_lowercase();
    if !["vmware,inc.", "vmware,inc", "vmware"].contains(&vendor.as_str())
        || !renderer.starts_with("svga3d")
        || [
            "llvmpipe",
            "softpipe",
            "software",
            "gdi generic",
            "basic render",
        ]
        .iter()
        .any(|s| renderer.contains(s))
    {
        return Err("OpenGL context is not the verified VMware SVGA3D hardware renderer".into());
    }
    Ok(())
}

impl VmRenderer {
    pub fn preflight(path: &Path) -> Result<Self, String> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|e| format!("Open VM renderer proof: {e}"))?
            .take(MAX_PROOF_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_PROOF_BYTES {
            return Err("VM renderer proof exceeds size limit".into());
        }
        let proof: Proof = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Invalid VM renderer proof: {e}"))?;
        validate_proof(&proof, &native_guest()?, now_ms()?)?;
        let adapter = select_virtual(&native_adapters()?)?;
        Ok(Self {
            proof,
            proof_sha256: format!("{:x}", Sha256::digest(&bytes)),
            adapter,
        })
    }

    pub fn revalidate(&self) -> Result<(), String> {
        validate_proof(&self.proof, &native_guest()?, now_ms()?)
    }

    pub fn verify_context(&self, cc: &eframe::CreationContext<'_>) -> Result<(), String> {
        use eframe::glow::HasContext;
        self.revalidate()?;
        let gl = cc
            .gl
            .as_ref()
            .ok_or("Expected explicit VM OpenGL context")?;
        let vendor = unsafe { gl.get_parameter_string(eframe::glow::VENDOR) };
        let renderer = unsafe { gl.get_parameter_string(eframe::glow::RENDERER) };
        verify_gl(&vendor, &renderer)?;
        let record = serde_json::json!({
            "schema_version":2,"process_id":std::process::id(),"tool_version":env!("CARGO_PKG_VERSION"),
            "stage":"renderer_initialized","mode":"vm_host_proof","renderer":"eframe-glow-vmware-svga3d",
            "virtual_adapter":self.adapter,"gl_vendor":vendor,"gl_renderer":renderer,
            "host_physical_proof":self.proof,"proof_sha256":self.proof_sha256,
            "observed_guest":native_guest()?,"verified_at_ms":now_ms()?
        });
        let dir = paladinscat_launcher::engine::data_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        paladinscat_launcher::engine::save_settings(&dir.join("graphics-device.json"), &record)
    }
}

fn smbios_uuid(raw: &[u8]) -> Result<String, String> {
    if raw.len() < 8 {
        return Err("Truncated SMBIOS data".into());
    }
    let len = u32::from_le_bytes(raw[4..8].try_into().unwrap()) as usize;
    let end = 8usize
        .checked_add(len)
        .filter(|v| *v <= raw.len())
        .ok_or("Invalid SMBIOS length")?;
    let mut at = 8;
    let mut found = None;
    while at < end {
        if end - at < 4 {
            return Err("Truncated SMBIOS record".into());
        }
        let kind = raw[at];
        let size = raw[at + 1] as usize;
        if size < 4 || size > end - at {
            return Err("Invalid SMBIOS record size".into());
        }
        if kind == 1 {
            if size < 24 || found.is_some() {
                return Err("Missing or ambiguous SMBIOS system UUID".into());
            }
            let bytes = &raw[at + 8..at + 24];
            let order = if raw[1] > 2 || (raw[1] == 2 && raw[2] >= 6) {
                [3, 2, 1, 0, 5, 4, 7, 6, 8, 9, 10, 11, 12, 13, 14, 15]
            } else {
                [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
            };
            let mut uuid = String::new();
            for (i, offset) in order.into_iter().enumerate() {
                if [4, 6, 8, 10].contains(&i) {
                    uuid.push('-');
                }
                uuid.push_str(&format!("{:02x}", bytes[offset]));
            }
            found = Some(uuid_key(&uuid)?);
        }
        at += size;
        while at + 1 < end && raw[at..at + 2] != [0, 0] {
            at += 1;
        }
        if at + 1 >= end {
            return Err("Unterminated SMBIOS strings".into());
        }
        at += 2;
        if kind == 127 {
            break;
        }
    }
    found.ok_or_else(|| "SMBIOS system UUID unavailable".into())
}

#[cfg(windows)]
fn native_guest() -> Result<GuestIdentity, String> {
    use windows_sys::Win32::System::SystemInformation::{GetSystemFirmwareTable, GetTickCount64};
    let provider = u32::from_be_bytes(*b"RSMB");
    let size = unsafe { GetSystemFirmwareTable(provider, 0, std::ptr::null_mut(), 0) };
    if !(8..=1024 * 1024).contains(&size) {
        return Err("SMBIOS firmware table unavailable or oversized".into());
    }
    let mut raw = vec![0u8; size as usize];
    let actual = unsafe { GetSystemFirmwareTable(provider, 0, raw.as_mut_ptr().cast(), size) };
    if actual != size {
        return Err("SMBIOS table changed during native read".into());
    }
    Ok(GuestIdentity {
        smbios_uuid: smbios_uuid(&raw)?,
        boot_time_unix_ms: now_ms()?
            .checked_sub(unsafe { GetTickCount64() })
            .ok_or("Invalid native guest boot time")?,
    })
}

#[cfg(windows)]
fn native_adapters() -> Result<Vec<VirtualAdapter>, String> {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
    };
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }.map_err(|e| e.to_string())?;
    let mut adapters = Vec::new();
    for index in 0..64 {
        let adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(adapter) => adapter,
            Err(error) if error.code().0 as u32 == 0x887a0002 => return Ok(adapters),
            Err(error) => return Err(format!("Enumerate VM hardware adapters: {error}")),
        };
        let desc = unsafe { adapter.GetDesc1() }.map_err(|e| e.to_string())?;
        let length = desc
            .Description
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(desc.Description.len());
        adapters.push(VirtualAdapter {
            name: String::from_utf16(&desc.Description[..length]).map_err(|e| e.to_string())?,
            vendor: desc.VendorId,
            device: desc.DeviceId,
            software: desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0,
        });
    }
    Err("VM hardware inventory exceeded its limit".into())
}

#[cfg(not(windows))]
fn native_guest() -> Result<GuestIdentity, String> {
    Err("VM renderer mode requires Windows".into())
}
#[cfg(not(windows))]
fn native_adapters() -> Result<Vec<VirtualAdapter>, String> {
    Err("VM renderer mode requires Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Proof, GuestIdentity) {
        let guest = GuestIdentity {
            smbios_uuid: "12345678-1234-5678-9012-345678901234".into(),
            boot_time_unix_ms: 100_000,
        };
        (
            Proof {
                schema_version: 1,
                issued_at_ms: 200_000,
                expires_at_ms: 400_000,
                guest: guest.clone(),
                host: HostProof {
                    vendor: 0x10de,
                    device: 0x2c02,
                    name: "NVIDIA GeForce RTX 5080".into(),
                    pci_bus_id: "0000:03:00.0".into(),
                    mks_device_index: 2,
                    mks_renderer: "Vulkan".into(),
                },
            },
            guest,
        )
    }
    #[test]
    fn proof_rejects_other_gpu_missing_pci_wrong_mks_and_stale_guest() {
        let (valid, guest) = fixture();
        assert!(validate_proof(&valid, &guest, 210_000).is_ok());
        for mutate in [
            |p: &mut Proof| p.host.device = 0x2b85,
            |p: &mut Proof| p.host.vendor = 0x1414,
            |p: &mut Proof| p.host.pci_bus_id = "software".into(),
            |p: &mut Proof| p.host.mks_device_index = 1,
            |p: &mut Proof| p.guest.smbios_uuid = "87654321-1234-5678-9012-345678901234".into(),
            |p: &mut Proof| p.guest.boot_time_unix_ms = 80_000,
        ] {
            let mut proof = valid.clone();
            mutate(&mut proof);
            assert!(validate_proof(&proof, &guest, 210_000).is_err());
        }
    }
    #[test]
    fn proof_rejects_expired_future_or_five_minute_lifetime() {
        let (mut proof, guest) = fixture();
        assert!(validate_proof(&proof, &guest, 400_000).is_err());
        assert!(validate_proof(&proof, &guest, 190_000).is_err());
        proof.expires_at_ms = 500_000;
        assert!(validate_proof(&proof, &guest, 210_000).is_err());
    }
    #[test]
    fn virtual_inventory_and_gl_context_cannot_choose_software_or_ambiguous_hardware() {
        let vm = VirtualAdapter {
            name: "VMware SVGA 3D".into(),
            vendor: 0x15ad,
            device: 0x0405,
            software: false,
        };
        let software = VirtualAdapter {
            name: "Microsoft Basic Render Driver".into(),
            vendor: 0x1414,
            device: 0x8c,
            software: true,
        };
        assert!(select_virtual(&[vm.clone(), software.clone()]).is_ok());
        assert!(select_virtual(&[software]).is_err());
        assert!(select_virtual(&[vm.clone(), vm.clone()]).is_err());
        let mut fake = vm;
        fake.software = true;
        assert!(select_virtual(&[fake]).is_err());
        assert!(verify_gl("VMware, Inc.", "SVGA3D; build: RELEASE; LLVM;").is_ok());
        assert!(verify_gl("Microsoft Corporation", "GDI Generic").is_err());
        assert!(verify_gl("VMware, Inc.", "SVGA3D llvmpipe").is_err());
    }
    #[test]
    fn native_smbios_uuid_parser_handles_windows_endian_and_rejects_truncation() {
        let mut raw = vec![0, 3, 0, 0];
        raw.extend_from_slice(&26u32.to_le_bytes());
        raw.extend_from_slice(&[1, 24, 0, 0, 0, 0, 0, 0]);
        raw.extend_from_slice(&[
            0x78, 0x56, 0x34, 0x12, 0x34, 0x12, 0x78, 0x56, 0x90, 0x12, 0x34, 0x56, 0x78, 0x90,
            0x12, 0x34,
        ]);
        raw.extend_from_slice(&[0, 0]);
        assert_eq!(
            smbios_uuid(&raw).unwrap(),
            "12345678-1234-5678-9012-345678901234"
        );
        assert!(smbios_uuid(&raw[..raw.len() - 1]).is_err());
    }
}
