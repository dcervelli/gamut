//! The picture measured again through its gain map's lift, on a thread of
//! its own. A scan walks the whole picture — a tenth of a second or more
//! for a large one in a debug build — and a Mac's display ramps its room
//! above white up over a second or two once a window asks for it, each
//! step a new weight for the lift. Scanned on the loop, every step was a
//! stall; here the picture follows the room at once and the numbers catch
//! up behind it.
//!
//! One thread, and one request waiting for it: a request made while
//! another waits replaces it, so that a ramp of a dozen steps is at most
//! the scan already under way and the last one asked for. A report made
//! stale by a later request is thrown away when it lands.
//!
//! The two measures a switch moves between are kept as well — the base the
//! loader measured, and the picture at the last weight scanned — so that
//! pressing the switch back and forth measures nothing again.

use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread::JoinHandle;

use crate::image::DecodedImage;
use crate::image::gain_map::Table;
use crate::image::stats::Stats;

/// How a measure comes home: through the event loop, as the folder does.
pub type Deliver = super::Deliver<Measured>;

/// A picture measured through its lift, on its way home.
pub struct Measured {
    /// Which request it answers; only the last one asked for is taken.
    asked: u64,
    /// The picture measured, held loosely: one stepped past in the meantime
    /// is not kept alive by its own measure.
    image: Weak<DecodedImage>,
    weight: f32,
    stats: Stats,
}

/// What the thread is asked for next.
struct Request {
    asked: u64,
    image: Arc<DecodedImage>,
    table: Arc<Table>,
}

/// The request waiting for the thread, and whether the thread is to stop.
#[derive(Default)]
struct Slot {
    next: Option<Request>,
    closed: bool,
}

/// A measure kept for a picture, and the weight it was taken at.
struct Kept {
    image: Weak<DecodedImage>,
    weight: f32,
    stats: Stats,
}

impl Kept {
    fn of(&self, image: &Arc<DecodedImage>, weight: f32) -> Option<&Stats> {
        (self.weight == weight && is(&self.image, image)).then_some(&self.stats)
    }
}

/// Whether `held` is `image`, without holding it any tighter.
fn is(held: &Weak<DecodedImage>, image: &Arc<DecodedImage>) -> bool {
    std::ptr::eq(held.as_ptr(), Arc::as_ptr(image))
}

pub(super) struct Measuring {
    deliver: Deliver,
    slot: Arc<(Mutex<Slot>, Condvar)>,
    /// Started at the first request, so that a session with no gain map in
    /// it never has one.
    thread: Option<JoinHandle<()>>,
    /// The last request made, or given up on: a report answering any other
    /// is stale.
    asked: u64,
    /// The picture unlifted, as the loader measured it.
    base: Option<Kept>,
    /// The picture at the last weight scanned.
    lifted: Option<Kept>,
}

impl Measuring {
    pub fn new(deliver: Deliver) -> Self {
        Self {
            deliver,
            slot: Arc::default(),
            thread: None,
            asked: 0,
            base: None,
            lifted: None,
        }
    }

    /// Keeps `stats` as `image`'s base, to be handed back when the lift
    /// goes to nothing.
    pub fn keep_base(&mut self, image: &Arc<DecodedImage>, stats: &Stats) {
        self.base = Some(Kept {
            image: Arc::downgrade(image),
            weight: 0.0,
            stats: stats.clone(),
        });
    }

    /// The measure of `image` through `table`, where one is kept: the base
    /// at no weight, the last scan at its own. Otherwise asks the thread for
    /// it and says `None`; the answer arrives as a [`Measured`]. Either way,
    /// whatever was asked before is given up on.
    pub fn measure(&mut self, image: &Arc<DecodedImage>, table: &Arc<Table>) -> Option<Stats> {
        self.asked += 1;
        let weight = table.weight();
        let kept = if weight == 0.0 {
            &self.base
        } else {
            &self.lifted
        };
        if let Some(stats) = kept.as_ref().and_then(|kept| kept.of(image, weight)) {
            // Nothing for the thread to do: a request still waiting is
            // stale, and taking it back saves the scan.
            let (lock, _) = &*self.slot;
            if let Ok(mut slot) = lock.lock() {
                slot.next = None;
            }
            return Some(stats.clone());
        }
        self.ask(Request {
            asked: self.asked,
            image: Arc::clone(image),
            table: Arc::clone(table),
        });
        None
    }

    /// The stats a report carries, where it answers the last request made
    /// and `image` is still the picture it measured; kept, for a switch
    /// back to its weight.
    pub fn arrived(&mut self, measured: Measured, image: &Arc<DecodedImage>) -> Option<Stats> {
        if measured.asked != self.asked || !is(&measured.image, image) {
            return None;
        }
        let stats = measured.stats.clone();
        self.lifted = Some(Kept {
            image: measured.image,
            weight: measured.weight,
            stats: measured.stats,
        });
        Some(stats)
    }

