//! The menu bar a Mac shows at the top of the screen, built in AppKit from a
//! tree the application hands over. What is in it and what each item does
//! are the application's — see `app::menubar` — and nothing here knows: an
//! item is a title, a key equivalent and a number, and choosing it hands the
//! number back through the event loop.
//!
//! A key equivalent is shown and never acted on here. AppKit looks for one
//! among the menus before the window sees the key whenever `⌘` is held, or
//! the key is one of the keys it reads as function keys — the F keys, the
//! arrows, Page Up and Down, forward Delete — and a match is the menu's,
//! even on an item that is disabled. The key table decides what a key does,
//! by contexts the menu cannot see and with a text field sometimes taking
//! the keys, so an item a key reached hands the key on to the window's view
//! as though the menu had not been there; and every item says it is enabled
//! while a key is being matched, so that a disabled one does not swallow its
//! key. The keys AppKit leaves alone reach the window without the menu
//! being asked. The reasoning, and what was tried to find it out, is in
//! `docs/macos.md`.
//!
//! Whether each item is enabled, checked, and what it is called, is read
//! from the [`Snapshot`] the application last published: AppKit asks as a
//! menu opens, on the main thread, while the application is not running a
//! handler and so cannot be asked itself.

use std::cell::{OnceCell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAboutPanelOptionApplicationName, NSAboutPanelOptionApplicationVersion, NSApplication,
    NSEventModifierFlags, NSEventType, NSMenu, NSMenuDelegate, NSMenuItem, NSMenuItemValidation,
    NSWindow,
};
use objc2_foundation::{NSDictionary, NSString};

/// What is held with an item's key.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Modifiers {
    pub command: bool,
    pub option: bool,
    pub control: bool,
    pub shift: bool,
}

/// The key an item shows beside its title: the characters AppKit writes a
/// key equivalent as — a capital letter carrying its Shift, a named key one
/// of its private-use characters — and what is held with it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Equivalent {
    pub key: String,
    pub modifiers: Modifiers,
}

/// An item AppKit does itself, sent up the responder chain as every Mac
/// program's is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Standard {
    Hide,
    HideOthers,
    ShowAll,
    Minimize,
    Zoom,
    FullScreen,
    BringAllToFront,
}

impl Standard {
    fn action(self) -> Sel {
        match self {
            Standard::Hide => sel!(hide:),
            Standard::HideOthers => sel!(hideOtherApplications:),
            Standard::ShowAll => sel!(unhideAllApplications:),
            Standard::Minimize => sel!(performMiniaturize:),
            Standard::Zoom => sel!(performZoom:),
            Standard::FullScreen => sel!(toggleFullScreen:),
            Standard::BringAllToFront => sel!(arrangeInFront:),
        }
    }
}

/// One line of a menu.
#[derive(Clone, PartialEq, Debug)]
pub enum Node {
    /// The application's: choosing it hands `tag` back, and the snapshot's
    /// entry at `tag` says how it is shown.
    Item {
        title: String,
        key: Option<Equivalent>,
        tag: usize,
    },
    /// AppKit's own.
    Standard {
        title: String,
        key: Option<Equivalent>,
        standard: Standard,
    },
    /// The standard About panel, told the program's name and version, which
    /// a binary run outside its bundle has no other way to give it.
    About {
        title: String,
    },
    /// A menu inside this one. With a `tag`, the snapshot's entry at it says
    /// whether it can be opened.
    Submenu {
        title: String,
        tag: Option<usize>,
        nodes: Vec<Node>,
    },
    /// A menu inside this one whose items are the snapshot's `list`, made
    /// again each time it opens; choosing one hands back its place in it.
    /// The snapshot's entry at `tag` says whether it can be opened.
    List {
        title: String,
        tag: usize,
    },
    /// The Services menu, which AppKit fills.
    Services {
        title: String,
    },
    Separator,
}

/// Which of AppKit's menus a menu of the bar is, where it is one: the
/// application's own at the head, and the two AppKit adds items to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Application,
    Window,
    Help,
    Other,
}

/// One menu of the bar.
#[derive(Clone, PartialEq, Debug)]
pub struct Menu {
    pub title: String,
    pub role: Role,
    pub nodes: Vec<Node>,
}

