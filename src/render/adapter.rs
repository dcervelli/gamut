//! Which GPU draws the window.
//!
//! A machine with two GPUs has, as a rule, one of them wired to the screens
//! and the compositor running on it. A window drawn on the other has to be
//! copied across to be shown, and where that copy is not supported — a
//! desktop whose monitor is on an NVIDIA card beside an idle integrated GPU,
//! say — it is shown black. Neither power preference is right everywhere:
//! asking for the low-power adapter picks that idle integrated GPU, and asking
//! for the high-performance one would wake a laptop's discrete GPU for a
//! window its integrated one is already showing. So on Linux the adapter is
//! the one whose card has a monitor connected, read from the kernel's DRM
//! directory, and the power preference only breaks a tie between two such
//! cards.
//!
//! `WGPU_ADAPTER_NAME` (a case-insensitive part of the adapter's name) and
//! `WGPU_POWER_PREF` (`low`, `high` or `none`) override the choice.

use std::path::Path;

use anyhow::{Context, Result};

/// A GPU the kernel has a monitor connected to: where it sits on the PCI bus,
/// and its vendor and device IDs, which an adapter that does not say where it
/// sits is matched by instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Card {
    pub bus: Option<String>,
    pub vendor: u32,
    pub device: u32,
}

/// The adapter to draw `surface` with.
pub(crate) fn choose(instance: &wgpu::Instance, surface: &wgpu::Surface) -> Result<wgpu::Adapter> {
    if let Ok(name) = std::env::var("WGPU_ADAPTER_NAME") {
        let name = name.to_lowercase();
        return supporting(instance, surface)
            .into_iter()
            .find(|adapter| adapter.get_info().name.to_lowercase().contains(&name))
            .with_context(|| format!("no GPU adapter named like WGPU_ADAPTER_NAME={name}"));
    }
    if let Some(preference) = wgpu::PowerPreference::from_env() {
        return request(instance, surface, preference);
    }
    // Only Linux has the kernel's DRM directory; elsewhere the power
    // preference alone chooses.
    let cards = if cfg!(target_os = "linux") {
        displaying(Path::new("/sys/class/drm"))
    } else {
        Vec::new()
    };
    if !cards.is_empty() {
        let mut adapters = supporting(instance, surface);
        let infos: Vec<_> = adapters.iter().map(wgpu::Adapter::get_info).collect();
        if let Some(index) = pick(&infos, &cards) {
            return Ok(adapters.swap_remove(index));
        }
    }
    request(instance, surface, wgpu::PowerPreference::LowPower)
}

fn request(
    instance: &wgpu::Instance,
    surface: &wgpu::Surface,
    power_preference: wgpu::PowerPreference,
) -> Result<wgpu::Adapter> {
    pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference,
        compatible_surface: Some(surface),
        ..Default::default()
    }))
    .context("no suitable GPU adapter found")
}

fn supporting(instance: &wgpu::Instance, surface: &wgpu::Surface) -> Vec<wgpu::Adapter> {
    pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()))
        .into_iter()
        .filter(|adapter| adapter.is_surface_supported(surface))
        .collect()
}

/// Which of `adapters` to draw with, given the `cards` driving a monitor: one
/// on such a card, the integrated GPU first where there are several and
/// Vulkan before any other backend for the same GPU. `None` where no adapter
/// is on such a card.
pub(crate) fn pick(adapters: &[wgpu::AdapterInfo], cards: &[Card]) -> Option<usize> {
    adapters
        .iter()
        .enumerate()
        .filter(|(_, info)| cards.iter().any(|card| on(info, card)))
        .min_by_key(|(_, info)| {
            let kind = match info.device_type {
                wgpu::DeviceType::IntegratedGpu => 0,
                wgpu::DeviceType::DiscreteGpu => 1,
                wgpu::DeviceType::VirtualGpu => 2,
                wgpu::DeviceType::Other => 3,
                wgpu::DeviceType::Cpu => 4,
            };
            (kind, info.backend != wgpu::Backend::Vulkan)
        })
        .map(|(index, _)| index)
}

/// Whether `info` is the GPU `card` is: by bus address where both say it,
/// otherwise by vendor and device.
fn on(info: &wgpu::AdapterInfo, card: &Card) -> bool {
    match (&card.bus, info.device_pci_bus_id.is_empty()) {
        (Some(bus), false) => bus.eq_ignore_ascii_case(&info.device_pci_bus_id),
        _ => info.vendor != 0 && info.vendor == card.vendor && info.device == card.device,
    }
}

/// The cards under `root` — the kernel's `/sys/class/drm` — with a monitor
/// connected to one of their outputs. Empty where the directory cannot be
/// read, which leaves the choice to the power preference.
pub(crate) fn displaying(root: &Path) -> Vec<Card> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    let mut cards = Vec::new();
    for card in names.iter().filter(|name| is_card(name)) {
        let prefix = format!("{card}-");
        let connected = names.iter().any(|output| {
            output.starts_with(&prefix)
                && std::fs::read_to_string(root.join(output).join("status"))
                    .is_ok_and(|status| status.trim() == "connected")
        });
        if !connected {
            continue;
        }
        let device = root.join(card).join("device");
        let bus = std::fs::canonicalize(&device)
            .ok()
            .and_then(|path| path.file_name()?.to_str().map(str::to_owned))
            .filter(|name| name.contains(':'));
        cards.push(Card {
            bus,
            vendor: hex(&device.join("vendor")),
            device: hex(&device.join("device")),
        });
    }
    cards
}

