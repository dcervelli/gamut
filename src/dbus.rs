//! The session bus, spoken directly: enough of D-Bus to call a method on
//! the desktop's portal and wait for the signal it answers with.
//!
//! Written here rather than taken from a crate for the reason the clipboard
//! and the monitors are: the whole of what is needed is one connection, a
//! handful of calls and one signal, and the libraries that do it bring an
//! async runtime and forty crates with them. What is here is the wire
//! format — [`Value`] marshaled and unmarshaled by signature, a message's
//! header and body — and a [`Connection`] that authenticates, says hello,
//! and reads messages one at a time, blocking. Nothing here is asynchronous
//! on purpose: the one caller is a thread of its own that has nothing to do
//! but wait.
//!
//! The protocol is the D-Bus specification's: every message is a fixed
//! twelve-byte head, an array of header fields, padding to eight, and a
//! body whose layout is given by the `SIGNATURE` field. Every value is
//! aligned to its own size within the message, arrays carry their byte
//! length ahead of their elements, and a variant carries its signature
//! ahead of its value. Only little-endian messages are sent; both orders
//! are read, since the bus writes back in whichever it was written in.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

use anyhow::{Context, Result, bail};

/// Where the bus's own methods live: the destination and object of `Hello`
/// and `AddMatch`.
const BUS_NAME: &str = "org.freedesktop.DBus";
const BUS_PATH: &str = "/org/freedesktop/DBus";

/// The message types this speaks and reads.
const METHOD_CALL: u8 = 1;
const METHOD_RETURN: u8 = 2;
const ERROR: u8 = 3;
const SIGNAL: u8 = 4;

/// The header field codes, by the specification's numbering.
const FIELD_PATH: u8 = 1;
const FIELD_INTERFACE: u8 = 2;
const FIELD_MEMBER: u8 = 3;
const FIELD_ERROR_NAME: u8 = 4;
const FIELD_REPLY_SERIAL: u8 = 5;
const FIELD_DESTINATION: u8 = 6;
const FIELD_SENDER: u8 = 7;
const FIELD_SIGNATURE: u8 = 8;

/// The most a message may be, by the specification. A body claiming more
/// is a broken peer rather than a large message, and is refused before
/// anything is allocated for it.
const MAX_MESSAGE: usize = 128 * 1024 * 1024;

/// One D-Bus value, of any of the types the wire format has.
///
/// An array carries the signature of its element as well as its items, so
/// that an empty one can still be written with a type: `a{sv}` with nothing
/// in it is still an `a{sv}`.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Byte(u8),
    Bool(bool),
    I16(i16),
    U16(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    Double(f64),
    Str(String),
    ObjectPath(String),
    Signature(String),
    /// A file descriptor's index into the message's out-of-band list, which
    /// nothing here asks for; read as its number so that a message carrying
    /// one still parses.
    Fd(u32),
    Variant(Box<Value>),
    /// `element` is the signature of one element, `items` the elements.
    Array {
        element: String,
        items: Vec<Value>,
    },
    Struct(Vec<Value>),
    DictEntry(Box<Value>, Box<Value>),
}

impl Value {
    /// A dictionary of string keys to variants — `a{sv}` — which is the shape
    /// of every options argument the portals take.
    pub fn dict(entries: Vec<(&str, Value)>) -> Value {
        Value::Array {
            element: "{sv}".to_string(),
            items: entries
                .into_iter()
                .map(|(key, value)| {
                    Value::DictEntry(
                        Box::new(Value::Str(key.to_string())),
                        Box::new(Value::Variant(Box::new(value))),
                    )
                })
                .collect(),
        }
    }

    /// The signature of this value's type.
    pub fn signature(&self) -> String {
        match self {
            Value::Byte(_) => "y".into(),
            Value::Bool(_) => "b".into(),
            Value::I16(_) => "n".into(),
            Value::U16(_) => "q".into(),
            Value::I32(_) => "i".into(),
            Value::U32(_) => "u".into(),
            Value::I64(_) => "x".into(),
            Value::U64(_) => "t".into(),
            Value::Double(_) => "d".into(),
            Value::Str(_) => "s".into(),
            Value::ObjectPath(_) => "o".into(),
            Value::Signature(_) => "g".into(),
            Value::Fd(_) => "h".into(),
            Value::Variant(_) => "v".into(),
            Value::Array { element, .. } => format!("a{element}"),
            Value::Struct(fields) => {
                let inner: String = fields.iter().map(Value::signature).collect();
                format!("({inner})")
            }
            Value::DictEntry(key, value) => {
                format!("{{{}{}}}", key.signature(), value.signature())
            }
        }
    }

