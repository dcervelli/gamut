//! What a file says about itself in XMP: the other metadata block, the one
//! the programs that edit and catalog photographs write to.
//!
//! EXIF is what the camera wrote. XMP is what everyone since has written —
//! the title, the caption, the keywords, the credit — as an RDF/XML packet
//! that every container carries in a place of its own: an `APP1` segment in
//! a JPEG, an `iTXt` chunk in a PNG, an `XMP ` chunk in a WebP, an `xml `
//! box in a JPEG XL, a `mime` item in a HEIF, and tag 700 of a TIFF's own
//! directory. A file that has only a title has, as often as not, only this
//! block — nothing in EXIF holds a title at all.
//!
//! A file that cannot be written to — a camera raw, or one its owner would
//! rather not touch — keeps its packet in a sidecar instead: a file beside
//! it holding the packet and nothing else, under the picture's own name with
//! `.xmp` in place of its extension, as the specification spells it, or
//! after it, as darktable does. [`sidecar`] finds one; a property it holds
//! is taken over the embedded packet's, since the sidecar is what was
//! written last, and the rest of the packet's properties stand.
//!
//! Nothing here reaches the decoders or the image. [`packet`] finds the
//! block by walking the container's headers, reading a packet whole and
//! seeking past everything else, and [`Xmp::parse`] takes the packet apart
//! into properties: the namespace each is in, its name there, and the words
//! it holds. Which of those the panel shows, and under what names, is
//! [`super::exif`]'s business, alongside the EXIF fields that say the same
//! things.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use roxmltree::{Document, Node};

use super::{directory, tiff};

/// How large a packet is allowed to be. A packet is a few kilobytes of text,
/// and one carrying an edit history runs to a few hundred; a container that
/// claims more than this for it is not believed, and what would have been
/// read is left where it is.
const MAX_PACKET: u64 = 4 << 20;

/// The namespace the packet's structure is written in: the elements that
/// group properties, and the lists a property's values are held in.
const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

/// Dublin Core, where the title, the description, the creator, the keywords
/// and the rights go.
pub const DC: &str = "http://purl.org/dc/elements/1.1/";

/// XMP's own basic schema, where the program that wrote the file names
/// itself.
pub const BASIC: &str = "http://ns.adobe.com/xap/1.0/";

/// The header the packet wears in a JPEG's `APP1` segment, where it shares
/// the marker with EXIF and is told apart by this.
const JPEG_HEADER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";

/// The keyword of the PNG text chunk the packet is written under.
const PNG_KEYWORD: &[u8] = b"XML:com.adobe.xmp";

/// The signature the JPEG XL container opens with: a box of that length
/// and type, holding the two bytes a bare codestream opens with.
const JXL_SIGNATURE: [u8; 12] = [
    0x00, 0x00, 0x00, 0x0c, b'J', b'X', b'L', b' ', 0x0d, 0x0a, 0x87, 0x0a,
];

/// One property of the packet: where it is filed, what it is called there,
/// and what it says. A property that holds a list — the creators, the
/// keywords — holds every item; one that holds the same words in several
/// languages holds the default only, that being the one to show where there
/// is no asking.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Property {
    pub namespace: String,
    pub name: String,
    pub values: Vec<String>,
}

/// A file's XMP, as properties. Empty when the file carries none, or carries
/// a packet that will not parse: what is missing is a caption, not a
/// picture, so nothing here is an error anything else has to handle.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Xmp {
    properties: Vec<Property>,
}

impl Xmp {
    /// Reads `path`'s packet out of whatever container the file is, and the
    /// sidecar beside it, or gives back nothing at all.
    pub fn read(path: &Path) -> Self {
        Self::read_with(path, packet(path).as_deref())
    }

    /// The same, for a caller that has the embedded packet already — the
    /// EXIF reader, which parsed the TIFF directory the packet sits in —
    /// or knows the file carries none.
    pub fn read_with(path: &Path, embedded: Option<&[u8]>) -> Self {
        let embedded = embedded.and_then(Self::parse).unwrap_or_default();
        match sidecar(path).and_then(|packet| Self::parse(&packet)) {
            Some(sidecar) => sidecar.over(embedded),
            None => embedded,
        }
    }

    /// `self`'s properties, and whichever of `under`'s it does not have.
    fn over(mut self, under: Self) -> Self {
        for property in under.properties {
            push(
                &mut self.properties,
                &property.namespace,
                &property.name,
                property.values,
            );
        }
        self
    }