/// `card0`, `card1`, …, and not one of their outputs (`card1-HDMI-A-1`).
fn is_card(name: &str) -> bool {
    name.strip_prefix("card")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// A sysfs ID file, `0x10de`; zero where it is missing or unreadable.
fn hex(path: &Path) -> u32 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| u32::from_str_radix(text.trim().trim_start_matches("0x"), 16).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(
        name: &str,
        device_type: wgpu::DeviceType,
        backend: wgpu::Backend,
        bus: &str,
        vendor: u32,
        device: u32,
    ) -> wgpu::AdapterInfo {
        wgpu::AdapterInfo {
            name: name.into(),
            vendor,
            device,
            device_pci_bus_id: bus.into(),
            ..wgpu::AdapterInfo::new(device_type, backend)
        }
    }

    fn radeon() -> wgpu::AdapterInfo {
        let (integrated, vulkan) = (wgpu::DeviceType::IntegratedGpu, wgpu::Backend::Vulkan);
        info("Radeon", integrated, vulkan, "0000:74:00.0", 0x1002, 0x13c0)
    }

    fn geforce() -> wgpu::AdapterInfo {
        let (discrete, vulkan) = (wgpu::DeviceType::DiscreteGpu, wgpu::Backend::Vulkan);
        info("GeForce", discrete, vulkan, "0000:01:00.0", 0x10de, 0x1b80)
    }

    fn card(bus: &str, vendor: u32, device: u32) -> Card {
        Card {
            bus: Some(bus.into()),
            vendor,
            device,
        }
    }

    #[test]
    fn the_card_with_the_monitor_wins_over_an_idle_integrated_gpu() {
        let adapters = [radeon(), geforce()];
        let cards = [card("0000:01:00.0", 0x10de, 0x1b80)];
        assert_eq!(pick(&adapters, &cards), Some(1));
    }

    #[test]
    fn the_integrated_gpu_wins_where_both_drive_a_monitor() {
        let adapters = [geforce(), radeon()];
        let cards = [
            card("0000:01:00.0", 0x10de, 0x1b80),
            card("0000:74:00.0", 0x1002, 0x13c0),
        ];
        assert_eq!(pick(&adapters, &cards), Some(1));
    }

    #[test]
    fn vulkan_wins_over_another_backend_on_the_same_gpu() {
        let mut gl = geforce();
        gl.backend = wgpu::Backend::Gl;
        let adapters = [gl, geforce()];
        let cards = [card("0000:01:00.0", 0x10de, 0x1b80)];
        assert_eq!(pick(&adapters, &cards), Some(1));
    }

    #[test]
    fn an_adapter_without_a_bus_address_is_matched_by_its_ids() {
        let mut quiet = geforce();
        quiet.device_pci_bus_id.clear();
        let adapters = [radeon(), quiet];
        let cards = [card("0000:01:00.0", 0x10de, 0x1b80)];
        assert_eq!(pick(&adapters, &cards), Some(1));
    }

    #[test]
    fn no_match_leaves_the_choice_to_the_power_preference() {
        let adapters = [radeon()];
        let cards = [card("0000:01:00.0", 0x10de, 0x1b80)];
        assert_eq!(pick(&adapters, &cards), None);
    }

    #[test]
    fn only_cards_with_a_connected_output_are_read() {
        let root = std::env::temp_dir().join(format!("gamut-drm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let make = |card: &str, bus: &str, vendor: &str, device: &str, outputs: &[(&str, &str)]| {
            let pci = root.join("devices").join(bus);
            std::fs::create_dir_all(&pci).unwrap();
            std::fs::write(pci.join("vendor"), format!("{vendor}\n")).unwrap();
            std::fs::write(pci.join("device"), format!("{device}\n")).unwrap();
            std::fs::create_dir_all(root.join(card)).unwrap();
            std::os::unix::fs::symlink(&pci, root.join(card).join("device")).unwrap();
            for (output, status) in outputs {
                let dir = root.join(format!("{card}-{output}"));
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join("status"), format!("{status}\n")).unwrap();
            }
        };
        make(
            "card0",
            "0000:74:00.0",
            "0x1002",
            "0x13c0",
            &[("DP-4", "disconnected"), ("Writeback-1", "unknown")],
        );
        make(
            "card1",
            "0000:01:00.0",
            "0x10de",
            "0x1b80",
            &[("DP-1", "disconnected"), ("HDMI-A-1", "connected")],
        );
        let cards = displaying(&root);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(cards, [card("0000:01:00.0", 0x10de, 0x1b80)]);
    }

    #[test]
    fn a_missing_directory_reads_as_no_cards() {
        assert!(displaying(Path::new("/nonexistent/drm")).is_empty());
    }
}