    /// The string this is, for reading a reply, or `None` for anything else.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(text) | Value::ObjectPath(text) | Value::Signature(text) => Some(text),
            _ => None,
        }
    }

    /// The items of the array this is, or `None` for anything else.
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array { items, .. } => Some(items),
            _ => None,
        }
    }

    /// The value under `key` in the dictionary this is, unwrapped from its
    /// variant, or `None` where this is no dictionary or the key is absent.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_array()?.iter().find_map(|entry| match entry {
            Value::DictEntry(held, value) if held.as_str() == Some(key) => match &**value {
                Value::Variant(inner) => Some(&**inner),
                other => Some(other),
            },
            _ => None,
        })
    }
}

/// The alignment of the first byte of a value of the type `signature`
/// opens with.
fn alignment(signature: &[u8]) -> usize {
    match signature.first() {
        Some(b'y' | b'g' | b'v') => 1,
        Some(b'n' | b'q') => 2,
        Some(b'b' | b'i' | b'u' | b's' | b'o' | b'h' | b'a') => 4,
        Some(b'x' | b't' | b'd' | b'(' | b'{') => 8,
        _ => 1,
    }
}

/// How many bytes of `signature` make up its first complete type: one for
/// a basic type, the element's after the `a` for an array, and everything
/// to the matching bracket for a struct or a dict entry.
fn complete_type(signature: &[u8]) -> Result<usize> {
    match signature.first() {
        None => bail!("a signature ended where a type was expected"),
        Some(b'a') => Ok(1 + complete_type(&signature[1..])?),
        Some(open @ (b'(' | b'{')) => {
            let close = if *open == b'(' { b')' } else { b'}' };
            let mut depth = 0usize;
            for (index, byte) in signature.iter().enumerate() {
                match byte {
                    b'(' | b'{' => depth += 1,
                    b')' | b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            if *byte != close {
                                bail!("a signature closed a bracket it did not open");
                            }
                            return Ok(index + 1);
                        }
                    }
                    _ => {}
                }
            }
            bail!("a signature opened a bracket it did not close")
        }
        Some(_) => Ok(1),
    }
}

/// Bytes being written in wire order: little-endian, each value padded to
/// its own alignment counted from the start of the message.
struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn pad(&mut self, alignment: usize) {
        while !self.bytes.len().is_multiple_of(alignment) {
            self.bytes.push(0);
        }
    }

    fn u32(&mut self, value: u32) {
        self.pad(4);
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// A string as `s` and `o` are written: its length, its bytes and a
    /// terminating NUL the length does not count.
    fn string(&mut self, text: &str) {
        self.u32(text.len() as u32);
        self.bytes.extend_from_slice(text.as_bytes());
        self.bytes.push(0);
    }

    /// A signature, whose length is a byte rather than four.
    fn signature(&mut self, text: &str) {
        self.bytes.push(text.len() as u8);
        self.bytes.extend_from_slice(text.as_bytes());
        self.bytes.push(0);
    }

    fn value(&mut self, value: &Value) {
        match value {
            Value::Byte(byte) => self.bytes.push(*byte),
            Value::Bool(flag) => self.u32(u32::from(*flag)),
            Value::I16(number) => {
                self.pad(2);
                self.bytes.extend_from_slice(&number.to_le_bytes());
            }
            Value::U16(number) => {
                self.pad(2);
                self.bytes.extend_from_slice(&number.to_le_bytes());
            }
            Value::I32(number) => {
                self.pad(4);
                self.bytes.extend_from_slice(&number.to_le_bytes());
            }
            Value::U32(number) | Value::Fd(number) => self.u32(*number),
            Value::I64(number) => {
                self.pad(8);
                self.bytes.extend_from_slice(&number.to_le_bytes());
            }
            Value::U64(number) => {
                self.pad(8);
                self.bytes.extend_from_slice(&number.to_le_bytes());
            }
            Value::Double(number) => {
                self.pad(8);
                self.bytes.extend_from_slice(&number.to_le_bytes());
            }
            Value::Str(text) | Value::ObjectPath(text) => self.string(text),
            Value::Signature(text) => self.signature(text),
            Value::Variant(inner) => {
                self.signature(&inner.signature());
                self.value(inner);
            }
            // The length is the elements' bytes alone: it does not count the
            // padding between itself and the first of them.
            Value::Array { element, items } => {
                self.u32(0);
                let length_at = self.bytes.len() - 4;
                self.pad(alignment(element.as_bytes()));
                let start = self.bytes.len();
                for item in items {
                    self.value(item);
                }
                let length = (self.bytes.len() - start) as u32;
                self.bytes[length_at..length_at + 4].copy_from_slice(&length.to_le_bytes());
            }
            Value::Struct(fields) => {
                self.pad(8);
                for field in fields {
                    self.value(field);
                }
            }
            Value::DictEntry(key, value) => {
                self.pad(8);
                self.value(key);
                self.value(value);
            }
        }
    }
}

