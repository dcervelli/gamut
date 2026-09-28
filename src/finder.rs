//! The files a Mac asks this program to open: a double click in Finder, its
//! "Open With", a file dropped on the Dock's icon, `open -a`. Launch Services
//! starts the program with no arguments and sends the files after it as an
//! Apple Event, `kAEOpenDocuments`, which winit does not pass on; so the
//! program answers the event itself and hands the paths to the event loop.
//!
//! AppKit installs its own handler for the event while it finishes
//! launching, and the event that launched the program arrives just after,
//! so this handler is installed in between, on
//! `NSApplicationWillFinishLaunchingNotification`, as Apple's documentation
//! asks: installed any earlier it would be replaced, and any later the
//! first files would already have gone to AppKit's. Once installed it
//! answers for the rest of the run, so a file opened from Finder while a
//! window is up joins it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::NSObject;
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send, sel};
use objc2_app_kit::NSApplicationWillFinishLaunchingNotification;
use objc2_foundation::{
    NSAppleEventDescriptor, NSAppleEventManager, NSNotification, NSNotificationCenter,
};

/// How the files reach the window: what `main` made from the event loop's
/// proxy.
pub type Deliver = Box<dyn Fn(Vec<PathBuf>)>;

/// Whether any files have been handed over. Read on the main thread, as
/// it is written: see [`sent_any`].
static SENT: AtomicBool = AtomicBool::new(false);

/// The four-character codes Carbon names these by, spelled as the numbers
/// they are, so that the Core Services bindings are not needed for three
/// constants.
const K_CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");
const K_AE_OPEN_DOCUMENTS: u32 = u32::from_be_bytes(*b"odoc");
const KEY_DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and `Listener`
    // implements no `Drop`.
    #[unsafe(super(NSObject))]
    #[name = "GamutOpenDocuments"]
    #[ivars = Deliver]
    struct Listener;

    impl Listener {
        #[unsafe(method(willFinishLaunching:))]
        fn will_finish_launching(&self, _notification: &NSNotification) {
            let manager = NSAppleEventManager::sharedAppleEventManager();
            // SAFETY: `self` answers the selector with the signature an
            // Apple Event handler has, and lives for the rest of the
            // process (see `listen`), so the manager's unretained
            // reference to it is never left dangling.
            unsafe {
                let _: () = msg_send![
                    &manager,
                    setEventHandler: self,
                    andSelector: sel!(openDocuments:withReplyEvent:),
                    forEventClass: K_CORE_EVENT_CLASS,
                    andEventID: K_AE_OPEN_DOCUMENTS,
                ];
            }
        }

        #[unsafe(method(openDocuments:withReplyEvent:))]
        fn open_documents(
            &self,
            event: &NSAppleEventDescriptor,
            _reply: &NSAppleEventDescriptor,
        ) {
            let paths = autoreleasepool(|_| paths(event));
            if !paths.is_empty() {
                SENT.store(true, Ordering::Relaxed);
                (self.ivars())(paths);
            }
        }
    }
);

/// Answers the files Launch Services sends from now on by handing them to
/// `deliver`. Called before the event loop runs, so that the notification
/// it waits for has not yet been posted.
pub fn listen(deliver: Deliver) {
    let listener = Listener::alloc().set_ivars(deliver);
    // SAFETY: `init` on an allocated NSObject subclass with its ivars set.
    let listener: Retained<Listener> = unsafe { msg_send![super(listener), init] };
    let center = NSNotificationCenter::defaultCenter();
    // SAFETY: `listener` answers `willFinishLaunching:` with a notification
    // argument, and is never released (below), so the center's unretained
    // reference to it stays good.
    unsafe {
        center.addObserver_selector_name_object(
            &listener,
            sel!(willFinishLaunching:),
            Some(NSApplicationWillFinishLaunchingNotification),
            None,
        );
    }
    // Neither the center nor the event manager keeps it alive, and it
    // answers for as long as the program runs.
    std::mem::forget(listener);
}

/// Whether Launch Services has sent any files yet. The files a program is
/// launched to open arrive before AppKit finishes launching, and so before
/// the event loop's `resumed`; but they reach the loop through its proxy,
/// which it reads only after, so this is how `resumed` knows they are on
/// their way and the window can wait to open at their size.
pub fn sent_any() -> bool {
    SENT.load(Ordering::Relaxed)
}

/// The files an open-documents event names: its direct object, a list of
/// file URLs, or a single one.
fn paths(event: &NSAppleEventDescriptor) -> Vec<PathBuf> {
    // SAFETY: `paramDescriptorForKeyword:` takes an `AEKeyword`, a `u32`, and
    // returns a descriptor or nil.
    let direct: Option<Retained<NSAppleEventDescriptor>> =
        unsafe { msg_send![event, paramDescriptorForKeyword: KEY_DIRECT_OBJECT] };
    let Some(direct) = direct else {
        return Vec::new();
    };
    let items = direct.numberOfItems();
    let descriptors: Vec<Retained<NSAppleEventDescriptor>> = match items {
        0 => vec![direct],
        _ => (1..=items)
            .filter_map(|index| direct.descriptorAtIndex(index))
            .collect(),
    };
    descriptors
        .iter()
        .filter_map(|descriptor| descriptor.fileURLValue())
        .filter_map(|url| url.to_file_path())
        .collect()
}

// The listener's selector for the notification must be one it answers; a
// typo would only show as a crash on the first launch from Finder.
#[cfg(test)]
mod tests {
    use super::*;
    use objc2::runtime::AnyObject;

    #[test]
    fn the_listener_answers_both_selectors() {
        let listener = Listener::alloc().set_ivars(Box::new(|_| {}));
        let listener: Retained<Listener> = unsafe { msg_send![super(listener), init] };
        let object: &AnyObject = &listener;
        for selector in [
            sel!(willFinishLaunching:),
            sel!(openDocuments:withReplyEvent:),
        ] {
            let answers: bool = unsafe { msg_send![object, respondsToSelector: selector] };
            assert!(answers, "{selector:?}");
        }
    }

    #[test]
    fn a_list_of_file_urls_is_read_as_paths() {
        let list = NSAppleEventDescriptor::listDescriptor();
        for (index, path) in ["/tmp/a picture.png", "/tmp/b.jpg"].iter().enumerate() {
            let url = objc2_foundation::NSURL::from_file_path(path).expect("a file URL");
            let item = NSAppleEventDescriptor::descriptorWithFileURL(&url);
            list.insertDescriptor_atIndex(&item, index as isize + 1);
        }
        let event = NSAppleEventDescriptor::recordDescriptor();
        let _: () =
            unsafe { msg_send![&event, setDescriptor: &*list, forKeyword: KEY_DIRECT_OBJECT] };
        assert_eq!(
            paths(&event),
            [
                PathBuf::from("/tmp/a picture.png"),
                PathBuf::from("/tmp/b.jpg")
            ]
        );
    }
}
