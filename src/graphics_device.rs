//! Select the required hardware before allocation, then supply it to eframe.
use eframe::{egui_wgpu, wgpu};
#[path = "graphics_policy.rs"]
mod policy;
use policy::{GraphicsAdapterIdentity, select_rtx_5080};

fn evidence(info: &wgpu::AdapterInfo, stage: &str) -> Result<(), String> {
    let path = paladinscat_launcher::engine::data_dir().join("graphics-device.json");
    std::fs::create_dir_all(path.parent().ok_or("GPU evidence directory unavailable")?)
        .map_err(|e| e.to_string())?;
    let record = serde_json::json!({
        "schema_version": 1,
        "process_id": std::process::id(),
        "tool_version": env!("CARGO_PKG_VERSION"),
        "stage": stage,
        "name": info.name,
        "vendor": info.vendor,
        "device": info.device,
        "pci_bus_id": info.device_pci_bus_id,
        "backend": format!("{:?}", info.backend),
        "device_type": format!("{:?}", info.device_type),
        "renderer": "eframe-wgpu-existing-device",
    });
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("Cannot save selected renderer identity: {e}"))
}

pub fn configuration() -> Result<(egui_wgpu::WgpuConfiguration, wgpu::AdapterInfo), String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let mut adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::DX12));
    let information: Vec<_> = adapters.iter().map(wgpu::Adapter::get_info).collect();
    let identities: Vec<_> = information
        .iter()
        .map(|info| GraphicsAdapterIdentity {
            name: &info.name,
            vendor: info.vendor,
            device: info.device,
            software: info.device_type == wgpu::DeviceType::Cpu,
            pci_bus_id: &info.device_pci_bus_id,
        })
        .collect();
    let index = select_rtx_5080(&identities)?;
    let selected = information[index].clone();
    evidence(&selected, "adapter_selected")?;
    let adapter = adapters.remove(index);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("PaladinsCat Launcher RTX 5080"),
        ..Default::default()
    }))
    .map_err(|e| format!("Cannot create selected RTX 5080 device: {e}"))?;
    Ok((
        egui_wgpu::WgpuConfiguration {
            wgpu_setup: egui_wgpu::WgpuSetup::Existing(egui_wgpu::WgpuSetupExisting {
                instance,
                adapter,
                device,
                queue,
            }),
            ..Default::default()
        },
        selected,
    ))
}

pub fn verify_renderer(
    cc: &eframe::CreationContext<'_>,
    expected: &wgpu::AdapterInfo,
) -> Result<(), String> {
    let actual = cc
        .wgpu_render_state
        .as_ref()
        .ok_or("Expected manually selected wgpu renderer")?
        .adapter
        .get_info();
    if actual.vendor != expected.vendor
        || actual.device != expected.device
        || actual.name != expected.name
        || actual.device_pci_bus_id != expected.device_pci_bus_id
        || actual.backend != wgpu::Backend::Dx12
        || actual.device_type == wgpu::DeviceType::Cpu
    {
        return Err("Renderer did not retain the selected RTX 5080 identity".into());
    }
    evidence(&actual, "renderer_initialized")
}
