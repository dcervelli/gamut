//! What the mouse does: every gesture a surface answers is a slot — a
//! surface, what is pressed or turned on it, and the kind of gesture — and
//! each slot holds one behavior, which the configuration file can change.
//!
//! Slots point the other way from keys. Many chords may reach one action,
//! so a key's name holds a list of chords; a slot on a surface is one place
//! for the hand to be, so it holds exactly one thing, and writing it
//! replaces what it held. The set of slots is closed: there is nothing to
//! evict from anywhere else.
//!
//! Pure, and beneath both the interface and the application: the pass asks
//! which drag a button starts, and the application which wheel it turns.
//! The modifier words are here too, since the key table reads and spells
//! them the same way.

use std::fmt::Write as _;

pub use winit::keyboard::ModifiersState as Mods;

/// The modifiers, in the order a chord or a slot writes them, each with the
/// word the configuration file reads and the word people read.
const MODIFIERS: [(Mods, &str, &str); 4] = [
    (Mods::CONTROL, "ctrl", "Ctrl"),
    (Mods::ALT, "alt", "Alt"),
    (Mods::SUPER, "super", "Super"),
    (Mods::SHIFT, "shift", "Shift"),
];

/// The modifier a word of the file names, in any case; `control` is taken
/// for `ctrl`.
pub fn read_modifier(word: &str) -> Option<Mods> {
    let word = word.to_ascii_lowercase();
    if word == "control" {
        return Some(Mods::CONTROL);
    }
    MODIFIERS
        .iter()
        .find(|(_, token, _)| *token == word)
        .map(|(mods, ..)| *mods)
}

/// `mods` as the file writes them, each followed by `+`: `ctrl+shift+`.
pub fn modifiers_token(mods: Mods) -> String {
    MODIFIERS
        .iter()
        .filter(|(modifier, ..)| mods.contains(*modifier))
        .map(|(_, token, _)| format!("{token}+"))
        .collect()
}

/// `mods` as people read them, each followed by `+`: `Ctrl+Shift+`.
pub fn modifiers_spelled(mods: Mods) -> String {
    MODIFIERS
        .iter()
        .filter(|(modifier, ..)| mods.contains(*modifier))
        .map(|(_, _, spelled)| format!("{spelled}+"))
        .collect()
}

/// What a gesture is made on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surface {
    /// The picture, with nothing of the interface over it.
    Image,
    /// The minimap's thumbnail.
    Minimap,
}

impl Surface {
    const ALL: [Surface; 2] = [Surface::Image, Surface::Minimap];

    fn token(self) -> &'static str {
        match self {
            Surface::Image => "image",
            Surface::Minimap => "minimap",
        }
    }
}

/// A button on the mouse. `Back` and `Forward` are the two on its side.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Button {
    Left,
    Middle,
    Right,
    Back,
    Forward,
}

impl Button {
    pub const ALL: [Button; 5] = [
        Button::Left,
        Button::Middle,
        Button::Right,
        Button::Back,
        Button::Forward,
    ];

    fn token(self) -> &'static str {
        match self {
            Button::Left => "left",
            Button::Middle => "middle",
            Button::Right => "right",
            Button::Back => "back",
            Button::Forward => "forward",
        }
    }

    fn spelled(self) -> &'static str {
        match self {
            Button::Left => "Left",
            Button::Middle => "Middle",
            Button::Right => "Right",
            Button::Back => "Back",
            Button::Forward => "Forward",
        }
    }

    fn read(word: &str) -> Option<Button> {
        let word = word.to_ascii_lowercase();
        Button::ALL
            .into_iter()
            .find(|button| button.token() == word)
    }
}

/// What is done with a button.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Pressed and moved.
    Drag,
    /// Held down, for as long as it is.
    Hold,
    /// Pressed and let go where it was pressed.
    Click,
}

impl Kind {
    const ALL: [Kind; 3] = [Kind::Drag, Kind::Hold, Kind::Click];

    fn token(self) -> &'static str {
        match self {
            Kind::Drag => "drag",
            Kind::Hold => "hold",
            Kind::Click => "click",
        }
    }
}