/// How an item of the application's is shown: whether it can be chosen,
/// whether it is checked, and the title it wears in place of its own.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Shown {
    pub enabled: bool,
    pub checked: bool,
    pub title: Option<String>,
}

/// Everything the menus read as they open: an entry for each tag, and the
/// items of the one [`Node::List`].
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Snapshot {
    pub shown: Vec<Shown>,
    pub list: Vec<String>,
}

/// What was chosen with the pointer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chosen {
    /// The item at this tag.
    Item(usize),
    /// The item at this place in the snapshot's list.
    Listed(usize),
}

/// How a choice reaches the window: what `main` made from the event loop's
/// proxy.
pub type Deliver = Box<dyn Fn(Chosen)>;

/// What the program is called and which version it is, for the About panel.
pub struct Identity {
    pub name: String,
    pub version: String,
}

struct Ivars {
    deliver: Deliver,
    identity: Identity,
    snapshot: RefCell<Snapshot>,
    /// Each tag's own title, which it wears when the snapshot gives none.
    titles: RefCell<Vec<String>>,
    /// Each tag's item, where the tag is an item's: what [`Bar::rekey`]
    /// binds again.
    items: RefCell<Vec<Option<Retained<NSMenuItem>>>>,
    /// The menu that [`Node::List`] opens, made again from the snapshot.
    list: OnceCell<Retained<NSMenu>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and `Target`
    // implements no `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "GamutMenuTarget"]
    #[ivars = Ivars]
    struct Target;

    impl Target {
        /// An item of the application's, reached by the pointer or by its
        /// key.
        #[unsafe(method(choose:))]
        fn choose(&self, item: &NSMenuItem) {
            if !self.forward_key() {
                (self.ivars().deliver)(Chosen::Item(item.tag() as usize));
            }
        }

        /// An item of the list.
        #[unsafe(method(pick:))]
        fn pick(&self, item: &NSMenuItem) {
            if !self.forward_key() {
                (self.ivars().deliver)(Chosen::Listed(item.tag() as usize));
            }
        }

        /// What an item holding a submenu is sent, which it never is: it
        /// opens the submenu instead. Answered so that the item is
        /// validated, and so can be disabled — see `holder`.
        #[unsafe(method(open:))]
        fn open(&self, _item: &NSMenuItem) {}

        #[unsafe(method(about:))]
        fn about(&self, _sender: Option<&AnyObject>) {
            let mtm = self.mtm();
            let identity = &self.ivars().identity;
            let name = NSString::from_str(&identity.name);
            let version = NSString::from_str(&identity.version);
            // SAFETY: the keys are the panel's option keys, and each value
            // is the string the documentation says that key takes.
            unsafe {
                let options: Retained<NSDictionary<NSString, AnyObject>> =
                    NSDictionary::from_slices(
                        &[
                            NSAboutPanelOptionApplicationName,
                            NSAboutPanelOptionApplicationVersion,
                        ],
                        &[name.as_ref(), version.as_ref()],
                    );
                NSApplication::sharedApplication(mtm)
                    .orderFrontStandardAboutPanelWithOptions(&options);
            }
        }
    }

    unsafe impl NSObjectProtocol for Target {}

    unsafe impl NSMenuItemValidation for Target {
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> Bool {
            // A key being matched: the item takes it only to hand it on,
            // so it is always enabled — see the head of the module.
            if self.key_down() {
                return Bool::YES;
            }
            let action = item.action();
            if action != Some(sel!(choose:)) && action != Some(sel!(open:)) {
                return Bool::YES;
            }
            let tag = item.tag() as usize;
            let snapshot = self.ivars().snapshot.borrow();
            let shown = snapshot.shown.get(tag).cloned().unwrap_or_default();
            if action == Some(sel!(choose:)) {
                let titles = self.ivars().titles.borrow();
                let title = shown
                    .title
                    .as_deref()
                    .or(titles.get(tag).map(String::as_str))
                    .unwrap_or_default();
                if item.title().to_string() != title {
                    item.setTitle(&NSString::from_str(title));
                }
                item.setState(shown.checked.into());
            }
            Bool::new(shown.enabled)
        }
    }