    /// The properties in `packet`, which is the RDF/XML a container hands
    /// over, with or without the `<?xpacket?>` wrapper and the padding that
    /// comes after it. `None` for a packet that is not XML, or is XML with
    /// no RDF in it.
    pub fn parse(packet: &[u8]) -> Option<Self> {
        // A packet is UTF-8 by definition, and one that is not is still
        // mostly readable; the padding after it is sometimes nulls rather
        // than spaces, and either way is not XML.
        let text = String::from_utf8_lossy(packet);
        let text = text.trim_matches(|c: char| c.is_whitespace() || c == '\0');
        let document = Document::parse(text).ok()?;
        let rdf = document
            .descendants()
            .find(|node| node.has_tag_name((RDF, "RDF")))?;
        let mut properties = Vec::new();
        for description in rdf
            .children()
            .filter(|node| node.has_tag_name((RDF, "Description")))
        {
            // The compact form: a property with one plain value may be
            // written as an attribute of the description rather than as an
            // element under it, and the program that wrote the file chose.
            for attribute in description.attributes() {
                let Some(namespace) = attribute.namespace() else {
                    continue;
                };
                if namespace == RDF {
                    continue;
                }
                push(
                    &mut properties,
                    namespace,
                    attribute.name(),
                    vec![attribute.value().to_string()],
                );
            }
            for element in description.children().filter(Node::is_element) {
                let Some(namespace) = element.tag_name().namespace() else {
                    continue;
                };
                push(
                    &mut properties,
                    namespace,
                    element.tag_name().name(),
                    values(element),
                );
            }
        }
        Some(Self { properties })
    }

    /// What the property `name` in `namespace` says: every item of a list,
    /// the default of a set of translations, or the one value of a plain
    /// property. `None` where the packet does not hold it.
    pub fn property(&self, namespace: &str, name: &str) -> Option<&[String]> {
        self.properties
            .iter()
            .find(|property| property.namespace == namespace && property.name == name)
            .map(|property| property.values.as_slice())
    }
}

/// Keeps a property whose words are worth keeping: one with a value that is
/// not blank. The first statement of a property is the one that stands,
/// since a packet saying the same thing twice is a packet that was
/// written twice.
fn push(properties: &mut Vec<Property>, namespace: &str, name: &str, values: Vec<String>) {
    let values: Vec<String> = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    if values.is_empty()
        || properties
            .iter()
            .any(|property| property.namespace == namespace && property.name == name)
    {
        return;
    }
    properties.push(Property {
        namespace: namespace.to_string(),
        name: name.to_string(),
        values,
    });
}

