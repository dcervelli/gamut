//! What the compositor says each monitor is in: SDR, or HDR with room above
//! white.
//!
//! A driver offers an HDR color space for a window on any monitor, and
//! nothing about the surface says what the monitor in front of it is doing.
//! The compositor knows, and on Wayland says so: every output carries an
//! image description under the color-management protocol, and it changes
//! when the monitor is switched into or out of HDR mode. This reads them, on
//! a connection of its own so that winit's is left alone, and says when one
//! changes.
//!
//! Why it matters: an HDR surface asked for on a monitor in SDR mode makes a
//! compositor that switches monitors automatically switch this one, and on
//! some drivers that is a modeset which blanks every display on the way.
//! With the mode in hand the surface can follow the monitor rather than lead
//! it. Off Wayland, or under a compositor without the protocol, there is
//! nothing to read and the surface is chosen by hand as before.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;

use wayland_client::protocol::{wl_output, wl_registry};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::wp::color_management::v1::client::wp_color_management_output_v1::{
    self as cm_output, WpColorManagementOutputV1,
};
use wayland_protocols::wp::color_management::v1::client::wp_color_manager_v1::{
    self as cm_manager, TransferFunction, WpColorManagerV1,
};
use wayland_protocols::wp::color_management::v1::client::wp_image_description_info_v1::{
    self as info, WpImageDescriptionInfoV1,
};
use wayland_protocols::wp::color_management::v1::client::wp_image_description_v1::{
    self as description, WpImageDescriptionV1,
};

/// Which mode a monitor is in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Its white is the brightest thing it shows: a surface with headroom is
    /// mapped down to fit.
    Sdr,
    /// It has room above white, and a surface with headroom lands there.
    Hdr,
}

/// The monitors' modes, kept current by a thread that listens for changes.
pub struct Monitors {
    modes: Arc<Mutex<HashMap<String, Mode>>>,
}

impl Monitors {
    /// The mode of the monitor the compositor calls `name` — "DP-2", say,
    /// which is the name winit gives its handle for the same monitor — or
    /// `None` for one it has not described.
    pub fn mode(&self, name: &str) -> Option<Mode> {
        self.modes.lock().ok()?.get(name).copied()
    }
}

/// Starts listening. `None` off Wayland, or under a compositor that does not
/// speak color management: nothing then says what a monitor is.
///
/// The first answers are in hand before this returns, so that the window can
/// open on the right surface; `notify` is called from the listening thread
/// whenever an answer changes after that. The thread is left to die with the
/// process: it holds nothing but a socket of its own.
pub fn watch(notify: impl Fn() + Send + 'static) -> Option<Monitors> {
    let connection = Connection::connect_to_env().ok()?;
    let mut queue = connection.new_event_queue();
    let handle = queue.handle();
    let _registry = connection.display().get_registry(&handle, ());
    let modes = Arc::new(Mutex::new(HashMap::new()));
    let mut listener = Listener {
        manager: None,
        outputs: Vec::new(),
        modes: Arc::clone(&modes),
        notify: Box::new(notify),
    };
    // One round trip for the globals, one for the outputs' names and the
    // descriptions asked for once the manager is known, and one for what the
    // descriptions hold.
    for _ in 0..3 {
        queue.roundtrip(&mut listener).ok()?;
        listener.manager.as_ref()?;
    }
    thread::Builder::new()
        .name("gamut monitors".into())
        .spawn(move || while queue.blocking_dispatch(&mut listener).is_ok() {})
        .ok()?;
    Some(Monitors { modes })
}

/// What a description says that decides the mode, gathered event by event
/// until its `done`.
#[derive(Default, Clone, Copy)]
struct Reading {
    transfer: Option<TransferFunction>,
    luminances: Option<Luminances>,
}

/// The peak and the reference white of a description, in cd/m².
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Luminances {
    max: u32,
    reference: u32,
}

/// The mode a description describes: HDR where its curve is one made for
/// room above white, and where its peak is brighter than its white; SDR where
/// neither, which is what a monitor in SDR mode says of itself — an sRGB or a
/// power curve, and a peak that is its reference white.
fn mode_of(reading: Reading) -> Mode {
    let curve_has_room = matches!(
        reading.transfer,
        Some(TransferFunction::St2084Pq | TransferFunction::Hlg)
    );
    let peak_above_white = reading
        .luminances
        .is_some_and(|luminances| luminances.max > luminances.reference);
    if curve_has_room || peak_above_white {
        Mode::Hdr
    } else {
        Mode::Sdr
    }
}

/// One output the compositor has advertised.
struct Output {
    /// Its name in the registry, which is how its removal is announced and
    /// how every object made from it is told apart.
    global: u32,
    proxy: wl_output::WlOutput,
    /// Its color-management side, once the manager is known.
    managed: Option<WpColorManagementOutputV1>,
    name: Option<String>,
    reading: Reading,
    mode: Option<Mode>,
}

struct Listener {
    manager: Option<WpColorManagerV1>,
    outputs: Vec<Output>,
    modes: Arc<Mutex<HashMap<String, Mode>>>,
    notify: Box<dyn Fn() + Send>,
}