/// What is pressed or turned: a button, held with modifiers and used one
/// way; or the wheel, turned with modifiers and, it may be, a button held.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Input {
    Button {
        mods: Mods,
        button: Button,
        kind: Kind,
    },
    Wheel {
        mods: Mods,
        held: Option<Button>,
    },
}

/// One place for the hand: a surface and what is done on it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot {
    pub surface: Surface,
    pub input: Input,
}

impl Slot {
    /// The slot a name of the file names — the part after `gesture.`:
    /// `image.shift+left.drag`, `image.right+wheel`, `minimap.left.click`.
    pub fn read(name: &str) -> Result<Slot, String> {
        let (surface, rest) = name
            .split_once('.')
            .ok_or_else(|| format!("`{name}` is a surface and what is done on it"))?;
        let surface = Surface::ALL
            .into_iter()
            .find(|each| each.token() == surface.to_ascii_lowercase())
            .ok_or_else(|| format!("unknown surface `{surface}`: image or minimap"))?;
        let input = if rest.to_ascii_lowercase().ends_with("wheel") && !rest.contains('.') {
            let mut words: Vec<&str> = rest.split('+').collect();
            let wheel = words.pop().unwrap_or_default();
            if !wheel.eq_ignore_ascii_case("wheel") {
                return Err(format!("unknown gesture `{rest}`"));
            }
            let (mods, held) = modifiers_and_button(&words)?;
            if held == Some(Button::Left) {
                return Err(
                    "the left button held is a drag; hold another with the wheel".to_string(),
                );
            }
            Input::Wheel { mods, held }
        } else {
            let (input, kind) = rest.rsplit_once('.').ok_or_else(|| {
                format!("`{rest}` is a button and what is done with it, or the wheel")
            })?;
            let kind = Kind::ALL
                .into_iter()
                .find(|each| each.token() == kind.to_ascii_lowercase())
                .ok_or_else(|| format!("unknown gesture `{kind}`: drag, hold or click"))?;
            let mut words: Vec<&str> = input.split('+').collect();
            let last = words.pop().unwrap_or_default();
            let button = Button::read(last).ok_or_else(|| format!("unknown button `{last}`"))?;
            let (mods, held) = modifiers_and_button(&words)?;
            if held.is_some() {
                return Err("a button held with another is only for the wheel".to_string());
            }
            if button == Button::Left && kind == Kind::Hold {
                return Err("the left button held is a drag; hold another".to_string());
            }
            Input::Button { mods, button, kind }
        };
        if surface == Surface::Minimap
            && !matches!(
                input,
                Input::Button {
                    kind: Kind::Drag | Kind::Click,
                    ..
                }
            )
        {
            return Err("the minimap takes a drag or a click".to_string());
        }
        Ok(Slot { surface, input })
    }

    /// The slot as the file writes it, which [`Slot::read`] reads back.
    pub fn token(&self) -> String {
        let input = match self.input {
            Input::Button { mods, button, kind } => format!(
                "{}{}.{}",
                modifiers_token(mods),
                button.token(),
                kind.token()
            ),
            Input::Wheel { mods, held } => format!(
                "{}{}wheel",
                modifiers_token(mods),
                held.map_or(String::new(), |button| format!("{}+", button.token()))
            ),
        };
        format!("{}.{input}", self.surface.token())
    }

    /// The slot as people read it: `Drag`, `Shift+Drag`, `Right button
    /// held`, `Right+Wheel`, `Back`, `Minimap: Drag`.
    pub fn spell(&self) -> String {
        let input = match self.input {
            Input::Button {
                mods,
                button,
                kind: Kind::Drag,
            } => format!(
                "{}{}Drag",
                modifiers_spelled(mods),
                match button {
                    Button::Left => String::new(),
                    other => format!("{}+", other.spelled()),
                }
            ),
            Input::Button {
                mods,
                button,
                kind: Kind::Hold,
            } => format!(
                "{}{} button held",
                modifiers_spelled(mods),
                button.spelled()
            ),
            Input::Button {
                mods,
                button,
                kind: Kind::Click,
            } => format!(
                "{}{}",
                modifiers_spelled(mods),
                match button {
                    Button::Left => "Click".to_string(),
                    Button::Back | Button::Forward => button.spelled().to_string(),
                    other => format!("{} click", other.spelled()),
                }
            ),
            Input::Wheel { mods, held } => format!(
                "{}{}Wheel",
                modifiers_spelled(mods),
                held.map_or(String::new(), |button| format!("{}+", button.spelled()))
            ),
        };
        match self.surface {
            Surface::Image => input,
            Surface::Minimap => format!("Minimap: {input}"),
        }
    }

