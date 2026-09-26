//! Which chords reach which actions: every shortcut's stable dotted name,
//! the chords it answers to, and how a key pressed is looked up among them.
//!
//! The table itself — the rows `--help` prints, each with its names, actions
//! and default chords — is [`super::input::ROWS`]. What is here is the
//! machinery around it, generic over the rows so that the tests can use
//! small tables of their own: reading a chord as the configuration file
//! writes it and spelling it for people, rebinding a name, and dispatch.
//!
//! A name goes to any number of chords, and a chord has one holder in each
//! context: binding a chord to one name takes it from whichever other name
//! in the same context held it. A context is where a name is tried first —
//! [`ORDER`] — before the plain names, which hold always.

use std::fmt::Write as _;

use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};

use super::input::{Action, Section, When};
use crate::gestures::{Mods, modifiers_spelled, modifiers_token, read_modifier};

/// A key: the character it types, the name of one that types none, or —
/// where the character would depend on the layout — the place on the
/// keyboard it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyName {
    /// Shift is part of the character: `L` is Shift+`l`, and `>` is
    /// Shift+`.` on most keyboards. A chord on a character is never held
    /// with Shift besides.
    Char(char),
    Named(NamedKey),
    /// Matched by position rather than by what it types. For the number row,
    /// whose shifted characters are whatever the layout puts there: `@` is
    /// Shift+`2` on one keyboard and `"` on another, and the zoom that hangs
    /// off `2` should be under `2` on both.
    Position(KeyCode),
}

use KeyName::{Char, Named, Position};

/// The number row, in order from `0`.
const DIGITS: [KeyCode; 10] = [
    KeyCode::Digit0,
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

/// Every named key the file can write, with its word there and its word
/// for people. The first word of a key is the one it is written back as;
/// a later line for the same key is another word read for it.
const NAMED: &[(NamedKey, &str, &str)] = &[
    (NamedKey::Space, "space", "Space"),
    (NamedKey::Enter, "enter", "Enter"),
    (NamedKey::Enter, "return", "Enter"),
    (NamedKey::Escape, "esc", "Esc"),
    (NamedKey::Escape, "escape", "Esc"),
    (NamedKey::Tab, "tab", "Tab"),
    (NamedKey::Backspace, "backspace", "\u{232b}"),
    (NamedKey::Delete, "delete", "Del"),
    (NamedKey::Delete, "del", "Del"),
    (NamedKey::Insert, "insert", "Ins"),
    (NamedKey::Home, "home", "Home"),
    (NamedKey::End, "end", "End"),
    (NamedKey::PageUp, "pageup", "Page Up"),
    (NamedKey::PageDown, "pagedown", "Page Down"),
    (NamedKey::ArrowLeft, "left", "Left"),
    (NamedKey::ArrowRight, "right", "Right"),
    (NamedKey::ArrowUp, "up", "Up"),
    (NamedKey::ArrowDown, "down", "Down"),
    (NamedKey::F1, "f1", "F1"),
    (NamedKey::F2, "f2", "F2"),
    (NamedKey::F3, "f3", "F3"),
    (NamedKey::F4, "f4", "F4"),
    (NamedKey::F5, "f5", "F5"),
    (NamedKey::F6, "f6", "F6"),
    (NamedKey::F7, "f7", "F7"),
    (NamedKey::F8, "f8", "F8"),
    (NamedKey::F9, "f9", "F9"),
    (NamedKey::F10, "f10", "F10"),
    (NamedKey::F11, "f11", "F11"),
    (NamedKey::F12, "f12", "F12"),
];

/// The four arrows, which a column of four with one prefix names at once.
const ARROWS: [NamedKey; 4] = [
    NamedKey::ArrowLeft,
    NamedKey::ArrowRight,
    NamedKey::ArrowUp,
    NamedKey::ArrowDown,
];

/// A key and what is held down with it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Chord {
    /// Ctrl, Alt and Super, held exactly as written: a chord nothing binds
    /// belongs to the window manager. Shift as well, for a named key or a
    /// place on the number row; never for a character, which carries it.
    pub mods: Mods,
    pub key: KeyName,
}