    fn ask(&mut self, request: Request) {
        let (lock, wake) = &*self.slot;
        let Ok(mut slot) = lock.lock() else {
            return;
        };
        slot.next = Some(request);
        wake.notify_one();
        drop(slot);
        if self.thread.is_none() {
            let slot = Arc::clone(&self.slot);
            let deliver = Arc::clone(&self.deliver);
            self.thread = std::thread::Builder::new()
                .name("measure".into())
                .spawn(move || work(&slot, &deliver))
                .map_err(|error| eprintln!("gamut: cannot measure the lifted picture: {error}"))
                .ok();
        }
    }
}

impl Drop for Measuring {
    fn drop(&mut self) {
        let (lock, wake) = &*self.slot;
        if let Ok(mut slot) = lock.lock() {
            slot.closed = true;
            slot.next = None;
        }
        wake.notify_one();
    }
}

/// The thread: waits for a request, measures it, sends it home, and waits
/// again, until the loop's side is dropped.
fn work(slot: &(Mutex<Slot>, Condvar), deliver: &Deliver) {
    let (lock, wake) = slot;
    loop {
        let request = {
            let Ok(mut slot) = lock.lock() else {
                return;
            };
            loop {
                if slot.closed {
                    return;
                }
                if let Some(request) = slot.next.take() {
                    break request;
                }
                slot = match wake.wait(slot) {
                    Ok(slot) => slot,
                    Err(_) => return,
                };
            }
        };
        let stats = Stats::scan_with(&request.image, Some(&request.table));
        deliver(Measured {
            asked: request.asked,
            image: Arc::downgrade(&request.image),
            weight: request.table.weight(),
            stats,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::gain_map::{GainMap, Lift};
    use crate::image::{AlphaMode, Channels, ColorSpace, Referred, Samples};
    use std::sync::mpsc;
    use std::time::Duration;

    fn lifted() -> Arc<DecodedImage> {
        Arc::new(DecodedImage {
            width: 2,
            height: 1,
            samples: Samples::U8 {
                channels: Channels::Rgb,
                data: vec![128; 6],
            },
            color: ColorSpace::SRGB,
            alpha: AlphaMode::Opaque,
            referred: Referred::Display,
            exposure: None,
            nodata: None,
            gain_map: Some(Arc::new(GainMap {
                width: 2,
                height: 1,
                channels: 1,
                data: vec![0, 255],
                lift: Lift::Apple { headroom: 4.0 },
            })),
            depth: None,
        })
    }

    fn table(image: &DecodedImage, weight: f32) -> Arc<Table> {
        Arc::new(image.gain_map.as_ref().expect("a map").table(weight))
    }

    fn measuring() -> (Measuring, mpsc::Receiver<Measured>) {
        let (send, receive) = mpsc::channel();
        let send = Mutex::new(send);
        let deliver: Deliver = Arc::new(move |measured| {
            let _ = send.lock().expect("not poisoned").send(measured);
        });
        (Measuring::new(deliver), receive)
    }

    fn next(receive: &mpsc::Receiver<Measured>) -> Measured {
        receive
            .recv_timeout(Duration::from_secs(10))
            .expect("the thread reports")
    }

    /// A weight measured on the thread is taken when it lands, and kept:
    /// asked for again it is there at once, as the base is at no weight.
    #[test]
    fn a_measure_lands_and_is_kept_with_the_base() {
        let image = lifted();
        let (mut measuring, receive) = measuring();
        let base = Stats::scan(&image);
        measuring.keep_base(&image, &base);

        assert!(measuring.measure(&image, &table(&image, 1.0)).is_none());
        let stats = measuring
            .arrived(next(&receive), &image)
            .expect("the last asked for");
        assert!(stats.max > base.max, "lifted above the base");

        let back = measuring.measure(&image, &table(&image, 0.0));
        assert_eq!(
            back.map(|stats| stats.max),
            Some(base.max),
            "the base, kept"
        );
        let again = measuring.measure(&image, &table(&image, 1.0));
        assert_eq!(
            again.map(|stats| stats.max),
            Some(stats.max),
            "the lift, kept"
        );
    }

    /// A report answering a request made before the last is stale, and so
    /// is one about a picture no longer up.
    #[test]
    fn a_stale_measure_is_thrown_away() {
        let image = lifted();
        let (mut measuring, receive) = measuring();
        assert!(measuring.measure(&image, &table(&image, 0.5)).is_none());
        let first = next(&receive);
        assert!(measuring.measure(&image, &table(&image, 1.0)).is_none());
        assert!(measuring.arrived(first, &image).is_none(), "superseded");
        let second = next(&receive);
        assert!(
            measuring.arrived(second, &lifted()).is_none(),
            "another picture"
        );
    }
}