impl Listener {
    /// Asks for the description of every output not yet asked about, once
    /// there is a manager to ask.
    fn subscribe(&mut self, handle: &QueueHandle<Self>) {
        let Some(manager) = &self.manager else {
            return;
        };
        for output in &mut self.outputs {
            if output.managed.is_none() {
                let managed = manager.get_output(&output.proxy, handle, output.global);
                managed.get_image_description(handle, output.global);
                output.managed = Some(managed);
            }
        }
    }

    fn output(&mut self, global: u32) -> Option<&mut Output> {
        self.outputs
            .iter_mut()
            .find(|output| output.global == global)
    }

    /// Writes what is known to the shared table, and says so if it changed.
    fn publish(&self) {
        let modes: HashMap<String, Mode> = self
            .outputs
            .iter()
            .filter_map(|output| Some((output.name.clone()?, output.mode?)))
            .collect();
        let Ok(mut shared) = self.modes.lock() else {
            return;
        };
        if *shared != modes {
            *shared = modes;
            drop(shared);
            (self.notify)();
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for Listener {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        handle: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                // Version 4 is where an output gets its name.
                "wl_output" => {
                    let proxy = registry.bind(name, version.min(4), handle, name);
                    state.outputs.push(Output {
                        global: name,
                        proxy,
                        managed: None,
                        name: None,
                        reading: Reading::default(),
                        mode: None,
                    });
                    state.subscribe(handle);
                }
                "wp_color_manager_v1" => {
                    state.manager = Some(registry.bind(name, 1, handle, ()));
                    state.subscribe(handle);
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } => {
                state.outputs.retain(|output| output.global != name);
                state.publish();
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, u32> for Listener {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        global: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Name { name } = event
            && let Some(output) = state.output(*global)
        {
            output.name = Some(name);
            state.publish();
        }
    }
}

impl Dispatch<WpColorManagerV1, ()> for Listener {
    /// What the manager supports is of no interest: nothing here sets a
    /// description, it only reads them.
    fn event(
        _: &mut Self,
        _: &WpColorManagerV1,
        _: cm_manager::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpColorManagementOutputV1, u32> for Listener {
    /// The monitor was switched over: ask again.
    fn event(
        _: &mut Self,
        managed: &WpColorManagementOutputV1,
        event: cm_output::Event,
        global: &u32,
        _: &Connection,
        handle: &QueueHandle<Self>,
    ) {
        if let cm_output::Event::ImageDescriptionChanged = event {
            managed.get_image_description(handle, *global);
        }
    }
}

impl Dispatch<WpImageDescriptionV1, u32> for Listener {
    /// A description is only a handle; what it says comes from asking for
    /// its information, and the handle is let go as soon as that is asked.
    fn event(
        state: &mut Self,
        description: &WpImageDescriptionV1,
        event: description::Event,
        global: &u32,
        _: &Connection,
        handle: &QueueHandle<Self>,
    ) {
        match event {
            description::Event::Ready { .. } => {
                if let Some(output) = state.output(*global) {
                    output.reading = Reading::default();
                    description.get_information(handle, *global);
                }
                description.destroy();
            }
            description::Event::Failed { .. } => {
                if let Some(output) = state.output(*global) {
                    output.mode = None;
                    state.publish();
                }
                description.destroy();
            }
            _ => {}
        }
    }
}

impl Dispatch<WpImageDescriptionInfoV1, u32> for Listener {
    fn event(
        state: &mut Self,
        _: &WpImageDescriptionInfoV1,
        event: info::Event,
        global: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(output) = state.output(*global) else {
            return;
        };
        match event {
            info::Event::TfNamed { tf } => output.reading.transfer = tf.into_result().ok(),
            info::Event::Luminances {
                max_lum,
                reference_lum,
                ..
            } => {
                output.reading.luminances = Some(Luminances {
                    max: max_lum,
                    reference: reference_lum,
                });
            }
            info::Event::Done => {
                output.mode = Some(mode_of(output.reading));
                state.publish();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(transfer: Option<TransferFunction>, max: u32, reference: u32) -> Reading {
        Reading {
            transfer,
            luminances: Some(Luminances { max, reference }),
        }
    }

    /// A monitor in SDR mode describes itself with an ordinary curve and a
    /// peak that is its white; one in HDR mode with a curve made for room
    /// above white, or with a peak brighter than its white whatever the
    /// curve — scRGB's linear curve says nothing on its own. Nothing said at
    /// all is SDR, the mode that costs nothing to be wrong about.
    #[test]
    fn a_monitor_is_in_hdr_mode_where_it_has_room_above_its_white() {
        use TransferFunction as Tf;
        assert_eq!(mode_of(reading(Some(Tf::Gamma22), 80, 80)), Mode::Sdr);
        assert_eq!(mode_of(reading(Some(Tf::Srgb), 80, 80)), Mode::Sdr);
        assert_eq!(mode_of(reading(Some(Tf::St2084Pq), 455, 203)), Mode::Hdr);
        assert_eq!(mode_of(reading(Some(Tf::Hlg), 1000, 203)), Mode::Hdr);
        assert_eq!(mode_of(reading(Some(Tf::ExtLinear), 455, 203)), Mode::Hdr);
        assert_eq!(mode_of(reading(Some(Tf::ExtLinear), 80, 80)), Mode::Sdr);
        assert_eq!(mode_of(reading(None, 1000, 203)), Mode::Hdr);
        assert_eq!(mode_of(Reading::default()), Mode::Sdr);
    }
}