impl Chord {
    pub const fn new(mods: Mods, key: KeyName) -> Self {
        Self { mods, key }
    }

    /// A chord as the configuration file writes it: modifiers joined by
    /// `+`, then the key — `ctrl+shift+c`, `shift+2`, `pageup`, `ctrl+>`,
    /// `ctrl++`.
    pub fn read(token: &str) -> Result<Chord, String> {
        let (prefix, key) = if token == "+" {
            ("", "+")
        } else if let Some(prefix) = token.strip_suffix("++") {
            (prefix, "+")
        } else {
            token.rsplit_once('+').unwrap_or(("", token))
        };
        if key.is_empty() {
            return Err(format!("`{token}` names no key"));
        }
        let mut mods = Mods::empty();
        for word in prefix
            .split('+')
            .filter(|word| !prefix.is_empty() || !word.is_empty())
        {
            mods |= read_modifier(word)
                .ok_or_else(|| format!("unknown modifier `{word}` in `{token}`"))?;
        }
        let mut characters = key.chars();
        let key = match (characters.next(), characters.next()) {
            (Some(digit @ '0'..='9'), None) => Position(DIGITS[digit as usize - '0' as usize]),
            (Some(character), None) => {
                let shifted = mods.contains(Mods::SHIFT);
                mods.remove(Mods::SHIFT);
                match character {
                    _ if !shifted => Char(character),
                    'a'..='z' | 'A'..='Z' => Char(character.to_ascii_uppercase()),
                    _ => {
                        return Err(format!(
                            "write the character Shift types, as `>` rather than \
                             `shift+.`, not `{token}`"
                        ));
                    }
                }
            }
            _ => {
                let word = key.to_ascii_lowercase();
                let named = NAMED
                    .iter()
                    .find(|(_, each, _)| *each == word)
                    .ok_or_else(|| format!("unknown key `{key}`"))?;
                Named(named.0)
            }
        };
        Ok(Chord { mods, key })
    }

    /// The chord as the configuration file writes it, which [`Chord::read`]
    /// reads back. A capital letter is written with `shift+`.
    pub fn token(&self) -> String {
        let mut mods = self.mods;
        let key = match self.key {
            Char(character) if character.is_ascii_uppercase() => {
                mods |= Mods::SHIFT;
                character.to_ascii_lowercase().to_string()
            }
            Char(character) => character.to_string(),
            Named(named) => NAMED
                .iter()
                .find(|(each, ..)| *each == named)
                .map_or_else(|| format!("{named:?}"), |(_, word, _)| word.to_string()),
            Position(code) => digit(code).map_or_else(|| format!("{code:?}"), |d| d.to_string()),
        };
        format!("{}{key}", modifiers_token(mods))
    }

    /// The chord as people read it: `Ctrl+Z`, `m`, `Shift+L`,
    /// `Ctrl+Shift+C`, `Ctrl+>`, `Page Up`. A letter is a capital with
    /// Ctrl, Alt or Super held, as the keycap is; alone it is the letter it
    /// types.
    pub fn spell(&self) -> String {
        format!("{}{}", self.prefix(), self.key_spelled())
    }

    /// What is held with the key, as [`Chord::spell`] writes it.
    fn prefix(&self) -> String {
        let mut mods = self.mods;
        if matches!(self.key, Char(character) if character.is_ascii_uppercase()) {
            mods |= Mods::SHIFT;
        }
        modifiers_spelled(mods)
    }

    /// The key alone, as [`Chord::spell`] writes it.
    fn key_spelled(&self) -> String {
        match self.key {
            Char(character) if character.is_ascii_lowercase() && !self.mods.is_empty() => {
                character.to_ascii_uppercase().to_string()
            }
            Char(character) => character.to_string(),
            Named(named) => NAMED
                .iter()
                .find(|(each, ..)| *each == named)
                .map_or_else(|| format!("{named:?}"), |(.., spelled)| spelled.to_string()),
            Position(code) => digit(code).map_or_else(|| format!("{code:?}"), |d| d.to_string()),
        }
    }
}

