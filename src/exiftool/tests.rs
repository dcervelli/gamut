use super::*;

/// exiftool's `-X -l -D -t -G1 -a -u -struct` as it writes it, cut down to
/// one of each shape a tag comes in.
const FIXTURE: &str = r#"<?xml version='1.0' encoding='UTF-8'?>
<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>

<rdf:Description rdf:about='/tmp/sample.jpg'
  xmlns:et='http://ns.exiftool.org/1.0/' et:toolkit='Image::ExifTool 13.55'
  xmlns:ExifTool='http://ns.exiftool.org/ExifTool/1.0/'
  xmlns:System='http://ns.exiftool.org/File/System/1.0/'
  xmlns:IFD0='http://ns.exiftool.org/EXIF/IFD0/1.0/'
  xmlns:XMP-dc='http://ns.exiftool.org/XMP/XMP-dc/1.0/'
  xmlns:XMP-mwg-rs='http://ns.exiftool.org/XMP/XMP-mwg-rs/1.0/'
  xmlns:Composite='http://ns.exiftool.org/Composite/1.0/'>
 <ExifTool:Warning>
  <rdf:Description et:id='Warning' et:table='Extra'>
   <et:desc>Warning</et:desc>
   <et:prt>Truncated file</et:prt>
  </rdf:Description>
 </ExifTool:Warning>
 <System:FileSize>
  <rdf:Description et:id='FileSize' et:table='Extra'>
   <et:desc>File Size</et:desc>
   <et:prt>1540 bytes</et:prt>
   <et:val>1540</et:val>
  </rdf:Description>
 </System:FileSize>
 <IFD0:Orientation>
  <rdf:Description et:id='274' et:table='Exif::Main'>
   <et:desc>Orientation</et:desc>
   <et:prt>Rotate 180</et:prt>
   <et:val>3</et:val>
  </rdf:Description>
 </IFD0:Orientation>
 <IFD0:Orientation>
  <rdf:Description et:id='274' et:table='Exif::Main'>
   <et:desc>Orientation</et:desc>
   <et:prt>Horizontal (normal)</et:prt>
   <et:val>1</et:val>
  </rdf:Description>
 </IFD0:Orientation>
 <IFD0:ThumbnailImage>
  <rdf:Description et:id='513' et:table='Exif::Main'>
   <et:desc>Thumbnail Image</et:desc>
   <et:prt>(Binary data 5120 bytes, use -b option to extract)</et:prt>
  </rdf:Description>
 </IFD0:ThumbnailImage>
 <XMP-dc:Description>
  <rdf:Description et:id='description' et:table='XMP::dc'>
   <et:desc>Description</et:desc>
   <et:prt>The packet&#39;s caption</et:prt>
  </rdf:Description>
 </XMP-dc:Description>
 <XMP-dc:Title-fr>
  <rdf:Description et:id='title-fr' xml:lang='fr' et:table='XMP::dc'>
   <et:desc>Title (fr)</et:desc>
   <et:prt>Quatre</et:prt>
  </rdf:Description>
 </XMP-dc:Title-fr>
 <XMP-dc:Subject>
  <rdf:Description et:id='subject' et:table='XMP::dc'>
   <et:desc>Subject</et:desc>
   <et:prt>
    <rdf:Bag>
     <rdf:li>red</rdf:li>
     <rdf:li>green</rdf:li>
    </rdf:Bag>
   </et:prt>
  </rdf:Description>
 </XMP-dc:Subject>
 <XMP-mwg-rs:RegionInfo>
  <rdf:Description et:id='Regions' et:table='MWG::Regions'>
   <et:desc>Region Info</et:desc>
   <et:prt rdf:parseType='Resource'>
    <XMP-mwg-rs:AppliedToDimensions rdf:parseType='Resource'>
     <XMP-mwg-rs:H>24</XMP-mwg-rs:H>
     <XMP-mwg-rs:W>32</XMP-mwg-rs:W>
    </XMP-mwg-rs:AppliedToDimensions>
    <XMP-mwg-rs:RegionList>
     <rdf:Bag>
      <rdf:li rdf:parseType='Resource'>
       <XMP-mwg-rs:Name>Ann</XMP-mwg-rs:Name>
      </rdf:li>
      <rdf:li rdf:parseType='Resource'>
       <XMP-mwg-rs:Name>Bob</XMP-mwg-rs:Name>
      </rdf:li>
     </rdf:Bag>
    </XMP-mwg-rs:RegionList>
   </et:prt>
  </rdf:Description>
 </XMP-mwg-rs:RegionInfo>
 <Composite:ImageSize>
  <rdf:Description et:id='ImageSize' et:table='Composite'>
   <et:desc>Image Size</et:desc>
   <et:prt>32x24</et:prt>
   <et:val>32 24</et:val>
  </rdf:Description>
 </Composite:ImageSize>