    /// The kind of gesture this is, and so which words it takes: `None` for
    /// the wheel.
    fn kind(&self) -> Option<Kind> {
        match self.input {
            Input::Button { kind, .. } => Some(kind),
            Input::Wheel { .. } => None,
        }
    }
}

/// The modifiers among `words`, and the one button, if a button is among
/// them.
fn modifiers_and_button(words: &[&str]) -> Result<(Mods, Option<Button>), String> {
    let mut mods = Mods::empty();
    let mut held = None;
    for word in words {
        if let Some(modifier) = read_modifier(word) {
            mods |= modifier;
        } else if let Some(button) = Button::read(word) {
            if held.replace(button).is_some() {
                return Err("one button held at a time".to_string());
            }
        } else {
            return Err(format!("unknown modifier or button `{word}`"));
        }
    }
    Ok((mods, held))
}

/// What a drag does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DragAction {
    /// The picture follows the pointer.
    Pan,
    /// A box is drawn out, and the view zooms to it.
    ZoomBox,
    /// The whole region is moved, from inside it; outside one, the drag is
    /// the button's plain slot's.
    MoveRegion,
}

/// What holding a button does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HoldAction {
    /// The loupe is up while the button is.
    Loupe,
}

/// What the wheel steps, a notch at a time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WheelAction {
    Zoom,
    LoupeMagnification,
    Exposure,
    BlackPoint,
    WhitePoint,
    Files,
    Frames,
    Pan,
}

/// What a slot holds.
#[derive(Clone, PartialEq, Debug)]
pub enum Behavior {
    Drag(DragAction),
    Hold(HoldAction),
    Wheel(WheelAction),
    /// A click does what a key does: the name of that key, as the file
    /// wrote it, which the application resolves through its key table.
    Click(String),
    /// On the minimap: the view centered on the point under the hand.
    Center,
    /// Nothing at all.
    None,
}

/// Every word a behavior is written as, with what it is.
const WORDS: [(&str, Behavior); 12] = [
    ("pan", Behavior::Drag(DragAction::Pan)),
    ("zoom-box", Behavior::Drag(DragAction::ZoomBox)),
    ("move-region", Behavior::Drag(DragAction::MoveRegion)),
    ("loupe", Behavior::Hold(HoldAction::Loupe)),
    ("zoom", Behavior::Wheel(WheelAction::Zoom)),
    (
        "loupe-magnification",
        Behavior::Wheel(WheelAction::LoupeMagnification),
    ),
    ("exposure", Behavior::Wheel(WheelAction::Exposure)),
    ("black-point", Behavior::Wheel(WheelAction::BlackPoint)),
    ("white-point", Behavior::Wheel(WheelAction::WhitePoint)),
    ("files", Behavior::Wheel(WheelAction::Files)),
    ("frames", Behavior::Wheel(WheelAction::Frames)),
    ("center", Behavior::Center),
];

/// The wheel's `pan`, which shares its word with the drag's.
const WHEEL_PAN: &str = "pan";

impl Behavior {
    /// The behavior `word` names in `slot`: only the words that slot's kind
    /// takes, and where it takes none of them, which ones it does.
    pub fn read(slot: &Slot, word: &str) -> Result<Behavior, String> {
        let word = word.trim();
        if word.eq_ignore_ascii_case("none") {
            return Ok(Behavior::None);
        }
        let lower = word.to_ascii_lowercase();
        let found = match (slot.surface, slot.kind()) {
            (Surface::Minimap, _) => (lower == "center").then_some(Behavior::Center),
            (Surface::Image, None) if lower == WHEEL_PAN => Some(Behavior::Wheel(WheelAction::Pan)),
            (Surface::Image, Some(Kind::Click)) => {
                return match word.contains('.') {
                    true => Ok(Behavior::Click(word.to_string())),
                    false => Err(format!(
                        "a click takes a key's name, such as files.back, or none; not `{word}`"
                    )),
                };
            }
            (Surface::Image, kind) => WORDS
                .iter()
                .find(|(each, behavior)| *each == lower && behavior.fits(kind))
                .map(|(_, behavior)| behavior.clone()),
        };
        found.ok_or_else(|| {
            format!(
                "`{word}` is not something a {} does: {}",
                slot_kind_word(slot),
                words_for(slot).join(", ")
            )
        })
    }

