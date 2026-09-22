//! What the compositor says each monitor is: the mode it is in, SDR or HDR
//! with room above white, and the room it is laid out in.
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
//! nothing to read and the surface is chosen from the request alone.
//!
//! The room is read on the same connection, for the window's opening size
//! (`app::window`). A `wl_output` carries its mode in device pixels and an
//! integer scale, and winit passes both on; but a compositor running a
//! fractional scale — 1.6, say — rounds it up to 2 there, and the true scale
//! reaches a window only once it has a surface, which is after its size was
//! asked for. `xdg_output` says how large the output is in the logical pixels
//! the compositor lays windows out in, and that with the mode is the scale it
//! is really running.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;

use wayland_client::protocol::wl_output::{self, Transform};
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum};
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
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1::{
    self as xdg_manager, ZxdgOutputManagerV1,
};
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_v1::{
    self as xdg_output, ZxdgOutputV1,
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

/// The room a monitor is laid out in: its mode in device pixels, turned the
/// way the compositor has it, and the logical size the compositor lays it out
/// at. The ratio of the two is the scale it is really running, fractional
/// where the `wl_output` alone would have said an integer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Room {
    pub device: [u32; 2],
    pub logical: [u32; 2],
}

/// What the compositor has said so far, shared with the listening thread.
#[derive(Default, PartialEq)]
struct Table {
    modes: HashMap<String, Mode>,
    /// The room above white each monitor has, where it has said: its peak
    /// over its reference white.
    headrooms: HashMap<String, f32>,
    rooms: Vec<Room>,
}

/// The monitors' modes and rooms, kept current by a thread that listens for
/// changes.
pub struct Monitors {
    table: Arc<Mutex<Table>>,
    /// Whether the compositor speaks color management at all. Where it does
    /// not, no monitor will ever have a mode, and the absence means nothing.
    speaks_modes: bool,
}

impl Monitors {
    /// The mode of the monitor the compositor calls `name` — "DP-2", say,
    /// which is the name winit gives its handle for the same monitor — or
    /// `None` for one it has not described.
    pub fn mode(&self, name: &str) -> Option<Mode> {
        self.table.lock().ok()?.modes.get(name).copied()
    }

    /// How much room above white the monitor called `name` has: the ratio
    /// of its peak luminance to its reference white, as the compositor
    /// describes it, which is what a gain map's lift is weighed against.
    /// `None` for a monitor that has not said, and 1 for one in SDR mode.
    pub fn headroom(&self, name: &str) -> Option<f32> {
        self.table.lock().ok()?.headrooms.get(name).copied()
    }

    /// Whether the compositor can say what mode a monitor is in. Where it
    /// cannot, [`Monitors::mode`] is `None` for every monitor, and that is not
    /// a monitor in SDR mode but a question nothing answers.
    pub fn speaks_modes(&self) -> bool {
        self.speaks_modes
    }

    /// The room of every monitor that has said both its mode and its logical
    /// size. Empty under a compositor without `xdg_output`.
    pub fn rooms(&self) -> Vec<Room> {
        self.table
            .lock()
            .map(|table| table.rooms.clone())
            .unwrap_or_default()
    }
}