</rdf:Description>
</rdf:RDF>
"#;

fn text(value: &str) -> Value {
    Value::Text(value.to_string())
}

/// Every shape is read: the groups from the namespace, the words and both
/// values from inside, duplicates kept in the order written.
#[test]
fn the_fixture_is_read_whole() {
    let report = parse(FIXTURE).expect("well formed");
    assert_eq!(report.version, "13.55");
    let names: Vec<_> = report
        .tags
        .iter()
        .map(|tag| format!("{}/{}:{}", tag.family0, tag.family1, tag.name))
        .collect();
    assert_eq!(
        names,
        [
            "ExifTool/ExifTool:Warning",
            "File/System:FileSize",
            "EXIF/IFD0:Orientation",
            "EXIF/IFD0:Orientation",
            "EXIF/IFD0:ThumbnailImage",
            "XMP/XMP-dc:Description",
            "XMP/XMP-dc:Title-fr",
            "XMP/XMP-dc:Subject",
            "XMP/XMP-mwg-rs:RegionInfo",
            "Composite/Composite:ImageSize",
        ]
    );

    let orientation = &report.tags[2];
    assert_eq!(orientation.id.as_deref(), Some("274"));
    assert_eq!(orientation.table.as_deref(), Some("Exif::Main"));
    assert_eq!(orientation.desc, "Orientation");
    assert_eq!(orientation.printed, text("Rotate 180"));
    assert_eq!(orientation.raw, Some(text("3")));
    assert_eq!(report.tags[3].raw, Some(text("1")));

    // A value printed as it is has no raw one beside it.
    assert_eq!(report.tags[0].printed, text("Truncated file"));
    assert_eq!(report.tags[0].raw, None);
    assert_eq!(
        report.tags[4].printed,
        text("(Binary data 5120 bytes, use -b option to extract)")
    );
    assert_eq!(report.tags[5].printed, text("The packet's caption"));
    assert_eq!(report.tags[6].lang.as_deref(), Some("fr"));
    assert_eq!(report.tags[5].lang, None);
    assert_eq!(
        report.tags[7].printed,
        Value::List(vec![text("red"), text("green")])
    );
}

/// A structure keeps its fields by their own names, and a list of
/// structures inside it is a list of them.
#[test]
fn a_structure_nests() {
    let report = parse(FIXTURE).expect("well formed");
    let regions = &report.tags[8].printed;
    assert_eq!(
        *regions,
        Value::Struct(vec![
            (
                "AppliedToDimensions".to_string(),
                Value::Struct(vec![
                    ("H".to_string(), text("24")),
                    ("W".to_string(), text("32"))
                ])
            ),
            (
                "RegionList".to_string(),
                Value::List(vec![
                    Value::Struct(vec![("Name".to_string(), text("Ann"))]),
                    Value::Struct(vec![("Name".to_string(), text("Bob"))]),
                ])
            ),
        ])
    );
    assert_eq!(
        regions.leaves(),
        [
            ("AppliedToDimensions.H".to_string(), "24"),
            ("AppliedToDimensions.W".to_string(), "32"),
            ("RegionList.1.Name".to_string(), "Ann"),
            ("RegionList.2.Name".to_string(), "Bob"),
        ]
    );
    assert_eq!(regions.summary(), "2 fields");
    assert_eq!(report.tags[7].printed.summary(), "[red, green]");
    // A list of text is one piece, written whole; a list of anything
    // deeper is walked.
    assert_eq!(
        regions.pieces(),
        [
            ("AppliedToDimensions.H".to_string(), "24".to_string()),
            ("AppliedToDimensions.W".to_string(), "32".to_string()),
            ("RegionList.1.Name".to_string(), "Ann".to_string()),
            ("RegionList.2.Name".to_string(), "Bob".to_string()),
        ]
    );
    match regions {
        Value::Struct(fields) => assert_eq!(fields[1].1.summary(), "2 items"),
        other => panic!("a structure, not {other:?}"),
    }
    assert_eq!(
        report.tags[7].printed.pieces(),
        [(String::new(), "[red, green]".to_string())]
    );
    assert_eq!(text("x").leaves(), [(String::new(), "x")]);
}