/// Bytes being read in wire order, in whichever byte order the message
/// said. `at` is the offset from the start of the message, which is what
/// the alignment is counted from.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    little: bool,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], little: bool) -> Self {
        Self {
            bytes,
            at: 0,
            little,
        }
    }

    fn pad(&mut self, alignment: usize) -> Result<()> {
        while !self.at.is_multiple_of(alignment) {
            self.at += 1;
        }
        if self.at > self.bytes.len() {
            bail!("a message ended inside its padding");
        }
        Ok(())
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| anyhow::anyhow!("a message ended inside a value"))?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }

    /// A fixed-size number, from its own bytes in the message's order.
    fn number<const N: usize>(&mut self) -> Result<[u8; N]> {
        self.pad(N)?;
        let mut bytes = [0u8; N];
        bytes.copy_from_slice(self.take(N)?);
        if !self.little {
            bytes.reverse();
        }
        Ok(bytes)
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.number()?))
    }

    /// A string's bytes, checked to be UTF-8 and followed by the NUL the
    /// wire format requires.
    fn text(&mut self, length: usize) -> Result<String> {
        let bytes = self.take(length)?;
        let text = std::str::from_utf8(bytes).context("a string that was not UTF-8")?;
        if self.take(1)? != [0] {
            bail!("a string was not NUL-terminated");
        }
        Ok(text.to_string())
    }

    fn string(&mut self) -> Result<String> {
        let length = self.u32()? as usize;
        self.text(length)
    }

    fn signature(&mut self) -> Result<String> {
        let length = usize::from(self.take(1)?[0]);
        self.text(length)
    }

    /// One value of the type `signature` opens with. Returns it with the
    /// signature of what came after, for the caller to go on reading.
    fn value<'s>(&mut self, signature: &'s [u8]) -> Result<(Value, &'s [u8])> {
        let taken = complete_type(signature)?;
        let (this, rest) = signature.split_at(taken);
        let value = match this[0] {
            b'y' => Value::Byte(self.take(1)?[0]),
            b'b' => match self.u32()? {
                0 => Value::Bool(false),
                1 => Value::Bool(true),
                other => bail!("a boolean was {other}"),
            },
            b'n' => Value::I16(i16::from_le_bytes(self.number()?)),
            b'q' => Value::U16(u16::from_le_bytes(self.number()?)),
            b'i' => Value::I32(i32::from_le_bytes(self.number()?)),
            b'u' => Value::U32(self.u32()?),
            b'h' => Value::Fd(self.u32()?),
            b'x' => Value::I64(i64::from_le_bytes(self.number()?)),
            b't' => Value::U64(u64::from_le_bytes(self.number()?)),
            b'd' => Value::Double(f64::from_le_bytes(self.number()?)),
            b's' => Value::Str(self.string()?),
            b'o' => Value::ObjectPath(self.string()?),
            b'g' => Value::Signature(self.signature()?),
            b'v' => {
                let inner = self.signature()?;
                let (value, rest) = self.value(inner.as_bytes())?;
                if !rest.is_empty() {
                    bail!("a variant held more than one value");
                }
                Value::Variant(Box::new(value))
            }
            b'a' => {
                let element = &this[1..];
                let length = self.u32()? as usize;
                self.pad(alignment(element))?;
                let end = self
                    .at
                    .checked_add(length)
                    .filter(|end| *end <= self.bytes.len())
                    .ok_or_else(|| anyhow::anyhow!("an array ran past the end of the message"))?;
                let mut items = Vec::new();
                while self.at < end {
                    let (item, rest) = self.value(element)?;
                    if !rest.is_empty() {
                        bail!("an array's element signature held more than one type");
                    }
                    items.push(item);
                }
                if self.at != end {
                    bail!("an array's elements ran past its length");
                }
                Value::Array {
                    element: String::from_utf8_lossy(element).into_owned(),
                    items,
                }
            }
            b'(' => {
                self.pad(8)?;
                let mut fields = Vec::new();
                let mut inner = &this[1..this.len() - 1];
                while !inner.is_empty() {
                    let (field, rest) = self.value(inner)?;
                    fields.push(field);
                    inner = rest;
                }
                Value::Struct(fields)
            }
            b'{' => {
                self.pad(8)?;
                let inner = &this[1..this.len() - 1];
                let (key, rest) = self.value(inner)?;
                let (value, rest) = self.value(rest)?;
                if !rest.is_empty() {
                    bail!("a dict entry held more than a key and a value");
                }
                Value::DictEntry(Box::new(key), Box::new(value))
            }
            other => bail!("unknown type code {:?} in a signature", char::from(other)),
        };
        Ok((value, rest))
    }

    /// Every value `signature` describes, in order.
    fn values(&mut self, mut signature: &[u8]) -> Result<Vec<Value>> {
        let mut values = Vec::new();
        while !signature.is_empty() {
            let (value, rest) = self.value(signature)?;
            values.push(value);
            signature = rest;
        }
        Ok(values)
    }
}

