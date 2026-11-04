//! Isolated DX12/FXC compatibility experiment; guest acceptance is still required.

use eframe::egui_wgpu::WgpuSetupExisting;
use thiserror::Error;
use wgpu::{Backends, DeviceType, Dx12Compiler};

#[derive(Debug, Error)]
pub enum GraphicsError {
    #[error("could not initialize the graphics executor: {0}")]
    Executor(#[source] std::io::Error),
    #[error("no usable Direct3D 12 device (hardware/WARP): {attempts}")]
    Unavailable { attempts: String },
}

pub fn prepare(force_software: bool) -> Result<WgpuSetupExisting, GraphicsError> {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(GraphicsError::Executor)?
        .block_on(prepare_async(force_software))
}

async fn prepare_async(force_software: bool) -> Result<WgpuSetupExisting, GraphicsError> {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = Backends::DX12;
    descriptor.backend_options.dx12.shader_compiler = Dx12Compiler::Fxc;
    let instance = wgpu::Instance::new(descriptor);
    let adapters = instance.enumerate_adapters(Backends::DX12).await;
    let device_types: Vec<_> = adapters.iter().map(|a| a.get_info().device_type).collect();
    let mut attempts = Vec::new();
    for index in adapter_order(&device_types, force_software) {
        let adapter = &adapters[index];
        let info = adapter.get_info();
        eprintln!("Trying DX12/FXC: {} ({:?})", info.name, info.device_type);
        let device_descriptor = wgpu::DeviceDescriptor {
            label: Some("Browser Harbor compatibility device"),
            required_limits: wgpu::Limits {
                max_texture_dimension_2d: 8192,
                ..Default::default()
            },
            ..Default::default()
        };
        match adapter.request_device(&device_descriptor).await {
            Ok((device, queue)) => {
                eprintln!("Selected DX12/FXC: {} ({:?})", info.name, info.device_type);
                // shortcut: surface/presentation failures are reported by eframe; retrying them needs guest evidence.
                return Ok(WgpuSetupExisting {
                    instance,
                    adapter: adapter.clone(),
                    device,
                    queue,
                });
            }
            Err(error) => {
                eprintln!("DX12 device failed: {}: {error}", info.name);
                attempts.push(format!("{}: {error}", info.name));
            }
        }
    }
    Err(GraphicsError::Unavailable {
        attempts: if attempts.is_empty() {
            if force_software {
                "no DX12 CPU adapter was found (--warp)".to_owned()
            } else {
                "no DX12 adapter was found".to_owned()
            }
        } else {
            attempts.join("; ")
        },
    })
}

fn adapter_order(device_types: &[DeviceType], force_software: bool) -> Vec<usize> {
    let mut order: Vec<_> = (0..device_types.len())
        .filter(|&i| !force_software || device_types[i] == DeviceType::Cpu)
        .collect();
    order.sort_by_key(|&i| device_types[i] == DeviceType::Cpu);
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_is_tried_before_cpu_regardless_of_enumeration_order() {
        assert_eq!(
            adapter_order(
                &[
                    DeviceType::Cpu,
                    DeviceType::IntegratedGpu,
                    DeviceType::DiscreteGpu
                ],
                false
            ),
            vec![1, 2, 0]
        );
    }

    #[test]
    fn warp_mode_never_selects_a_hardware_device() {
        assert_eq!(
            adapter_order(
                &[
                    DeviceType::DiscreteGpu,
                    DeviceType::Cpu,
                    DeviceType::VirtualGpu
                ],
                true
            ),
            vec![1]
        );
        assert!(adapter_order(&[DeviceType::IntegratedGpu], true).is_empty());
        assert!(adapter_order(&[], false).is_empty());
    }

    #[test]
    #[ignore = "requires a real DX12 hardware device or WARP; explicitly run on the test machine"]
    fn dx12_can_create_egui_shader_pipeline() {
        check_pipeline(false);
    }

    #[test]
    #[ignore = "requires a real DX12 CPU/WARP device; explicitly run on the test machine"]
    fn warp_can_create_egui_shader_pipeline() {
        check_pipeline(true);
    }

    fn check_pipeline(force_software: bool) {
        let setup = prepare(force_software).expect("DX12 device");
        if force_software {
            assert_eq!(setup.adapter.get_info().device_type, DeviceType::Cpu);
        }
        let scope = setup.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let _renderer = eframe::egui_wgpu::Renderer::new(
            &setup.device,
            wgpu::TextureFormat::Bgra8Unorm,
            Default::default(),
        );
        let error = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(scope.pop());
        assert!(error.is_none(), "egui pipeline validation: {error:?}");
    }
}