/// Anything else is refused rather than read as nothing.
#[test]
fn malformed_output_is_an_error() {
    assert!(parse("").is_err());
    assert!(parse("<rdf:RDF").is_err());
    assert!(parse("<other/>").is_err());
    // What exiftool writes for a file it could not find: no description.
    assert!(
        parse("<?xml version='1.0'?><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'></rdf:RDF>")
            .is_err()
    );
}

/// A path is taken as it is, and only where it is a file; a bare name is
/// found in the directories given, in order.
#[test]
fn the_program_is_located() {
    let images = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_images");
    let png = images.join("png-rgb8.png");
    assert_eq!(
        locate_in(png.to_str().expect("utf-8"), &[]),
        Some(png.clone())
    );
    assert_eq!(locate_in(images.to_str().expect("utf-8"), &[]), None);
    assert_eq!(
        locate_in(
            "png-rgb8.png",
            &[PathBuf::from("/nonexistent"), images.clone()]
        ),
        Some(png)
    );
    assert_eq!(locate_in("png-rgb8.png", &[]), None);
    assert_eq!(locate_in("", std::slice::from_ref(&images)), None);
}

/// The program on this machine, where there is one — required where
/// `GAMUT_REQUIRE_EXIFTOOL` is set, which is how a run proves it ran.
fn installed() -> Option<PathBuf> {
    let found = Program::new("exiftool").locate().map(Path::to_path_buf);
    assert!(
        found.is_some() || std::env::var_os("GAMUT_REQUIRE_EXIFTOOL").is_none(),
        "GAMUT_REQUIRE_EXIFTOOL is set and exiftool was not found"
    );
    found
}

/// A real run: a file's tags, a missing file's refusal, and a damaged
/// file read with what is wrong with it.
#[test]
fn exiftool_itself_reads_the_fixtures() {
    let Some(program) = installed() else {
        return;
    };
    let images = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_images");

    let report = run(&program, &images.join("jpeg-about.jpg")).expect("read");
    assert!(!report.version.is_empty());
    let find = |name: &str| {
        report
            .tags
            .iter()
            .find(|tag| tag.name == name)
            .unwrap_or_else(|| panic!("{name} is read"))
    };
    assert_eq!(find("Artist").family1, "IFD0");
    assert_eq!(find("Artist").printed, text("Test Pattern"));
    match &find("Subject").printed {
        Value::List(items) => assert_eq!(items.len(), 4),
        other => panic!("Subject is a list, not {other:?}"),
    }

    let rotated = run(&program, &images.join("jpeg-exif-rotated.jpg")).expect("read");
    assert!(
        rotated
            .tags
            .iter()
            .any(|tag| tag.name == "Orientation" && tag.raw.is_some())
    );

    match run(&program, &images.join("no-such-file.jpg")) {
        Err(Failure::Failed(said)) => assert!(said.contains("File not found"), "{said}"),
        other => panic!("a missing file fails, not {other:?}"),
    }

    let damaged = run(&program, &images.join("bad-truncated.png")).expect("still read");
    assert!(damaged.tags.iter().any(|tag| tag.name == "Warning"));
}