    /// Whether this is a behavior of `kind` — `None` being the wheel — on
    /// the picture.
    fn fits(&self, kind: Option<Kind>) -> bool {
        matches!(
            (self, kind),
            (Behavior::Drag(_), Some(Kind::Drag))
                | (Behavior::Hold(_), Some(Kind::Hold))
                | (Behavior::Wheel(_), None)
        )
    }

    /// The behavior as the file writes it, which [`Behavior::read`] reads
    /// back.
    pub fn word(&self) -> String {
        match self {
            Behavior::None => "none".to_string(),
            Behavior::Click(name) => name.clone(),
            Behavior::Wheel(WheelAction::Pan) => WHEEL_PAN.to_string(),
            other => WORDS
                .iter()
                .find(|(_, behavior)| behavior == other)
                .map_or_else(String::new, |(word, _)| word.to_string()),
        }
    }

    /// What it does, for the help popup and `--help`. `None` for a click,
    /// which is named by the key it runs.
    pub fn describe(&self) -> Option<&'static str> {
        Some(match self {
            Behavior::Drag(DragAction::Pan) => "Pan, the image following the pointer",
            Behavior::Drag(DragAction::ZoomBox) => "Zoom to the box dragged out",
            Behavior::Drag(DragAction::MoveRegion) => {
                "Move the whole region, from inside it; outside it, as without the modifier"
            }
            Behavior::Hold(HoldAction::Loupe) => "The loupe, while the button is held",
            Behavior::Wheel(WheelAction::Zoom) => "Zoom about the pointer",
            Behavior::Wheel(WheelAction::LoupeMagnification) => {
                "Step the loupe's magnification: 2, 4, 8, 16"
            }
            Behavior::Wheel(WheelAction::Exposure) => "Exposure up / down, a quarter stop a notch",
            Behavior::Wheel(WheelAction::BlackPoint) => "Black point up / down",
            Behavior::Wheel(WheelAction::WhitePoint) => "White point up / down",
            Behavior::Wheel(WheelAction::Files) => "Previous / next file",
            Behavior::Wheel(WheelAction::Frames) => "Previous / next frame or page",
            Behavior::Wheel(WheelAction::Pan) => "Pan; both ways on a trackpad",
            Behavior::Center => "Center the view on the point under the pointer",
            Behavior::Click(_) | Behavior::None => return None,
        })
    }
}

/// What a slot's kind is called, for a problem with a word given it.
fn slot_kind_word(slot: &Slot) -> &'static str {
    match (slot.surface, slot.kind()) {
        (Surface::Minimap, _) => "minimap gesture",
        (_, None) => "wheel",
        (_, Some(kind)) => kind.token(),
    }
}

/// Every word `slot` takes.
fn words_for(slot: &Slot) -> Vec<&'static str> {
    let mut words: Vec<&'static str> = match (slot.surface, slot.kind()) {
        (Surface::Minimap, _) => vec!["center"],
        (Surface::Image, Some(Kind::Click)) => vec!["a key's name"],
        (Surface::Image, kind) => {
            let mut words: Vec<&'static str> = WORDS
                .iter()
                .filter(|(_, behavior)| behavior.fits(kind))
                .map(|(word, _)| *word)
                .collect();
            if kind.is_none() {
                words.push(WHEEL_PAN);
            }
            words
        }
    };
    words.push("none");
    words
}

/// Every slot's behavior.
#[derive(Clone, PartialEq, Debug)]
pub struct Gestures {
    slots: Vec<(Slot, Behavior)>,
}

/// A button on the picture, held with nothing.
const fn image(button: Button, kind: Kind) -> Slot {
    Slot {
        surface: Surface::Image,
        input: Input::Button {
            mods: Mods::empty(),
            button,
            kind,
        },
    }
}