/// Starts listening. `None` off Wayland, or under a compositor that speaks
/// neither color management nor `xdg_output`: nothing then says anything
/// about a monitor that winit does not.
///
/// The first answers are in hand before this returns, so that the window can
/// open at the right size and on the right surface; `notify` is called from
/// the listening thread whenever an answer changes after that. The thread is
/// left to die with the process: it holds nothing but a socket of its own.
pub fn watch(notify: impl Fn() + Send + 'static) -> Option<Monitors> {
    let connection = Connection::connect_to_env().ok()?;
    let mut queue = connection.new_event_queue();
    let handle = queue.handle();
    let _registry = connection.display().get_registry(&handle, ());
    let table = Arc::new(Mutex::new(Table::default()));
    let mut listener = Listener {
        manager: None,
        xdg_manager: None,
        outputs: Vec::new(),
        table: Arc::clone(&table),
        notify: Box::new(notify),
    };
    // One round trip for the globals; one for the outputs' names, modes and
    // logical sizes, and the descriptions asked for once the manager is
    // known; and one for what the descriptions hold.
    queue.roundtrip(&mut listener).ok()?;
    if listener.manager.is_none() && listener.xdg_manager.is_none() {
        return None;
    }
    let speaks_modes = listener.manager.is_some();
    for _ in 0..2 {
        queue.roundtrip(&mut listener).ok()?;
    }
    thread::Builder::new()
        .name("gamut monitors".into())
        .spawn(move || while queue.blocking_dispatch(&mut listener).is_ok() {})
        .ok()?;
    Some(Monitors {
        table,
        speaks_modes,
    })
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

/// The room above white a description says the monitor has: its peak over
/// its reference white, and 1 — none — where it has said nothing, or says
/// its peak is its white.
fn headroom_of(reading: Reading) -> f32 {
    match reading.luminances {
        Some(Luminances { max, reference }) if reference > 0 && max > reference => {
            max as f32 / reference as f32
        }
        _ => 1.0,
    }
}

/// The device pixels an output's current mode covers, turned the way the
/// compositor has the output: a mode is given as the panel scans it, and a
/// panel stood on its side is laid out with its height along the desk.
fn device_size(mode: [u32; 2], transform: Transform) -> [u32; 2] {
    match transform {
        Transform::_90 | Transform::_270 | Transform::Flipped90 | Transform::Flipped270 => {
            [mode[1], mode[0]]
        }
        _ => mode,
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
    /// Its `xdg_output`, once that manager is known.
    xdg: Option<ZxdgOutputV1>,
    name: Option<String>,
    reading: Reading,
    mode: Option<Mode>,
    /// Its current mode's size, as the panel scans it.
    scanned: Option<[u32; 2]>,
    transform: Transform,
    logical: Option<[u32; 2]>,
}

impl Output {
    fn room(&self) -> Option<Room> {
        Some(Room {
            device: device_size(self.scanned?, self.transform),
            logical: self.logical?,
        })
    }
}

struct Listener {
    manager: Option<WpColorManagerV1>,
    xdg_manager: Option<ZxdgOutputManagerV1>,
    outputs: Vec<Output>,
    table: Arc<Mutex<Table>>,
    notify: Box<dyn Fn() + Send>,
}

impl Listener {
    /// Asks for the description and the logical size of every output not yet
    /// asked about, once there is a manager to ask.
    fn subscribe(&mut self, handle: &QueueHandle<Self>) {
        for output in &mut self.outputs {
            if let Some(manager) = &self.manager
                && output.managed.is_none()
            {
                let managed = manager.get_output(&output.proxy, handle, output.global);
                managed.get_image_description(handle, output.global);
                output.managed = Some(managed);
            }
            if let Some(manager) = &self.xdg_manager
                && output.xdg.is_none()
            {
                output.xdg = Some(manager.get_xdg_output(&output.proxy, handle, output.global));
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
        let table = Table {
            modes: self
                .outputs
                .iter()
                .filter_map(|output| Some((output.name.clone()?, output.mode?)))
                .collect(),
            headrooms: self
                .outputs
                .iter()
                .filter_map(|output| {
                    output.mode?;
                    Some((output.name.clone()?, headroom_of(output.reading)))
                })
                .collect(),
            rooms: self.outputs.iter().filter_map(Output::room).collect(),
        };
        let Ok(mut shared) = self.table.lock() else {
            return;
        };
        if *shared != table {
            *shared = table;
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
                        xdg: None,
                        name: None,
                        reading: Reading::default(),
                        mode: None,
                        scanned: None,
                        transform: Transform::Normal,
                        logical: None,
                    });
                    state.subscribe(handle);
                }
                "wp_color_manager_v1" => {
                    state.manager = Some(registry.bind(name, 1, handle, ()));
                    state.subscribe(handle);
                }
                // Version 3 is where the logical size stops waiting on a
                // `done` of its own and follows the `wl_output`'s.
                "zxdg_output_manager_v1" => {
                    state.xdg_manager = Some(registry.bind(name, version.min(3), handle, ()));
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
        let Some(output) = state.output(*global) else {
            return;
        };
        match event {
            wl_output::Event::Name { name } => output.name = Some(name),
            wl_output::Event::Mode {
                flags: WEnum::Value(flags),
                width,
                height,
                ..
            } if flags.contains(wl_output::Mode::Current) => {
                output.scanned = u32::try_from(width)
                    .ok()
                    .zip(u32::try_from(height).ok())
                    .map(|(width, height)| [width, height]);
            }
            wl_output::Event::Geometry {
                transform: WEnum::Value(transform),
                ..
            } => output.transform = transform,
            _ => return,
        }
        state.publish();
    }
}

impl Dispatch<ZxdgOutputManagerV1, ()> for Listener {
    fn event(
        _: &mut Self,
        _: &ZxdgOutputManagerV1,
        _: xdg_manager::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZxdgOutputV1, u32> for Listener {
    fn event(
        state: &mut Self,
        _: &ZxdgOutputV1,
        event: xdg_output::Event,
        global: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_output::Event::LogicalSize { width, height } = event
            && let Some(output) = state.output(*global)
        {
            output.logical = u32::try_from(width)
                .ok()
                .zip(u32::try_from(height).ok())
                .map(|(width, height)| [width, height]);
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

    /// The room is the peak over the white, and a monitor that has not said
    /// — or whose peak is its white — has none.
    #[test]
    fn the_room_above_white_is_the_peak_over_the_reference() {
        use TransferFunction as Tf;
        assert!((headroom_of(reading(Some(Tf::St2084Pq), 812, 203)) - 4.0).abs() < 1e-6);
        assert_eq!(headroom_of(reading(Some(Tf::Srgb), 80, 80)), 1.0);
        assert_eq!(headroom_of(reading(None, 0, 0)), 1.0);
        assert_eq!(headroom_of(Reading::default()), 1.0);
    }

    /// A mode is the panel's own scan; a panel on its side is laid out the
    /// other way round, and its room has to be too, or the scale worked out
    /// from it would be one axis against the other.
    #[test]
    fn a_turned_output_has_its_mode_turned() {
        assert_eq!(device_size([3840, 2160], Transform::Normal), [3840, 2160]);
        assert_eq!(device_size([3840, 2160], Transform::_180), [3840, 2160]);
        assert_eq!(device_size([3840, 2160], Transform::_90), [2160, 3840]);
        assert_eq!(
            device_size([3840, 2160], Transform::Flipped270),
            [2160, 3840]
        );
    }
}