/// Which digit of the number row `code` is.
fn digit(code: KeyCode) -> Option<usize> {
    DIGITS.iter().position(|each| *each == code)
}

/// Several chords as one cell of a column: each in full, joined by `, `,
/// except that the four arrows held with one prefix are `Arrows` under it,
/// and a run of the number row held with one prefix names it once —
/// `Shift+2, 3, 4`.
pub fn spell_all(chords: &[Chord]) -> String {
    let mut words = Vec::new();
    let mut index = 0;
    while index < chords.len() {
        let chord = chords[index];
        let arrows = chords.get(index..index + 4).filter(|four| {
            four.iter().all(|each| each.mods == chord.mods)
                && ARROWS
                    .iter()
                    .all(|arrow| four.iter().any(|each| each.key == Named(*arrow)))
        });
        if arrows.is_some() {
            words.push(format!("{}Arrows", chord.prefix()));
            index += 4;
            continue;
        }
        let run_on = index > 0
            && matches!(chord.key, Position(_))
            && matches!(chords[index - 1].key, Position(_))
            && chords[index - 1].mods == chord.mods;
        words.push(match run_on {
            true => chord.key_spelled(),
            false => chord.spell(),
        });
        index += 1;
    }
    words.join(", ")
}

/// Where a name is tried before the plain names: while its condition
/// holds, the name that holds a chord in it answers the chord.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Context {
    /// A region is selected on the picture.
    Region,
}

/// The contexts, in the order they are tried; the plain names come after
/// all of them.
pub const ORDER: &[Context] = &[Context::Region];

/// One line of `--help`, of the help popup and of the template.
#[derive(PartialEq, Debug)]
pub struct Row {
    pub section: Section,
    /// When the line does anything at all, for the help popup to dim it by;
    /// and, where it is a context's, where its names are tried first — see
    /// [`When::context`].
    pub when: Option<When>,
    pub help: &'static str,
    pub keys: Keys,
}

/// What a line's key column is.
#[derive(PartialEq, Debug)]
pub enum Keys {
    /// The names the line binds, each to its action.
    Bound(&'static [Bound]),
    /// A name bound on another line, whose chords this line only
    /// describes: what they do under this line's condition.
    Also(&'static str),
    /// A name's chords held while the pointer drags: the line describes
    /// what the drag does while they are.
    Gesture(&'static str),
}

/// A name, the action it runs, and the chords it answers to until the
/// configuration file says otherwise.
#[derive(PartialEq, Debug)]
pub struct Bound {
    pub name: &'static str,
    pub action: Action,
    pub defaults: &'static [Chord],
}

impl Row {
    /// The names this line binds; none for a line that only describes.
    fn binds(&self) -> &'static [Bound] {
        match self.keys {
            Keys::Bound(binds) => binds,
            Keys::Also(_) | Keys::Gesture(_) => &[],
        }
    }

    /// The context this line's names are in, if any.
    fn context(&self) -> Option<Context> {
        self.when.and_then(When::context)
    }
}

/// The chords each name answers to, as the table has them or the
/// configuration file set them.
#[derive(Clone, PartialEq, Debug)]
pub struct Keymap {
    rows: &'static [Row],
    /// Indexed as the names come in the table: see [`Keymap::binds`].
    chords: Vec<Vec<Chord>>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::new(super::input::ROWS)
    }
}

impl Keymap {
    /// Every name of `rows` at its defaults.
    pub fn new(rows: &'static [Row]) -> Self {
        let chords = rows
            .iter()
            .flat_map(Row::binds)
            .map(|bound| bound.defaults.to_vec())
            .collect();
        Self { rows, chords }
    }

    /// Every line of the table.
    pub fn rows(&self) -> &'static [Row] {
        self.rows
    }

    /// Every name, with the line it is on, in the order `chords` is.
    fn binds(&self) -> impl Iterator<Item = (&'static Row, &'static Bound)> + Clone + use<> {
        let rows = self.rows;
        rows.iter()
            .flat_map(|row| row.binds().iter().map(move |bound| (row, bound)))
    }