    unsafe impl NSMenuDelegate for Target {
        /// The list, made again from what it holds now.
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            let Some(list) = self.ivars().list.get() else {
                return;
            };
            if !std::ptr::eq(menu, &**list) {
                return;
            }
            menu.removeAllItems();
            let mtm = self.mtm();
            for (index, name) in self.ivars().snapshot.borrow().list.iter().enumerate() {
                let item = item(mtm, name, None, sel!(pick:), self);
                item.setTag(index as isize);
                menu.addItem(&item);
            }
        }
    }
);

impl Target {
    /// Whether the event being answered is a key going down.
    fn key_down(&self) -> bool {
        NSApplication::sharedApplication(self.mtm())
            .currentEvent()
            .is_some_and(|event| event.r#type() == NSEventType::KeyDown)
    }

    /// Hands the key that reached an item on to the key window's view, as
    /// though the menu had not been there. Returns whether it was a key.
    fn forward_key(&self) -> bool {
        let app = NSApplication::sharedApplication(self.mtm());
        let Some(event) = app
            .currentEvent()
            .filter(|event| event.r#type() == NSEventType::KeyDown)
        else {
            return false;
        };
        if let Some(view) = app.keyWindow().and_then(|window| window.contentView()) {
            view.keyDown(&event);
        }
        true
    }
}

/// The menu bar, once installed: what the application publishes to.
pub struct Bar {
    target: Retained<Target>,
}

impl Bar {
    /// Puts `menus` up as the menu bar, in place of whatever was there, and
    /// hands each choice made in them to `deliver`. Every item starts
    /// disabled, until the first snapshot is published.
    pub fn install(
        menus: &[Menu],
        identity: Identity,
        deliver: Deliver,
        mtm: MainThreadMarker,
    ) -> Self {
        let target = Target::alloc(mtm).set_ivars(Ivars {
            deliver,
            identity,
            snapshot: RefCell::default(),
            titles: RefCell::default(),
            items: RefCell::default(),
            list: OnceCell::new(),
        });
        // SAFETY: `init` on an allocated NSObject subclass with its ivars set.
        let target: Retained<Target> = unsafe { msg_send![super(target), init] };
        let app = NSApplication::sharedApplication(mtm);
        let bar = NSMenu::new(mtm);
        for menu in menus {
            let built = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(&menu.title));
            for node in &menu.nodes {
                add(&built, node, &target, &app, mtm);
            }
            let head = NSMenuItem::new(mtm);
            head.setTitle(&NSString::from_str(&menu.title));
            head.setSubmenu(Some(&built));
            bar.addItem(&head);
            match menu.role {
                Role::Window => app.setWindowsMenu(Some(&built)),
                Role::Help => app.setHelpMenu(Some(&built)),
                Role::Application | Role::Other => {}
            }
        }
        app.setMainMenu(Some(&bar));
        // One window: AppKit's tabs, and the items it adds to the View and
        // Window menus for them, have nothing to do.
        NSWindow::setAllowsAutomaticWindowTabbing(false, mtm);
        Self { target }
    }

    /// What the menus read the next time one opens.
    pub fn publish(&self, snapshot: Snapshot) {
        *self.target.ivars().snapshot.borrow_mut() = snapshot;
    }

    /// Gives each of the application's items the key `menus` gives it, or
    /// none: `menus` is the tree that was installed, built again from a
    /// keymap read again, so that its tags are the same items'.
    pub fn rekey(&self, menus: &[Menu]) {
        let items = self.target.ivars().items.borrow();
        for menu in menus {
            rekey(&items, &menu.nodes);
        }
    }
}

/// The items of `nodes` and of the submenus in them keyed again.
fn rekey(items: &[Option<Retained<NSMenuItem>>], nodes: &[Node]) {
    for node in nodes {
        match node {
            Node::Item { key, tag, .. } => {
                let Some(Some(item)) = items.get(*tag) else {
                    continue;
                };
                match key {
                    Some(key) => set_key(item, key),
                    None => {
                        item.setKeyEquivalent(&NSString::from_str(""));
                        item.setKeyEquivalentModifierMask(NSEventModifierFlags::empty());
                    }
                }
            }
            Node::Submenu { nodes, .. } => rekey(items, nodes),
            _ => {}
        }
    }
}

/// `node` built, and added to `menu`.
fn add(menu: &NSMenu, node: &Node, target: &Target, app: &NSApplication, mtm: MainThreadMarker) {
    match node {
        Node::Item { title, key, tag } => {
            let built = item(mtm, title, key.as_ref(), sel!(choose:), target);
            built.setTag(*tag as isize);
            let mut titles = target.ivars().titles.borrow_mut();
            if titles.len() <= *tag {
                titles.resize(*tag + 1, String::new());
            }
            titles[*tag] = title.clone();
            let mut items = target.ivars().items.borrow_mut();
            if items.len() <= *tag {
                items.resize(*tag + 1, None);
            }
            items[*tag] = Some(built.clone());
            menu.addItem(&built);
        }
        Node::Standard {
            title,
            key,
            standard,
        } => {
            let built = NSMenuItem::new(mtm);
            built.setTitle(&NSString::from_str(title));
            // SAFETY: with no target the action goes up the responder
            // chain, where AppKit's own objects answer each of these.
            unsafe { built.setAction(Some(standard.action())) };
            if let Some(key) = key {
                set_key(&built, key);
            }
            menu.addItem(&built);
        }
        Node::About { title } => {
            menu.addItem(&item(mtm, title, None, sel!(about:), target));
        }
        Node::Submenu { title, tag, nodes } => {
            let inner = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
            for node in nodes {
                add(&inner, node, target, app, mtm);
            }
            menu.addItem(&holder(mtm, title, *tag, &inner, target));
        }
        Node::List { title, tag } => {
            let inner = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
            inner.setDelegate(Some(ProtocolObject::from_ref(target)));
            menu.addItem(&holder(mtm, title, Some(*tag), &inner, target));
            let _ = target.ivars().list.set(inner);
        }
        Node::Services { title } => {
            let inner = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
            app.setServicesMenu(Some(&inner));
            menu.addItem(&holder(mtm, title, None, &inner, target));
        }
        Node::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
    }
}

/// An item that sends `action` to `target`.
fn item(
    mtm: MainThreadMarker,
    title: &str,
    key: Option<&Equivalent>,
    action: Sel,
    target: &Target,
) -> Retained<NSMenuItem> {
    let built = NSMenuItem::new(mtm);
    built.setTitle(&NSString::from_str(title));
    // SAFETY: the target answers `action`, and is kept for the rest of the
    // run by the `Bar` the application holds.
    unsafe {
        built.setAction(Some(action));
        built.setTarget(Some(target));
    }
    if let Some(key) = key {
        set_key(&built, key);
    }
    built
}

/// The item a submenu hangs from. With a tag, it is sent to the target and
/// so validated by it, which is how it is disabled: AppKit enables an item
/// that only opens a submenu whatever is in it.
fn holder(
    mtm: MainThreadMarker,
    title: &str,
    tag: Option<usize>,
    submenu: &NSMenu,
    target: &Target,
) -> Retained<NSMenuItem> {
    let built = NSMenuItem::new(mtm);
    built.setTitle(&NSString::from_str(title));
    built.setSubmenu(Some(submenu));
    if let Some(tag) = tag {
        built.setTag(tag as isize);
        // SAFETY: as for `item`; `open:` is never sent, an item with a
        // submenu opening it instead, and is there to be validated.
        unsafe {
            built.setAction(Some(sel!(open:)));
            built.setTarget(Some(target));
        }
    }
    built
}

fn set_key(item: &NSMenuItem, key: &Equivalent) {
    item.setKeyEquivalent(&NSString::from_str(&key.key));
    let mut mask = NSEventModifierFlags::empty();
    for (held, flag) in [
        (key.modifiers.command, NSEventModifierFlags::Command),
        (key.modifiers.option, NSEventModifierFlags::Option),
        (key.modifiers.control, NSEventModifierFlags::Control),
        (key.modifiers.shift, NSEventModifierFlags::Shift),
    ] {
        if held {
            mask |= flag;
        }
    }
    item.setKeyEquivalentModifierMask(mask);
}