/// Writes `values` as a message body would carry them, and says what
/// signature describes them.
pub fn marshal(values: &[Value]) -> (String, Vec<u8>) {
    let mut writer = Writer::new();
    for value in values {
        writer.value(value);
    }
    (values.iter().map(Value::signature).collect(), writer.bytes)
}

/// Reads the values `signature` describes out of `bytes`, written in the
/// order `little` says: the body half of [`parse`], for the tests.
#[cfg(test)]
fn unmarshal(bytes: &[u8], signature: &str, little: bool) -> Result<Vec<Value>> {
    let mut reader = Reader::new(bytes, little);
    let values = reader.values(signature.as_bytes())?;
    if reader.at != bytes.len() {
        bail!("a message body was longer than its signature");
    }
    Ok(values)
}

/// A message read off the bus: its kind, the header fields that matter
/// here, and its body.
#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub kind: u8,
    pub serial: u32,
    pub path: Option<String>,
    pub interface: Option<String>,
    pub member: Option<String>,
    pub error_name: Option<String>,
    pub reply_serial: Option<u32>,
    pub sender: Option<String>,
    pub body: Vec<Value>,
}

impl Message {
    /// Whether this is the signal `member` of `interface` from the object
    /// at `path`.
    pub fn is_signal(&self, path: &str, interface: &str, member: &str) -> bool {
        self.kind == SIGNAL
            && self.path.as_deref() == Some(path)
            && self.interface.as_deref() == Some(interface)
            && self.member.as_deref() == Some(member)
    }
}

/// The whole of a method call, laid out for the wire: the head, the fields
/// and the body, padded as the specification asks.
fn method_call(
    serial: u32,
    destination: &str,
    path: &str,
    interface: &str,
    member: &str,
    body: &[Value],
) -> Vec<u8> {
    let (signature, body) = marshal(body);
    let mut fields = vec![
        Value::Struct(vec![
            Value::Byte(FIELD_PATH),
            Value::Variant(Box::new(Value::ObjectPath(path.to_string()))),
        ]),
        Value::Struct(vec![
            Value::Byte(FIELD_DESTINATION),
            Value::Variant(Box::new(Value::Str(destination.to_string()))),
        ]),
        Value::Struct(vec![
            Value::Byte(FIELD_INTERFACE),
            Value::Variant(Box::new(Value::Str(interface.to_string()))),
        ]),
        Value::Struct(vec![
            Value::Byte(FIELD_MEMBER),
            Value::Variant(Box::new(Value::Str(member.to_string()))),
        ]),
    ];
    if !signature.is_empty() {
        fields.push(Value::Struct(vec![
            Value::Byte(FIELD_SIGNATURE),
            Value::Variant(Box::new(Value::Signature(signature))),
        ]));
    }
    let mut writer = Writer::new();
    writer.bytes.extend_from_slice(&[b'l', METHOD_CALL, 0, 1]);
    writer.u32(body.len() as u32);
    writer.u32(serial);
    writer.value(&Value::Array {
        element: "(yv)".to_string(),
        items: fields,
    });
    writer.pad(8);
    writer.bytes.extend_from_slice(&body);
    writer.bytes
}