/// What a property element holds, in the three shapes XMP writes: words
/// straight in the element; a list of items, each an `rdf:li`, under an
/// `rdf:Seq` or `rdf:Bag`; or the same words in several languages under an
/// `rdf:Alt`, each item tagged with its language, one of them `x-default`.
///
/// A structure — a property whose value is itself a set of named fields —
/// comes back empty. Nothing the panel shows is one, and the fields would
/// need naming in turn to mean anything.
fn values(element: Node) -> Vec<String> {
    let list = element
        .children()
        .find(|node| node.is_element() && node.tag_name().namespace() == Some(RDF));
    let Some(list) = list else {
        return element.text().map(str::to_string).into_iter().collect();
    };
    let items = list
        .children()
        .filter(|node| node.has_tag_name((RDF, "li")));
    match list.tag_name().name() {
        "Alt" => {
            let items: Vec<Node> = items.collect();
            let default = items.iter().find(|item| {
                item.attribute(("http://www.w3.org/XML/1998/namespace", "lang"))
                    == Some("x-default")
            });
            default
                .or(items.first())
                .and_then(|item| item.text())
                .map(str::to_string)
                .into_iter()
                .collect()
        }
        "Seq" | "Bag" => items
            .filter_map(|item| item.text())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

/// The packet `path` carries, found by the container it is in. The file is
/// read only as far as the headers that lead to the packet, and the packet
/// itself; the pixels are seeked past. A TIFF is its own directory, and the
/// tag the packet sits in is read through [`directory`], which seeks to the
/// directory wherever the file keeps it. `None` for a container this does
/// not know, one that holds no packet, or one whose headers do not add up.
pub fn packet(path: &Path) -> Option<Vec<u8>> {
    let mut source = BufReader::new(File::open(path).ok()?);
    let mut signature = [0u8; 12];
    let read = source.read(&mut signature).ok()?;
    let signature = &signature[..read];
    source.seek(SeekFrom::Start(0)).ok()?;
    if signature.starts_with(&[0xff, 0xd8]) {
        jpeg(&mut source)
    } else if signature.starts_with(b"\x89PNG\r\n\x1a\n") {
        png(&mut source)
    } else if signature.starts_with(b"RIFF") && signature.get(8..12) == Some(b"WEBP") {
        webp(&mut source)
    } else if signature == JXL_SIGNATURE {
        jxl(&mut source)
    } else if signature.get(4..8) == Some(b"ftyp") {
        super::decode::xmp(path)
    } else if tiff::header(signature).is_some() {
        directory::packet(path)
    } else {
        None
    }
}

/// The packet in the sidecar beside `path`, if there is one: a file of the
/// picture's name with `.xmp` in place of its extension, or, failing that,
/// after it. `None` where there is neither, or the one there is runs past
/// [`MAX_PACKET`], which a file holding one packet never does.
pub fn sidecar(path: &Path) -> Option<Vec<u8>> {
    sidecar_names(path)
        .into_iter()
        .find_map(|name| File::open(name).ok())
        .and_then(|mut file| {
            let length = file.metadata().ok()?.len();
            take(&mut file, length)
        })
}

/// Where a sidecar of `path` would be, in the order they are looked for:
/// the specification's spelling first, then darktable's. A path with no
/// extension has the one spelling.
fn sidecar_names(path: &Path) -> Vec<PathBuf> {
    let mut names = vec![path.with_extension("xmp")];
    if path.extension().is_some() {
        let mut appended = path.as_os_str().to_owned();
        appended.push(".xmp");
        names.push(PathBuf::from(appended));
    }
    names
}

/// Reads `length` bytes, or gives up on a length nothing should be asked
/// for.
fn take(source: &mut impl Read, length: u64) -> Option<Vec<u8>> {
    if length > MAX_PACKET {
        return None;
    }
    let mut held = Vec::with_capacity(length as usize);
    source.take(length).read_to_end(&mut held).ok()?;
    (held.len() as u64 == length).then_some(held)
}

/// Seeks past `length` bytes from where the reader stands.
fn skip(source: &mut impl Seek, length: u64) -> Option<()> {
    source
        .seek(SeekFrom::Current(i64::try_from(length).ok()?))
        .ok()?;
    Some(())
}

/// The packet in an `APP1` segment headed [`JPEG_HEADER`]. The segments come
/// before the scan, so the walk stops at the scan's marker rather than
/// reading the entropy-coded data looking for more. Extended XMP — a
/// packet too long for one segment, spread over several under a different
/// header — is not gathered up; nothing shown here is long enough to need
/// it.
fn jpeg(source: &mut BufReader<File>) -> Option<Vec<u8>> {
    source.seek(SeekFrom::Start(2)).ok()?;
    loop {
        let mut marker = [0u8; 2];
        source.read_exact(&mut marker).ok()?;
        if marker[0] != 0xff {
            return None;
        }
        match marker[1] {
            // Fill bytes before a marker are the marker's own.
            0xff => {
                source.seek(SeekFrom::Current(-1)).ok()?;
                continue;
            }
            // The scan, or the end: no segment past here carries a packet.
            0xda | 0xd9 => return None,
            // Stand-alone markers carry no length.
            0x01 | 0xd0..=0xd7 => continue,
            _ => {}
        }
        let mut length = [0u8; 2];
        source.read_exact(&mut length).ok()?;
        let length = u64::from(u16::from_be_bytes(length)).checked_sub(2)?;
        if marker[1] == 0xe1 {
            let payload = take(source, length)?;
            if let Some(packet) = payload.strip_prefix(JPEG_HEADER) {
                return Some(packet.to_vec());
            }
        } else {
            skip(source, length)?;
        }
    }
}

/// The packet in an `iTXt` chunk keyed [`PNG_KEYWORD`]. The chunk may come
/// before or after the image data, so the walk runs to `IEND`, seeking past
/// the image data's chunks by their lengths. The chunk is written
/// uncompressed, as the packet's own specification asks; one that was
/// compressed anyway is left.
fn png(source: &mut BufReader<File>) -> Option<Vec<u8>> {
    source.seek(SeekFrom::Start(8)).ok()?;
    loop {
        let mut header = [0u8; 8];
        source.read_exact(&mut header).ok()?;
        let length = u64::from(u32::from_be_bytes([
            header[0], header[1], header[2], header[3],
        ]));
        let kind = &header[4..8];
        if kind == b"IEND" {
            return None;
        }
        if kind == b"iTXt" {
            let data = take(source, length)?;
            if let Some(rest) = data.strip_prefix(PNG_KEYWORD)
                && let Some(rest) = rest.strip_prefix(b"\0")
                && let [compressed, _method, rest @ ..] = rest
                && *compressed == 0
                // The language tag and the translated keyword, each ended
                // by a null, and then the text.
                && let Some(language_end) = rest.iter().position(|&byte| byte == 0)
                && let Some(keyword_end) = rest[language_end + 1..]
                    .iter()
                    .position(|&byte| byte == 0)
            {
                return Some(rest[language_end + 1 + keyword_end + 1..].to_vec());
            }
        } else {
            skip(source, length)?;
        }
        // The CRC.
        skip(source, 4)?;
    }
}

/// The packet in the RIFF chunk called `XMP `, which sits beside the pixels
/// rather than in them. Each chunk is padded to an even length.
fn webp(source: &mut BufReader<File>) -> Option<Vec<u8>> {
    source.seek(SeekFrom::Start(12)).ok()?;
    loop {
        let mut header = [0u8; 8];
        if source.read_exact(&mut header).is_err() {
            return None;
        }
        let length = u64::from(u32::from_le_bytes([
            header[4], header[5], header[6], header[7],
        ]));
        if &header[..4] == b"XMP " {
            return take(source, length);
        }
        skip(source, length + (length & 1))?;
    }
}

/// The packet in the container's `xml ` box. The container's boxes are the
/// ISO base media format's, at the top level: a length, a type, and the
/// contents, with a length of one meaning an eight-byte length follows and
/// a length of zero meaning the box runs to the end of the file. A box that
/// was Brotli-compressed into a `brob` is left.
fn jxl(source: &mut BufReader<File>) -> Option<Vec<u8>> {
    source.seek(SeekFrom::Start(0)).ok()?;
    loop {
        let mut header = [0u8; 8];
        if source.read_exact(&mut header).is_err() {
            return None;
        }
        let short = u64::from(u32::from_be_bytes([
            header[0], header[1], header[2], header[3],
        ]));
        let kind = &header[4..8];
        let mut used = 8;
        let length = match short {
            0 => None,
            1 => {
                let mut long = [0u8; 8];
                source.read_exact(&mut long).ok()?;
                used += 8;
                Some(u64::from_be_bytes(long))
            }
            _ => Some(short),
        };
        // A box shorter than its own header is a file that does not add up.
        let contents = match length {
            Some(length) => Some(length.checked_sub(used)?),
            None => None,
        };
        if kind == b"xml " {
            return match contents {
                Some(contents) => take(source, contents),
                None => {
                    let mut held = Vec::new();
                    source.take(MAX_PACKET).read_to_end(&mut held).ok()?;
                    Some(held)
                }
            };
        }
        // A box that runs to the end of the file is the last one.
        skip(source, contents?)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A packet as a cataloging program writes one: a title in a set of
    /// translations, a list of creators, a bag of keywords, and the
    /// program's name as an attribute.
    const PACKET: &str = r#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="XMP Core 4.4.0-Exiv2">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:dc="http://purl.org/dc/elements/1.1/"
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmp:CreatorTool="digiKam-8.0.0">
   <dc:title>
    <rdf:Alt>
     <rdf:li xml:lang="de-DE">Mäusebussard</rdf:li>
     <rdf:li xml:lang="x-default">Common Buzzard</rdf:li>
    </rdf:Alt>
   </dc:title>
   <dc:creator>
    <rdf:Seq>
     <rdf:li>John James Audubon</rdf:li>
     <rdf:li>Robert Havell</rdf:li>
    </rdf:Seq>
   </dc:creator>
   <dc:subject>
    <rdf:Bag>
     <rdf:li>bird</rdf:li>
     <rdf:li>raptor</rdf:li>
    </rdf:Bag>
   </dc:subject>
   <dc:description>
    <rdf:Alt>
     <rdf:li xml:lang="x-default">   </rdf:li>
    </rdf:Alt>
   </dc:description>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
<?xpacket end="w"?>"#;

    fn parsed() -> Xmp {
        Xmp::parse(PACKET.as_bytes()).expect("the packet parses")
    }

    fn written(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("gamut-xmp-{name}"));
        std::fs::write(&path, bytes).expect("the temporary directory is writable");
        path
    }

    #[test]
    fn a_set_of_translations_gives_its_default() {
        let strings = |values: &[String]| values.to_vec();
        assert_eq!(
            parsed().property(DC, "title").map(strings),
            Some(vec!["Common Buzzard".to_string()])
        );
    }

    #[test]
    fn a_list_gives_every_item() {
        assert_eq!(
            parsed().property(DC, "creator"),
            Some(
                &[
                    "John James Audubon".to_string(),
                    "Robert Havell".to_string()
                ][..]
            )
        );
        assert_eq!(
            parsed().property(DC, "subject"),
            Some(&["bird".to_string(), "raptor".to_string()][..])
        );
    }

    #[test]
    fn an_attribute_is_a_property_too() {
        assert_eq!(
            parsed().property(BASIC, "CreatorTool"),
            Some(&["digiKam-8.0.0".to_string()][..])
        );
    }

    #[test]
    fn a_blank_property_is_not_held() {
        assert_eq!(parsed().property(DC, "description"), None);
        assert_eq!(parsed().property(DC, "rights"), None);
    }

    /// A packet whose padding is nulls rather than spaces, as some writers
    /// pad, still parses; one that is not XML at all comes back as nothing.
    #[test]
    fn padding_and_rubbish_are_told_apart() {
        let mut padded = PACKET.as_bytes().to_vec();
        padded.extend_from_slice(&[0; 64]);
        assert!(Xmp::parse(&padded).is_some());
        assert_eq!(Xmp::parse(b"<not xml"), None);
        assert_eq!(Xmp::parse(b"<a><b/></a>"), None);
    }

    /// Four containers around the same packet, each assembled by hand as
    /// its format lays the packet out, and each found through its headers.
    #[test]
    fn a_packet_is_found_in_each_container() {
        let packet = PACKET.as_bytes();

        let mut jpeg = b"\xff\xd8".to_vec();
        // An APP0 to walk past, then the APP1, then the scan.
        jpeg.extend_from_slice(b"\xff\xe0\x00\x04\x00\x00");
        let mut payload = JPEG_HEADER.to_vec();
        payload.extend_from_slice(packet);
        jpeg.extend_from_slice(b"\xff\xe1");
        jpeg.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        jpeg.extend_from_slice(&payload);
        jpeg.extend_from_slice(b"\xff\xda\x00\x02\xff\xd9");

        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let chunk = |png: &mut Vec<u8>, kind: &[u8], data: &[u8]| {
            png.extend_from_slice(&(data.len() as u32).to_be_bytes());
            png.extend_from_slice(kind);
            png.extend_from_slice(data);
            png.extend_from_slice(&[0; 4]);
        };
        chunk(&mut png, b"IHDR", &[0; 13]);
        chunk(&mut png, b"IDAT", &[0; 40]);
        let mut text = PNG_KEYWORD.to_vec();
        text.extend_from_slice(b"\0\0\0\0\0");
        text.extend_from_slice(packet);
        chunk(&mut png, b"iTXt", &text);
        chunk(&mut png, b"IEND", &[]);

        let mut webp = b"RIFF\0\0\0\0WEBP".to_vec();
        // An odd-length chunk to walk past, padded, then the packet.
        webp.extend_from_slice(b"VP8X\x0b\x00\x00\x00");
        webp.extend_from_slice(&[0; 12]);
        webp.extend_from_slice(b"XMP ");
        webp.extend_from_slice(&(packet.len() as u32).to_le_bytes());
        webp.extend_from_slice(packet);

        let mut jxl = JXL_SIGNATURE.to_vec();
        let boxed = |jxl: &mut Vec<u8>, kind: &[u8], data: &[u8]| {
            jxl.extend_from_slice(&((data.len() + 8) as u32).to_be_bytes());
            jxl.extend_from_slice(kind);
            jxl.extend_from_slice(data);
        };
        boxed(&mut jxl, b"ftyp", b"jxl \0\0\0\0jxl ");
        boxed(&mut jxl, b"xml ", packet);
        boxed(&mut jxl, b"jxlc", &[0xff, 0x0a]);

        for (name, bytes) in [
            ("found.jpg", jpeg),
            ("found.png", png),
            ("found.webp", webp),
            ("found.jxl", jxl),
        ] {
            let found = super::packet(&written(name, &bytes));
            assert_eq!(found.as_deref(), Some(packet), "{name}");
            assert_eq!(
                Xmp::read(&written(name, &bytes)).property(DC, "title"),
                Some(&["Common Buzzard".to_string()][..]),
                "{name}"
            );
        }
    }

    /// A sidecar is read where the file has no packet, in either spelling
    /// of its name; and where the file has one too, the sidecar's word on
    /// a property is the one taken, and the packet's other properties
    /// stand.
    #[test]
    fn a_sidecar_is_read_beside_the_file() {
        const SIDECAR: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:dc="http://purl.org/dc/elements/1.1/"
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmp:CreatorTool="darktable">
   <dc:title>
    <rdf:Alt>
     <rdf:li xml:lang="x-default">Buzzard, Retitled</rdf:li>
    </rdf:Alt>
   </dc:title>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>"#;
        let retitled = Some(&["Buzzard, Retitled".to_string()][..]);

        // A plain JPEG with no packet, and its sidecar under each name. The
        // temporary directory outlives the test, so the sidecar an earlier
        // run wrote is taken away before the read that expects none.
        let plain = written("alone.jpg", b"\xff\xd8\xff\xd9");
        for name in sidecar_names(&plain) {
            let _ = std::fs::remove_file(name);
        }
        assert_eq!(Xmp::read(&plain), Xmp::default());
        written("alone.xmp", SIDECAR.as_bytes());
        assert_eq!(Xmp::read(&plain).property(DC, "title"), retitled);

        let plain = written("appended.jpg", b"\xff\xd8\xff\xd9");
        written("appended.jpg.xmp", SIDECAR.as_bytes());
        assert_eq!(Xmp::read(&plain).property(DC, "title"), retitled);

        // A file that carries the packet, and a sidecar retitling it.
        let mut jpeg = b"\xff\xd8".to_vec();
        let mut payload = JPEG_HEADER.to_vec();
        payload.extend_from_slice(PACKET.as_bytes());
        jpeg.extend_from_slice(b"\xff\xe1");
        jpeg.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        jpeg.extend_from_slice(&payload);
        jpeg.extend_from_slice(b"\xff\xda\x00\x02\xff\xd9");
        let both = written("both.jpg", &jpeg);
        written("both.xmp", SIDECAR.as_bytes());
        let read = Xmp::read(&both);
        assert_eq!(read.property(DC, "title"), retitled);
        assert_eq!(
            read.property(BASIC, "CreatorTool"),
            Some(&["darktable".to_string()][..])
        );
        assert_eq!(read.property(DC, "subject"), parsed().property(DC, "subject"));
        assert_eq!(
            Xmp::read_with(&both, super::packet(&both).as_deref()),
            read
        );

        // A sidecar that is not a packet leaves the file's own alone.
        let own = written("rubbish.jpg", &jpeg);
        written("rubbish.xmp", b"<not xml");
        assert_eq!(Xmp::read(&own), parsed());
    }

    /// A container with no packet, one cut short, and one whose lengths do
    /// not add up all come back as nothing rather than as a panic or as a
    /// read of the whole file.
    #[test]
    fn a_container_without_a_packet_gives_nothing() {
        assert_eq!(
            super::packet(&written("plain.jpg", b"\xff\xd8\xff\xd9")),
            None
        );
        assert_eq!(
            super::packet(&written("short.jpg", b"\xff\xd8\xff\xe1\xff\xff")),
            None
        );
        assert_eq!(
            super::packet(&written(
                "plain.webp",
                b"RIFF\0\0\0\0WEBPVP8 \x00\x00\x00\x00"
            )),
            None
        );
        assert_eq!(
            super::packet(&written(
                "lying.webp",
                b"RIFF\0\0\0\0WEBPXMP \xff\xff\xff\x7f"
            )),
            None
        );
        assert_eq!(super::packet(&written("empty.png", b"")), None);
        assert_eq!(super::packet(&written("nothing.txt", b"hello")), None);
        assert!(
            Xmp::read(Path::new("/nonexistent/image.jpg"))
                .property(DC, "title")
                .is_none()
        );
    }
}
