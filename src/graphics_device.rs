//! Select concrete hardware before allocation, then supply that device to eframe.
use eframe::{egui_wgpu, wgpu};
use paladinscat_launcher::graphics_policy::{
    self as policy, GraphicsAdapterIdentity, GraphicsPolicy, HardwareKind,
};

pub struct SelectedAdapter {
    info: wgpu::AdapterInfo,
    policy: GraphicsPolicy,
}

fn identity(info: &wgpu::AdapterInfo) -> GraphicsAdapterIdentity<'_> {
    GraphicsAdapterIdentity {
        name: &info.name,
        vendor: info.vendor,
        device: info.device,
        software: info.device_type == wgpu::DeviceType::Cpu,
        pci_bus_id: &info.device_pci_bus_id,
        kind: match info.device_type {
            wgpu::DeviceType::DiscreteGpu => HardwareKind::Discrete,
            wgpu::DeviceType::IntegratedGpu => HardwareKind::Integrated,
            wgpu::DeviceType::VirtualGpu => HardwareKind::Virtual,
            _ => HardwareKind::Other,
        },
    }
}

fn evidence(selected: &SelectedAdapter, stage: &str) -> Result<(), String> {
    let info = &selected.info;
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
        "selection_policy": selected.policy,
    });
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("Cannot save selected renderer identity: {e}"))
}

pub fn configuration() -> Result<(egui_wgpu::WgpuConfiguration, SelectedAdapter), String> {
    // Resolve the per-user policy before even enumerating hardware. Bad local
    // configuration must not silently disable a required developer GPU pin.
    let policy =
        policy::load_policy(&paladinscat_launcher::engine::data_dir().join(policy::POLICY_FILE))?;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let mut adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::DX12));
    let information: Vec<_> = adapters.iter().map(wgpu::Adapter::get_info).collect();
    let identities: Vec<_> = information.iter().map(identity).collect();
    let index = policy::select_adapter(policy, &identities)?;
    let selected = SelectedAdapter {
        info: information[index].clone(),
        policy,
    };
    evidence(&selected, "adapter_selected")?;
    let adapter = adapters.remove(index);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("PaladinsCat Launcher selected hardware"),
        ..Default::default()
    }))
    .map_err(|e| format!("Cannot create selected hardware device: {e}"))?;
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
    expected: &SelectedAdapter,
) -> Result<(), String> {
    let actual = cc
        .wgpu_render_state
        .as_ref()
        .ok_or("Expected manually selected wgpu renderer")?
        .adapter
        .get_info();
    if actual.backend != wgpu::Backend::Dx12 {
        return Err("Renderer did not retain the selected DirectX 12 backend".into());
    }
    policy::verify_selected_identity(&identity(&expected.info), &identity(&actual))?;
    evidence(expected, "renderer_initialized")
}