/// Takes a message apart: `head` is its first sixteen bytes, `rest` the
/// header fields, the padding and the body after them.
fn parse(head: &[u8; 16], rest: &[u8]) -> Result<Message> {
    let little = match head[0] {
        b'l' => true,
        b'B' => false,
        other => bail!("a message in an unknown byte order {other:#x}"),
    };
    let whole: Vec<u8> = head.iter().chain(rest).copied().collect();
    let mut reader = Reader::new(&whole, little);
    reader.at = 4;
    let body_length = reader.u32()? as usize;
    let serial = reader.u32()?;
    let (fields, _) = reader.value(b"a(yv)")?;
    reader.pad(8)?;
    let body_at = reader.at;
    if body_at + body_length != whole.len() {
        bail!("a message's body was not the length its head said");
    }

    let mut message = Message {
        kind: head[1],
        serial,
        path: None,
        interface: None,
        member: None,
        error_name: None,
        reply_serial: None,
        sender: None,
        body: Vec::new(),
    };
    let mut signature = String::new();
    for field in fields.as_array().unwrap_or_default() {
        let Value::Struct(pair) = field else {
            continue;
        };
        let (Some(Value::Byte(code)), Some(Value::Variant(value))) = (pair.first(), pair.get(1))
        else {
            continue;
        };
        let text = || value.as_str().map(str::to_string);
        match *code {
            FIELD_PATH => message.path = text(),
            FIELD_INTERFACE => message.interface = text(),
            FIELD_MEMBER => message.member = text(),
            FIELD_ERROR_NAME => message.error_name = text(),
            FIELD_REPLY_SERIAL => {
                if let Value::U32(reply) = **value {
                    message.reply_serial = Some(reply);
                }
            }
            FIELD_SENDER => message.sender = text(),
            FIELD_SIGNATURE => signature = text().unwrap_or_default(),
            _ => {}
        }
    }
    let mut body = Reader::new(&whole, little);
    body.at = body_at;
    message.body = body.values(signature.as_bytes())?;
    if body.at != whole.len() {
        bail!("a message body was longer than its signature");
    }
    Ok(message)
}

/// A connection to the session bus, authenticated and named.
pub struct Connection {
    stream: UnixStream,
    /// The serial the next message goes out under. Never zero, which the
    /// specification reserves.
    next_serial: u32,
    /// The unique name the bus gave this connection: `:1.42`.
    unique: String,
    /// Signals that arrived while a reply was being waited for, kept for
    /// whoever asks for them next. A portal may answer a request before it
    /// returns the request's handle, so the signal that answers it can be
    /// on the wire ahead of the reply to the call that made it.
    signals: VecDeque<Message>,
}

impl Connection {
    /// Connects to the session bus, authenticates as this user, and asks
    /// the bus for a name.
    pub fn session() -> Result<Self> {
        let stream = connect().context("connecting to the session bus")?;
        let mut connection = Self {
            stream,
            next_serial: 1,
            unique: String::new(),
            signals: VecDeque::new(),
        };
        connection
            .authenticate()
            .context("authenticating to the session bus")?;
        let reply = connection.call(BUS_NAME, BUS_PATH, BUS_NAME, "Hello", &[])?;
        connection.unique = reply
            .first()
            .and_then(Value::as_str)
            .map(str::to_string)
            .context("the session bus gave this connection no name")?;
        Ok(connection)
    }

    /// The name the bus knows this connection by.
    pub fn unique_name(&self) -> &str {
        &self.unique
    }

    /// The SASL exchange the bus opens with. `EXTERNAL` hands over the uid
    /// the socket's credentials already vouch for, which is the one
    /// mechanism every session bus accepts.
    fn authenticate(&mut self) -> Result<()> {
        // SAFETY: `getuid` takes nothing and cannot fail.
        let uid = unsafe { libc::getuid() };
        let hex: String = uid
            .to_string()
            .bytes()
            .map(|b| format!("{b:02x}"))
            .collect();
        self.stream.write_all(b"\0")?;
        self.stream
            .write_all(format!("AUTH EXTERNAL {hex}\r\n").as_bytes())?;
        let line = self.auth_line()?;
        if !line.starts_with("OK ") {
            bail!("the session bus answered `{}`", line.trim());
        }
        self.stream.write_all(b"BEGIN\r\n")?;
        Ok(())
    }