impl Default for Gestures {
    fn default() -> Self {
        let wheel = |held| Slot {
            surface: Surface::Image,
            input: Input::Wheel {
                mods: Mods::empty(),
                held,
            },
        };
        let minimap = |kind| Slot {
            surface: Surface::Minimap,
            input: Input::Button {
                mods: Mods::empty(),
                button: Button::Left,
                kind,
            },
        };
        Self {
            slots: vec![
                (
                    image(Button::Left, Kind::Drag),
                    Behavior::Drag(DragAction::Pan),
                ),
                (
                    Slot {
                        surface: Surface::Image,
                        input: Input::Button {
                            mods: Mods::SHIFT,
                            button: Button::Left,
                            kind: Kind::Drag,
                        },
                    },
                    Behavior::Drag(DragAction::MoveRegion),
                ),
                (
                    image(Button::Right, Kind::Hold),
                    Behavior::Hold(HoldAction::Loupe),
                ),
                (wheel(None), Behavior::Wheel(WheelAction::Zoom)),
                (
                    wheel(Some(Button::Right)),
                    Behavior::Wheel(WheelAction::LoupeMagnification),
                ),
                (
                    image(Button::Back, Kind::Click),
                    Behavior::Click("files.back".to_string()),
                ),
                (
                    image(Button::Forward, Kind::Click),
                    Behavior::Click("files.forward".to_string()),
                ),
                (minimap(Kind::Drag), Behavior::Center),
                (minimap(Kind::Click), Behavior::Center),
            ],
        }
    }
}

impl Gestures {
    /// Puts `behavior` in `slot`, in place of whatever it held.
    pub fn set(&mut self, slot: Slot, behavior: Behavior) {
        match self.slots.iter_mut().find(|(each, _)| *each == slot) {
            Some((_, held)) => *held = behavior,
            None => self.slots.push((slot, behavior)),
        }
    }

    /// What `slot` holds, where anything is written for it.
    pub fn get(&self, slot: &Slot) -> Option<&Behavior> {
        self.slots
            .iter()
            .find(|(each, _)| each == slot)
            .map(|(_, behavior)| behavior)
    }

    fn button(
        &self,
        surface: Surface,
        mods: Mods,
        button: Button,
        kind: Kind,
    ) -> Option<&Behavior> {
        self.get(&Slot {
            surface,
            input: Input::Button { mods, button, kind },
        })
    }

    /// What a drag of `button` held with `mods` does on `surface`.
    pub fn drag(&self, surface: Surface, mods: Mods, button: Button) -> Option<DragAction> {
        match self.button(surface, mods, button, Kind::Drag)? {
            Behavior::Drag(action) => Some(*action),
            _ => None,
        }
    }

    /// What holding `button` with `mods` does on `surface`.
    pub fn hold(&self, surface: Surface, mods: Mods, button: Button) -> Option<HoldAction> {
        match self.button(surface, mods, button, Kind::Hold)? {
            Behavior::Hold(action) => Some(*action),
            _ => None,
        }
    }

    /// What the wheel steps on `surface`, turned with `mods` and `held`.
    pub fn wheel(&self, surface: Surface, mods: Mods, held: Option<Button>) -> Option<WheelAction> {
        match self.get(&Slot {
            surface,
            input: Input::Wheel { mods, held },
        })? {
            Behavior::Wheel(action) => Some(*action),
            _ => None,
        }
    }

    /// The key a click of `button` with `mods` on `surface` runs, by name.
    pub fn click(&self, surface: Surface, mods: Mods, button: Button) -> Option<&str> {
        match self.button(surface, mods, button, Kind::Click)? {
            Behavior::Click(name) => Some(name),
            _ => None,
        }
    }

    /// Whether `button` used as `kind` with `mods` on the minimap centers
    /// the view.
    pub fn center(&self, mods: Mods, button: Button, kind: Kind) -> bool {
        self.button(Surface::Minimap, mods, button, kind) == Some(&Behavior::Center)
    }

    /// The first slot on the picture whose hold is the loupe, spelled, for
    /// the loupe's tooltip.
    pub fn held_for_loupe(&self) -> Option<String> {
        self.slots
            .iter()
            .find(|(slot, behavior)| {
                slot.surface == Surface::Image && *behavior == Behavior::Hold(HoldAction::Loupe)
            })
            .map(|(slot, _)| slot.spell())
    }