    fn index_of(&self, name: &str) -> Option<usize> {
        self.binds().position(|(_, bound)| bound.name == name)
    }

    /// Makes `chords` the whole of what `name` answers to, and takes each
    /// of them from whichever other name in the same context held it.
    /// Returns the names it took chords from; an unknown name is refused.
    pub fn bind(&mut self, name: &str, chords: Vec<Chord>) -> Result<Vec<&'static str>, String> {
        let index = self
            .index_of(name)
            .ok_or_else(|| format!("unknown key name `{name}`"))?;
        let context = self.binds().nth(index).and_then(|(row, _)| row.context());
        let mut unique = Vec::new();
        for chord in chords {
            if !unique.contains(&chord) {
                unique.push(chord);
            }
        }
        let mut displaced = Vec::new();
        for (other, (row, bound)) in self.binds().enumerate() {
            if other == index || row.context() != context {
                continue;
            }
            let held = &mut self.chords[other];
            let before = held.len();
            held.retain(|chord| !unique.contains(chord));
            if held.len() != before && !displaced.contains(&bound.name) {
                displaced.push(bound.name);
            }
        }
        self.chords[index] = unique;
        Ok(displaced)
    }

    /// `name` as the table spells it, where it is a name: the one that
    /// lives as long as the table does.
    pub fn name_of(&self, name: &str) -> Option<&'static str> {
        self.binds()
            .find(|(_, bound)| bound.name == name)
            .map(|(_, bound)| bound.name)
    }

    /// The action `name` runs, where it is a name.
    pub fn action_named(&self, name: &str) -> Option<Action> {
        self.binds()
            .find(|(_, bound)| bound.name == name)
            .map(|(_, bound)| bound.action)
    }

    /// What the line `name` is on says it does.
    pub fn help_of(&self, name: &str) -> Option<&'static str> {
        self.binds()
            .find(|(_, bound)| bound.name == name)
            .map(|(row, _)| row.help)
    }

    /// The name in `context` that holds `chord`, as its place in `chords`.
    fn holder(&self, chord: Chord, context: Option<Context>) -> Option<usize> {
        self.binds()
            .enumerate()
            .find(|(index, (row, _))| {
                row.context() == context && self.chords[*index].contains(&chord)
            })
            .map(|(index, _)| index)
    }

    /// What `key`, pressed at `position` and held with `mods`, asks for,
    /// with a region selected on the picture or not.
    ///
    /// The place on the keyboard is tried first, for the number row; then
    /// the named key or the character, Shift being part of a character and
    /// so not asked for again. A capital letter nothing binds is taken for
    /// its lower case, so that Caps Lock does not turn the keyboard off.
    /// Each is looked for in the contexts that hold, in [`ORDER`], and then
    /// among the plain names.
    pub fn action_for(
        &self,
        key: &Key,
        position: PhysicalKey,
        mods: Mods,
        region_selected: bool,
    ) -> Option<Action> {
        let mut candidates = Vec::with_capacity(2);
        if let PhysicalKey::Code(code) = position
            && digit(code).is_some()
        {
            candidates.push(Chord::new(mods, Position(code)));
        }
        match key {
            Key::Named(named) => candidates.push(Chord::new(mods, Named(*named))),
            Key::Character(text) => {
                let mut characters = text.chars();
                if let (Some(character), None) = (characters.next(), characters.next()) {
                    let mods = mods.difference(Mods::SHIFT);
                    let exact = Chord::new(mods, Char(character));
                    let held = |chord| {
                        self.chords
                            .iter()
                            .any(|chords: &Vec<Chord>| chords.contains(&chord))
                    };
                    candidates.push(match character.is_ascii_uppercase() && !held(exact) {
                        true => Chord::new(mods, Char(character.to_ascii_lowercase())),
                        false => exact,
                    });
                }
            }
            _ => {}
        }
        let contexts = ORDER
            .iter()
            .filter(|context| match context {
                Context::Region => region_selected,
            })
            .map(|context| Some(*context))
            .chain([None]);
        let actions: Vec<Action> = self.binds().map(|(_, bound)| bound.action).collect();
        candidates.into_iter().find_map(|chord| {
            contexts
                .clone()
                .find_map(|context| self.holder(chord, context))
                .map(|index| actions[index])
        })
    }

    /// The chords `name` answers to.
    pub fn chords_of(&self, name: &str) -> &[Chord] {
        self.index_of(name)
            .map_or(&[], |index| self.chords[index].as_slice())
    }

    /// The chords `name` answers to, as people read them: empty where it
    /// answers to none.
    pub fn spelled(&self, name: &str) -> String {
        spell_all(self.chords_of(name))
    }

    /// The key column of `row`, as `--help` and the help popup write it.
    pub fn column(&self, row: &Row) -> String {
        match row.keys {
            Keys::Bound(binds) => {
                let chords: Vec<Chord> = binds
                    .iter()
                    .flat_map(|bound| self.chords_of(bound.name).iter().copied())
                    .collect();
                spell_all(&chords)
            }
            Keys::Also(name) => self.spelled(name),
            Keys::Gesture(name) => self
                .chords_of(name)
                .iter()
                .map(|chord| format!("{}+Drag", chord.spell()))
                .collect::<Vec<_>>()
                .join(", "),
        }
    }

    /// The line that binds `action`, and the name on it that does; a line
    /// that only describes never answers.
    pub fn bound_for(&self, action: Action) -> Option<(&'static Row, &'static str)> {
        self.binds()
            .find(|(_, bound)| bound.action == action)
            .map(|(row, bound)| (row, bound.name))
    }

    /// The line that binds `action`.
    pub fn row_for(&self, action: Action) -> Option<&'static Row> {
        self.bound_for(action).map(|(row, _)| row)
    }

    /// Every name at its chords, commented out, under its section and what
    /// its line says it does, as `--print-config` lists them.
    pub fn template(&self) -> String {
        let mut text = String::from(
            "\n# Keys: each keys.<name> takes its chords, whitespace between them; with\n\
             # none the name is unbound. A chord is modifiers joined by + and then a key:\n\
             # ctrl, alt, shift, super; the key one character as the keyboard types it\n\
             # (> rather than shift+., L for Shift+L), a digit of the number row by its\n\
             # place (shift+2), or space, enter, esc, tab, backspace, delete, insert,\n\
             # home, end, pageup, pagedown, left, right, up, down, f1 to f12. # starts\n\
             # a comment, so the key that types it is written by its place: shift+3.\n\
             # A chord set here is taken from whichever other name held it. With a\n\
             # region selected the region's names are tried first, then the rest.\n",
        );
        for section in Section::ALL {
            let _ = writeln!(text, "\n# {}", section.title());
            for row in self.rows.iter().filter(|row| row.section == section) {
                let Keys::Bound(binds) = row.keys else {
                    continue;
                };
                let _ = writeln!(text, "\n# {}", row.help);
                if row.context() == Some(Context::Region) {
                    let _ = writeln!(text, "# Tried first while a region is selected.");
                }
                for bound in binds {
                    let tokens: Vec<String> = self
                        .chords_of(bound.name)
                        .iter()
                        .map(Chord::token)
                        .collect();
                    let _ = writeln!(text, "# keys.{} = {}", bound.name, tokens.join(" "));
                    for describing in self.rows.iter() {
                        match describing.keys {
                            Keys::Also(name) if name == bound.name => {
                                let when = describing.when.map_or("", When::describe);
                                let _ = writeln!(text, "#   With {when}: {}", describing.help);
                            }
                            Keys::Gesture(name) if name == bound.name => {
                                let _ = writeln!(text, "#   Held, a drag: {}", describing.help);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::input::ROWS;
    use winit::keyboard::SmolStr;

    const PLAIN: Mods = Mods::empty();
    const CTRL: Mods = Mods::CONTROL;
    const SHIFT: Mods = Mods::SHIFT;

    fn chord(token: &str) -> Chord {
        Chord::read(token).unwrap_or_else(|error| panic!("{token}: {error}"))
    }

    #[test]
    fn a_chord_reads_as_the_file_writes_it() {
        assert_eq!(chord("m"), Chord::new(PLAIN, Char('m')));
        assert_eq!(chord("L"), Chord::new(PLAIN, Char('L')));
        assert_eq!(chord("shift+l"), Chord::new(PLAIN, Char('L')));
        assert_eq!(chord("ctrl+shift+c"), Chord::new(CTRL, Char('C')));
        assert_eq!(chord("Control+z"), Chord::new(CTRL, Char('z')));
        assert_eq!(chord("ctrl+>"), Chord::new(CTRL, Char('>')));
        assert_eq!(chord("+"), Chord::new(PLAIN, Char('+')));
        assert_eq!(chord("ctrl++"), Chord::new(CTRL, Char('+')));
        assert_eq!(
            chord("shift+2"),
            Chord::new(SHIFT, Position(KeyCode::Digit2))
        );
        assert_eq!(chord("0"), Chord::new(PLAIN, Position(KeyCode::Digit0)));
        assert_eq!(chord("PageUp"), Chord::new(PLAIN, Named(NamedKey::PageUp)));
        assert_eq!(chord("return"), Chord::new(PLAIN, Named(NamedKey::Enter)));
        assert_eq!(chord("del"), Chord::new(PLAIN, Named(NamedKey::Delete)));
        assert_eq!(
            chord("ctrl+shift+left"),
            Chord::new(CTRL | SHIFT, Named(NamedKey::ArrowLeft))
        );
        assert_eq!(chord("f12"), Chord::new(PLAIN, Named(NamedKey::F12)));

        for bad in [
            "", "ctrl+", "shift+.", "shift+#", "hyper+a", "pgup", "f13", "ab",
        ] {
            assert!(Chord::read(bad).is_err(), "{bad:?} reads");
        }
        let error = Chord::read("shift+.").unwrap_err();
        assert!(error.contains("write the character Shift types"), "{error}");
    }

    #[test]
    fn a_chord_is_spelled_for_people() {
        let spelled = |token| chord(token).spell();
        assert_eq!(spelled("ctrl+z"), "Ctrl+Z");
        assert_eq!(spelled("m"), "m");
        assert_eq!(spelled("L"), "Shift+L");
        assert_eq!(spelled("ctrl+shift+c"), "Ctrl+Shift+C");
        assert_eq!(spelled("ctrl+>"), "Ctrl+>");
        assert_eq!(spelled("shift+2"), "Shift+2");
        assert_eq!(spelled("pageup"), "Page Up");
        assert_eq!(spelled("backspace"), "\u{232b}");
        assert_eq!(spelled("delete"), "Del");
        assert_eq!(spelled("f2"), "F2");
        assert_eq!(spelled("alt+["), "Alt+[");
    }

    #[test]
    fn several_chords_are_spelled_as_one_cell() {
        let all = |tokens: &[&str]| spell_all(&tokens.iter().map(|t| chord(t)).collect::<Vec<_>>());
        assert_eq!(
            all(&[
                "ctrl+shift+left",
                "ctrl+shift+right",
                "ctrl+shift+up",
                "ctrl+shift+down"
            ]),
            "Ctrl+Shift+Arrows"
        );
        assert_eq!(all(&["left", "right", "up", "down"]), "Arrows");
        assert_eq!(all(&["shift+2", "shift+3", "shift+4"]), "Shift+2, 3, 4");
        assert_eq!(all(&["1", "0"]), "1, 0");
        assert_eq!(all(&["alt+[", "alt+pageup"]), "Alt+[, Alt+Page Up");
        assert_eq!(all(&["A", "S"]), "Shift+A, Shift+S");
        assert_eq!(all(&[";", "'"]), ";, '");
        assert_eq!(all(&["left", "right"]), "Left, Right");
        assert_eq!(all(&[]), "");
    }

    /// Every chord of the table reads back from the way the file writes it.
    #[test]
    fn every_default_chord_reads_back_from_its_token() {
        for row in ROWS {
            for bound in row.binds() {
                for chord in bound.defaults {
                    assert_eq!(Chord::read(&chord.token()), Ok(*chord), "{}", bound.name);
                }
            }
        }
    }

    /// A small table of its own, for the rules of binding and dispatch.
    static TABLE: &[Row] = &[
        Row {
            section: Section::Zoom,
            when: None,
            help: "zoom in",
            keys: Keys::Bound(&[Bound {
                name: "zoom.in",
                action: Action::ZoomIn,
                defaults: &[Chord::new(PLAIN, Char('+'))],
            }]),
        },
        Row {
            section: Section::Zoom,
            when: None,
            help: "zoom out, and pan",
            keys: Keys::Bound(&[
                Bound {
                    name: "zoom.out",
                    action: Action::ZoomOut,
                    defaults: &[Chord::new(PLAIN, Char('-'))],
                },
                Bound {
                    name: "pan.left",
                    action: Action::Pan(
                        super::super::input::Direction::Left,
                        super::super::input::PanStep::Coarse,
                    ),
                    defaults: &[Chord::new(PLAIN, Named(NamedKey::ArrowLeft))],
                },
                Bound {
                    name: "interface.minimap",
                    action: Action::ToggleMinimap,
                    defaults: &[Chord::new(PLAIN, Char('m'))],
                },
                Bound {
                    name: "clipboard.path",
                    action: Action::CopyPath,
                    defaults: &[Chord::new(PLAIN, Char('C'))],
                },
                Bound {
                    name: "clipboard.name",
                    action: Action::CopyName,
                    defaults: &[Chord::new(PLAIN, Char('c'))],
                },
            ]),
        },
        Row {
            section: Section::Region,
            when: Some(When::RegionSelected),
            help: "move the region",
            keys: Keys::Bound(&[Bound {
                name: "region.move.left",
                action: Action::MoveRegion(super::super::input::Direction::Left),
                defaults: &[Chord::new(PLAIN, Named(NamedKey::ArrowLeft))],
            }]),
        },
        Row {
            section: Section::Region,
            when: Some(When::RegionSelected),
            help: "zoom in with a region",
            keys: Keys::Also("zoom.in"),
        },
    ];

    fn press(keymap: &Keymap, key: Key, region: bool) -> Option<Action> {
        keymap.action_for(&key, PhysicalKey::Code(KeyCode::F13), PLAIN, region)
    }

    fn typed(text: &str) -> Key {
        Key::Character(SmolStr::new(text))
    }

    #[test]
    fn binding_a_name_sets_its_chords_and_takes_them_from_others_in_its_context() {
        let mut keymap = Keymap::new(TABLE);
        assert!(keymap.bind("zoom.nope", vec![]).is_err());

        // The chord leaves the name that held it, in the same context only.
        assert_eq!(
            keymap.bind("zoom.in", vec![chord("-"), chord("left")]),
            Ok(vec!["zoom.out", "pan.left"])
        );
        assert_eq!(keymap.chords_of("zoom.out"), []);
        assert_eq!(keymap.chords_of("pan.left"), []);
        assert_eq!(keymap.chords_of("region.move.left"), [chord("left")]);
        assert_eq!(press(&keymap, typed("-"), false), Some(Action::ZoomIn));

        // An empty value unbinds.
        assert_eq!(keymap.bind("zoom.in", vec![]), Ok(vec![]));
        assert_eq!(press(&keymap, typed("+"), false), None);
        assert_eq!(keymap.spelled("zoom.in"), "");
    }

    #[test]
    fn a_region_name_is_tried_first_while_a_region_is_selected() {
        let mut keymap = Keymap::new(TABLE);
        let left = || Key::Named(NamedKey::ArrowLeft);
        let pan = Action::Pan(
            super::super::input::Direction::Left,
            super::super::input::PanStep::Coarse,
        );
        let moves = Action::MoveRegion(super::super::input::Direction::Left);
        assert_eq!(press(&keymap, left(), false), Some(pan));
        assert_eq!(press(&keymap, left(), true), Some(moves));
        // Moved to another key, the region's name leaves Left panning under
        // a region, and takes its own key.
        keymap.bind("region.move.left", vec![chord("h")]).unwrap();
        assert_eq!(press(&keymap, left(), true), Some(pan));
        assert_eq!(press(&keymap, typed("h"), true), Some(moves));
        assert_eq!(press(&keymap, typed("h"), false), None);
    }

    #[test]
    fn a_capital_nothing_binds_answers_as_its_lower_case() {
        let keymap = Keymap::new(TABLE);
        assert_eq!(
            press(&keymap, typed("M"), false),
            Some(Action::ToggleMinimap)
        );
        assert_eq!(press(&keymap, typed("C"), false), Some(Action::CopyPath));
        assert_eq!(press(&keymap, typed("c"), false), Some(Action::CopyName));
    }

    #[test]
    fn the_columns_are_spelled_from_the_chords_in_force() {
        let mut keymap = Keymap::new(TABLE);
        let also = &TABLE[3];
        assert_eq!(keymap.column(also), "+");
        keymap.bind("zoom.in", vec![chord("ctrl+i")]).unwrap();
        assert_eq!(keymap.column(also), "Ctrl+I");
        assert_eq!(keymap.column(&TABLE[1]), "-, Left, m, Shift+C, c");
        // A line that only describes never answers for its action.
        assert_eq!(
            keymap.row_for(Action::ZoomIn).map(|row| row.help),
            Some("zoom in")
        );
    }

    /// The template, read back line by line, is the default keymap.
    #[test]
    fn the_template_reads_back_as_the_defaults() {
        let defaults = Keymap::default();
        let mut read = Keymap::default();
        for name in read
            .binds()
            .map(|(_, bound)| bound.name)
            .collect::<Vec<_>>()
        {
            read.bind(name, Vec::new()).unwrap();
        }
        let template = defaults.template();
        let mut names = 0;
        for line in template
            .lines()
            .filter_map(|line| line.strip_prefix("# keys."))
        {
            let (name, value) = line
                .split_once(" = ")
                .unwrap_or((line.trim_end_matches(" ="), ""));
            let chords = value.split_whitespace().map(chord).collect();
            read.bind(name, chords).unwrap();
            names += 1;
        }
        assert_eq!(names, defaults.binds().count());
        assert_eq!(read, defaults);
    }

    /// No chord has two holders in one context, and none of the table's
    /// characters asks for Shift, which the character carries.
    #[test]
    fn the_table_binds_each_chord_once_in_each_context() {
        let keymap = Keymap::default();
        let mut seen: Vec<(Chord, Option<Context>, &str)> = Vec::new();
        for (row, bound) in keymap.binds() {
            for chord in bound.defaults {
                if let Some((.., first)) = seen
                    .iter()
                    .find(|(each, context, _)| each == chord && *context == row.context())
                {
                    panic!("{chord:?} is both {first} and {}", bound.name);
                }
                seen.push((*chord, row.context(), bound.name));
                if let Char(_) = chord.key {
                    assert!(!chord.mods.shift_key(), "{} asks for Shift", bound.name);
                }
            }
        }
    }

    /// Every name is unique, lowercase and dotted, and starts with the word
    /// of the section it is listed under; every name a line describes is a
    /// name the table binds.
    #[test]
    fn every_name_is_unique_dotted_and_in_its_section() {
        let keymap = Keymap::default();
        let mut names = Vec::new();
        for (row, bound) in keymap.binds() {
            let name = bound.name;
            assert!(!names.contains(&name), "{name} twice");
            names.push(name);
            assert!(name.contains('.'), "{name}");
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-'),
                "{name}"
            );
            let first = name.split('.').next().unwrap_or_default();
            let words: &[&str] = match row.section {
                Section::Zoom => &["zoom", "pan"],
                Section::Interface => &["interface"],
                Section::Files => &["files"],
                Section::Region => &["region"],
                Section::Clipboard => &["clipboard"],
                Section::Display => &["display"],
                Section::Playback => &["playback"],
            };
            assert!(words.contains(&first), "{name} under {:?}", row.section);
        }
        assert_eq!(names.len(), 90);
        for row in keymap.rows() {
            if let Keys::Also(name) | Keys::Gesture(name) = row.keys {
                assert!(names.contains(&name), "{name} is described but not bound");
            }
        }
    }
}