    /// One line of the authentication exchange, up to its CRLF.
    fn auth_line(&mut self) -> Result<String> {
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            self.stream.read_exact(&mut byte)?;
            line.push(byte[0]);
            if line.ends_with(b"\r\n") {
                return Ok(String::from_utf8_lossy(&line).into_owned());
            }
            if line.len() > 4096 {
                bail!("the session bus's greeting ran on past any reasonable length");
            }
        }
    }

    /// Calls `member` of `interface` on the object at `path` owned by
    /// `destination`, with `body` as its arguments, and returns the reply's
    /// values. An error reply is an error here, under the name the peer
    /// gave it.
    pub fn call(
        &mut self,
        destination: &str,
        path: &str,
        interface: &str,
        member: &str,
        body: &[Value],
    ) -> Result<Vec<Value>> {
        let serial = self.next_serial;
        self.next_serial = self.next_serial.wrapping_add(1).max(1);
        let bytes = method_call(serial, destination, path, interface, member, body);
        self.stream
            .write_all(&bytes)
            .with_context(|| format!("calling {interface}.{member}"))?;
        loop {
            let message = self.receive()?;
            if message.reply_serial != Some(serial) {
                if message.kind == SIGNAL {
                    self.signals.push_back(message);
                }
                continue;
            }
            return match message.kind {
                METHOD_RETURN => Ok(message.body),
                ERROR => {
                    let name = message.error_name.unwrap_or_default();
                    let said = message
                        .body
                        .first()
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    bail!("{interface}.{member} failed: {name}: {said}")
                }
                other => bail!("{interface}.{member} was answered with a message of type {other}"),
            };
        }
    }

    /// Asks the bus to deliver the messages `rule` describes.
    pub fn add_match(&mut self, rule: &str) -> Result<()> {
        self.call(
            BUS_NAME,
            BUS_PATH,
            BUS_NAME,
            "AddMatch",
            &[Value::Str(rule.to_string())],
        )?;
        Ok(())
    }

    /// Waits for the signal `member` of `interface` from the object at any
    /// of `paths`, and returns its body. Blocks for as long as it takes:
    /// the one caller is waiting on a dialog, which is up for as long as
    /// the user leaves it.
    pub fn wait_signal(
        &mut self,
        paths: &[&str],
        interface: &str,
        member: &str,
    ) -> Result<Vec<Value>> {
        let matches = |message: &Message| {
            paths
                .iter()
                .any(|path| message.is_signal(path, interface, member))
        };
        if let Some(at) = self.signals.iter().position(matches) {
            let message = self.signals.remove(at).expect("found above");
            return Ok(message.body);
        }
        loop {
            let message = self.receive()?;
            if matches(&message) {
                return Ok(message.body);
            }
        }
    }

    /// The next message off the wire, whole.
    fn receive(&mut self) -> Result<Message> {
        let mut head = [0u8; 16];
        self.stream
            .read_exact(&mut head)
            .context("reading from the session bus")?;
        let little = head[0] == b'l';
        let word = |at: usize| {
            let mut bytes = [head[at], head[at + 1], head[at + 2], head[at + 3]];
            if !little {
                bytes.reverse();
            }
            u32::from_le_bytes(bytes) as usize
        };
        let body_length = word(4);
        let fields_length = word(12);
        // The fields array starts at 12 and its elements at 16; the body
        // starts at the next multiple of eight past its end.
        let fields_end = 16 + fields_length;
        let body_at = fields_end.div_ceil(8) * 8;
        let total = body_at + body_length;
        if total > MAX_MESSAGE {
            bail!("the session bus sent a message of {total} bytes");
        }
        let mut rest = vec![0u8; total - 16];
        self.stream
            .read_exact(&mut rest)
            .context("reading from the session bus")?;
        parse(&head, &rest)
    }
}

/// Opens the socket the session bus is on: what `DBUS_SESSION_BUS_ADDRESS`
/// names, or the runtime directory's `bus` where nothing does.
fn connect() -> Result<UnixStream> {
    if let Some(address) = std::env::var_os("DBUS_SESSION_BUS_ADDRESS") {
        let address = address.to_string_lossy();
        for candidate in address.split(';') {
            if let Some(stream) = connect_to(candidate)? {
                return Ok(stream);
            }
        }
        bail!("no usable address in DBUS_SESSION_BUS_ADDRESS={address}");
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .context("neither DBUS_SESSION_BUS_ADDRESS nor XDG_RUNTIME_DIR is set")?;
    let path = std::path::PathBuf::from(runtime).join("bus");
    UnixStream::connect(&path).with_context(|| format!("connecting to {}", path.display()))
}

/// One address of the bus — `unix:path=/run/user/1000/bus`, or the
/// abstract form — as a stream, or `None` for a transport this does not
/// speak.
fn connect_to(address: &str) -> Result<Option<UnixStream>> {
    let Some(rest) = address.strip_prefix("unix:") else {
        return Ok(None);
    };
    for pair in rest.split(',') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        let value = unescape(value);
        match key {
            "path" => {
                let path = std::path::PathBuf::from(std::ffi::OsString::from(value));
                let stream = UnixStream::connect(&path)
                    .with_context(|| format!("connecting to {}", path.display()))?;
                return Ok(Some(stream));
            }
            #[cfg(any(target_os = "linux", target_os = "android"))]
            "abstract" => {
                use std::os::linux::net::SocketAddrExt;
                let socket = std::os::unix::net::SocketAddr::from_abstract_name(value.as_bytes())
                    .context("an abstract bus name too long for a socket")?;
                let stream = UnixStream::connect_addr(&socket)
                    .with_context(|| format!("connecting to abstract socket {value}"))?;
                return Ok(Some(stream));
            }
            _ => {}
        }
    }
    Ok(None)
}