    /// The first wheel on the picture that steps the loupe's magnification,
    /// spelled, for the same tooltip.
    pub fn wheel_for_magnification(&self) -> Option<String> {
        self.slots
            .iter()
            .find(|(slot, behavior)| {
                slot.surface == Surface::Image
                    && *behavior == Behavior::Wheel(WheelAction::LoupeMagnification)
            })
            .map(|(slot, _)| slot.spell())
    }

    /// Every slot holding something, with how it is spelled and what it
    /// holds, in the order they were set: the defaults first.
    pub fn rows(&self) -> impl Iterator<Item = (String, &Behavior)> {
        self.slots
            .iter()
            .filter(|(_, behavior)| *behavior != Behavior::None)
            .map(|(slot, behavior)| (slot.spell(), behavior))
    }

    /// Every slot at its default, commented out, under a comment giving the
    /// grammar and the words each kind takes.
    pub fn template(&self) -> String {
        let mut text = String::from(
            "\n# Gestures: each gesture.<surface>.<input> takes one behavior.\n\
             # A surface is image or minimap. An input is [mods+]<button>.<drag|hold|click>,\n\
             # the button left, middle, right, back or forward, or [mods+][button+]wheel\n\
             # for the wheel turned with a button held. Modifiers as for keys.\n\
             # A drag takes pan, zoom-box, move-region or none; a hold loupe or none;\n\
             # the wheel zoom, loupe-magnification, exposure, black-point, white-point,\n\
             # files, frames, pan or none; a click a key's name or none. On the\n\
             # minimap, a drag or a click takes center or none. Whatever the slot, a\n\
             # drag started with a region asked for draws it, one from a handle pulls\n\
             # it, and one with zoom.fit's key held zooms to a box.\n",
        );
        for (slot, behavior) in &self.slots {
            let _ = writeln!(text, "# gesture.{} = {}", slot.token(), behavior.word());
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slot_reads_back_what_it_writes() {
        let mut names: Vec<String> = Gestures::default()
            .slots
            .iter()
            .map(|(slot, _)| slot.token())
            .collect();
        names.extend(
            [
                "image.ctrl+middle+wheel",
                "image.shift+left.drag",
                "image.back.click",
                "image.alt+super+right.hold",
            ]
            .map(str::to_string),
        );
        for name in names {
            let slot = Slot::read(&name).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(slot.token(), name);
        }
        assert_eq!(
            Slot::read("image.Control+Middle+Wheel").map(|slot| slot.token()),
            Ok("image.ctrl+middle+wheel".to_string())
        );
    }

    #[test]
    fn a_held_button_is_only_for_the_wheel() {
        assert!(Slot::read("image.right+left.drag").is_err());
        assert!(Slot::read("image.middle+right.click").is_err());
        assert!(Slot::read("image.left+wheel").is_err());
        assert!(Slot::read("image.left.hold").is_err());
        assert!(Slot::read("image.middle+right+wheel").is_err());
        assert!(Slot::read("image.right+wheel").is_ok());
        // Nor anything the grammar does not have.
        assert!(Slot::read("desk.left.drag").is_err());
        assert!(Slot::read("image.left.tap").is_err());
        assert!(Slot::read("image.thumb.drag").is_err());
        assert!(Slot::read("image.hyper+left.drag").is_err());
        assert!(Slot::read("minimap.wheel").is_err());
        assert!(Slot::read("minimap.left.hold").is_err());
    }

    #[test]
    fn a_slot_is_spelled_for_people() {
        let spelled = |name| Slot::read(name).unwrap().spell();
        assert_eq!(spelled("image.left.drag"), "Drag");
        assert_eq!(spelled("image.shift+left.drag"), "Shift+Drag");
        assert_eq!(spelled("image.middle.drag"), "Middle+Drag");
        assert_eq!(spelled("image.right.hold"), "Right button held");
        assert_eq!(spelled("image.wheel"), "Wheel");
        assert_eq!(spelled("image.right+wheel"), "Right+Wheel");
        assert_eq!(spelled("image.ctrl+wheel"), "Ctrl+Wheel");
        assert_eq!(spelled("image.back.click"), "Back");
        assert_eq!(spelled("image.middle.click"), "Middle click");
        assert_eq!(spelled("minimap.left.drag"), "Minimap: Drag");
    }

    #[test]
    fn a_word_of_the_wrong_kind_is_refused_with_the_right_ones() {
        let slot = |name| Slot::read(name).unwrap();
        let error = Behavior::read(&slot("image.left.drag"), "loupe").unwrap_err();
        assert!(
            error.contains("pan, zoom-box, move-region, none"),
            "{error}"
        );
        assert!(Behavior::read(&slot("image.right.hold"), "pan").is_err());
        assert!(Behavior::read(&slot("image.wheel"), "loupe").is_err());
        assert!(Behavior::read(&slot("minimap.left.drag"), "pan").is_err());
        assert!(Behavior::read(&slot("image.middle.click"), "pan").is_err());
        assert_eq!(
            Behavior::read(&slot("image.wheel"), "pan"),
            Ok(Behavior::Wheel(WheelAction::Pan))
        );
        assert_eq!(
            Behavior::read(&slot("image.middle.click"), "interface.grid"),
            Ok(Behavior::Click("interface.grid".to_string()))
        );
        assert_eq!(
            Behavior::read(&slot("image.left.drag"), "None"),
            Ok(Behavior::None)
        );
        // And every word reads back as itself, where its kind takes it.
        for (word, behavior) in &WORDS {
            let name = match behavior {
                Behavior::Drag(_) => "image.left.drag",
                Behavior::Hold(_) => "image.right.hold",
                Behavior::Wheel(_) => "image.wheel",
                _ => "minimap.left.click",
            };
            assert_eq!(Behavior::read(&slot(name), word).as_ref(), Ok(behavior));
            assert_eq!(behavior.word(), *word);
        }
    }

    #[test]
    fn set_replaces_what_a_slot_held() {
        let mut gestures = Gestures::default();
        let count = gestures.slots.len();
        let slot = Slot::read("image.left.drag").unwrap();
        gestures.set(slot, Behavior::Drag(DragAction::ZoomBox));
        assert_eq!(gestures.slots.len(), count);
        assert_eq!(
            gestures.drag(Surface::Image, Mods::empty(), Button::Left),
            Some(DragAction::ZoomBox)
        );
        gestures.set(
            Slot::read("image.middle.hold").unwrap(),
            Behavior::Hold(HoldAction::Loupe),
        );
        assert_eq!(gestures.slots.len(), count + 1);
        assert_eq!(
            gestures.hold(Surface::Image, Mods::empty(), Button::Middle),
            Some(HoldAction::Loupe)
        );
    }

    #[test]
    fn the_lookups_are_exact() {
        let gestures = Gestures::default();
        assert_eq!(
            gestures.drag(Surface::Image, Mods::empty(), Button::Left),
            Some(DragAction::Pan)
        );
        assert_eq!(
            gestures.drag(Surface::Image, Mods::SHIFT, Button::Left),
            Some(DragAction::MoveRegion)
        );
        assert_eq!(
            gestures.drag(Surface::Image, Mods::CONTROL, Button::Left),
            None
        );
        assert_eq!(
            gestures.drag(Surface::Image, Mods::empty(), Button::Middle),
            None
        );
        assert_eq!(gestures.wheel(Surface::Image, Mods::CONTROL, None), None);
        assert_eq!(
            gestures.wheel(Surface::Image, Mods::empty(), Some(Button::Right)),
            Some(WheelAction::LoupeMagnification)
        );
        assert_eq!(
            gestures.click(Surface::Image, Mods::empty(), Button::Back),
            Some("files.back")
        );
        assert!(gestures.center(Mods::empty(), Button::Left, Kind::Drag));
        assert!(!gestures.center(Mods::empty(), Button::Middle, Kind::Drag));
    }

    #[test]
    fn the_template_lists_every_default() {
        let template = Gestures::default().template();
        let mut gestures = Gestures { slots: Vec::new() };
        for line in template
            .lines()
            .filter_map(|line| line.strip_prefix("# gesture."))
        {
            let (name, word) = line.split_once(" = ").expect("name = value");
            let slot = Slot::read(name).unwrap();
            gestures.set(slot, Behavior::read(&slot, word).unwrap());
        }
        assert_eq!(gestures, Gestures::default());
    }
}