/// A value of a bus address with its `%XX` escapes undone.
fn unescape(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let escaped = (bytes[at] == b'%' && at + 2 < bytes.len())
            .then(|| std::str::from_utf8(&bytes[at + 1..at + 3]).ok())
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                out.push(byte);
                at += 3;
            }
            None => {
                out.push(bytes[at]);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Everything written comes back the same by its own signature, in
    /// either byte order for the fixed-size numbers.
    #[test]
    fn values_survive_the_round_trip() {
        let options = Value::dict(vec![
            ("handle_token", Value::Str("gamut_1".into())),
            ("multiple", Value::Bool(true)),
            (
                "filters",
                Value::Array {
                    element: "(sa(us))".into(),
                    items: vec![Value::Struct(vec![
                        Value::Str("Images".into()),
                        Value::Array {
                            element: "(us)".into(),
                            items: vec![
                                Value::Struct(vec![Value::U32(0), Value::Str("*.png".into())]),
                                Value::Struct(vec![Value::U32(1), Value::Str("image/jpeg".into())]),
                            ],
                        },
                    ])],
                },
            ),
            (
                "empty",
                Value::Array {
                    element: "{sv}".into(),
                    items: vec![],
                },
            ),
        ]);
        let values = vec![
            Value::Str("".into()),
            Value::Byte(7),
            Value::I16(-2),
            Value::U16(3),
            Value::I32(-4),
            Value::U32(5),
            Value::I64(-6),
            Value::U64(7),
            Value::Double(0.5),
            Value::ObjectPath("/org/freedesktop/portal/desktop".into()),
            Value::Signature("a{sv}".into()),
            Value::Fd(0),
            options,
            Value::Variant(Box::new(Value::Struct(vec![Value::Byte(1), Value::U64(2)]))),
        ];
        let (signature, bytes) = marshal(&values);
        assert_eq!(signature, "synqiuxtdogha{sv}v");
        assert_eq!(unmarshal(&bytes, &signature, true).unwrap(), values);
    }

    /// An empty array is still an array of its type, and its length counts
    /// the elements' bytes alone — not the padding after the length.
    #[test]
    fn arrays_are_laid_out_as_the_specification_says() {
        let (signature, bytes) = marshal(&[Value::Array {
            element: "(us)".into(),
            items: vec![Value::Struct(vec![Value::U32(1), Value::Str("a".into())])],
        }]);
        assert_eq!(signature, "a(us)");
        // Length, four bytes of padding to the struct's eight, then the
        // element: a u32 and a string of one byte with its length and NUL.
        assert_eq!(bytes.len(), 4 + 4 + 4 + 4 + 1 + 1);
        assert_eq!(
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            10
        );

        let (signature, bytes) = marshal(&[Value::Array {
            element: "{sv}".into(),
            items: vec![],
        }]);
        assert_eq!(signature, "a{sv}");
        assert_eq!(bytes, vec![0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            unmarshal(&bytes, &signature, true).unwrap(),
            vec![Value::Array {
                element: "{sv}".into(),
                items: vec![],
            }]
        );
    }

    /// A message built here parses back to itself, and lays its head out
    /// as the bus expects: the byte order, the type, the version, the body
    /// length and the serial in their places.
    #[test]
    fn a_method_call_parses_back() {
        let bytes = method_call(
            7,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.FileChooser",
            "OpenFile",
            &[
                Value::Str("".into()),
                Value::Str("Open".into()),
                Value::dict(vec![("directory", Value::Bool(false))]),
            ],
        );
        assert_eq!(&bytes[..4], &[b'l', METHOD_CALL, 0, 1]);
        let mut head = [0u8; 16];
        head.copy_from_slice(&bytes[..16]);
        let message = parse(&head, &bytes[16..]).unwrap();
        assert_eq!(message.kind, METHOD_CALL);
        assert_eq!(message.serial, 7);
        assert_eq!(
            message.path.as_deref(),
            Some("/org/freedesktop/portal/desktop")
        );
        assert_eq!(
            message.interface.as_deref(),
            Some("org.freedesktop.portal.FileChooser")
        );
        assert_eq!(message.member.as_deref(), Some("OpenFile"));
        assert_eq!(message.body.len(), 3);
        assert_eq!(message.body[2].get("directory"), Some(&Value::Bool(false)));
        assert_eq!(message.body[2].get("multiple"), None);
    }

    /// The bytes a bus actually writes for a signal — the portal's
    /// `Response`, big-endian included — read as the values they carry.
    #[test]
    fn a_response_signal_reads_in_either_byte_order() {
        // Built little-endian by the writer, then transcribed to big-endian
        // by hand for the head and the numbers the test reads.
        let body = marshal(&[
            Value::U32(0),
            Value::dict(vec![(
                "uris",
                Value::Array {
                    element: "s".into(),
                    items: vec![Value::Str("file:///tmp/a.png".into())],
                },
            )]),
        ]);
        let fields = Value::Array {
            element: "(yv)".into(),
            items: vec![
                Value::Struct(vec![
                    Value::Byte(FIELD_PATH),
                    Value::Variant(Box::new(Value::ObjectPath("/request/1".into()))),
                ]),
                Value::Struct(vec![
                    Value::Byte(FIELD_INTERFACE),
                    Value::Variant(Box::new(Value::Str(
                        "org.freedesktop.portal.Request".into(),
                    ))),
                ]),
                Value::Struct(vec![
                    Value::Byte(FIELD_MEMBER),
                    Value::Variant(Box::new(Value::Str("Response".into()))),
                ]),
                Value::Struct(vec![
                    Value::Byte(FIELD_SIGNATURE),
                    Value::Variant(Box::new(Value::Signature(body.0.clone()))),
                ]),
            ],
        };
        let mut writer = Writer::new();
        writer.bytes.extend_from_slice(&[b'l', SIGNAL, 1, 1]);
        writer.u32(body.1.len() as u32);
        writer.u32(99);
        writer.value(&fields);
        writer.pad(8);
        writer.bytes.extend_from_slice(&body.1);
        let bytes = writer.bytes;

        let mut head = [0u8; 16];
        head.copy_from_slice(&bytes[..16]);
        let message = parse(&head, &bytes[16..]).unwrap();
        assert!(message.is_signal("/request/1", "org.freedesktop.portal.Request", "Response"));
        assert_eq!(message.body[0], Value::U32(0));
        let uris = message.body[1]
            .get("uris")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(uris[0].as_str(), Some("file:///tmp/a.png"));
    }

    /// A body cut short, a length past the end, or a signature with a
    /// bracket left open is refused rather than read off the end.
    #[test]
    fn malformed_messages_are_refused() {
        let (signature, bytes) = marshal(&[Value::Str("hello".into())]);
        assert!(unmarshal(&bytes[..bytes.len() - 2], &signature, true).is_err());
        let mut long = bytes.clone();
        long[0] = 200;
        assert!(unmarshal(&long, &signature, true).is_err());
        assert!(unmarshal(&bytes, "(s", true).is_err());
        assert!(unmarshal(&bytes, "a", true).is_err());
        assert!(unmarshal(&bytes, "ss", true).is_err());
        assert!(unmarshal(&bytes, "", true).is_err());
    }

    /// Against the real session bus, where there is one: the authentication,
    /// the greeting and a round trip through the bus's own methods. Ignored
    /// by default since a test machine need not have a bus.
    #[test]
    #[ignore = "needs a session bus"]
    fn talks_to_the_live_session_bus() {
        let mut bus = Connection::session().expect("a session bus");
        assert!(bus.unique_name().starts_with(':'), "{}", bus.unique_name());
        let names = bus
            .call(BUS_NAME, BUS_PATH, BUS_NAME, "ListNames", &[])
            .expect("ListNames answers");
        let names = names[0].as_array().expect("an array of names");
        assert!(
            names
                .iter()
                .any(|name| name.as_str() == Some(bus.unique_name())),
            "this connection is on the bus's own list"
        );
        // An error reply is an error here, named.
        let refused = bus
            .call(BUS_NAME, BUS_PATH, BUS_NAME, "NoSuchMethod", &[])
            .expect_err("an unknown method is refused");
        assert!(
            refused.to_string().contains("org.freedesktop.DBus.Error"),
            "{refused}"
        );
        // And the connection is still good afterwards.
        bus.add_match("type='signal',interface='org.freedesktop.portal.Request'")
            .expect("AddMatch answers");
    }

    #[test]
    fn a_bus_address_is_unescaped() {
        assert_eq!(unescape("/run/user/1000/bus"), "/run/user/1000/bus");
        assert_eq!(unescape("/tmp/a%20b%2c"), "/tmp/a b,");
        assert_eq!(unescape("%2"), "%2");
    }
}
