use std::fs::File;
use std::io::BufReader;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    // 1. What does `image` do if we tell it the format outright?
    let reader = image::ImageReader::with_format(
        BufReader::new(File::open(&path).unwrap()),
        image::ImageFormat::Tiff,
    );
    match reader.decode() {
        Ok(d) => println!("image crate: {:?} {}x{}", d.color(), d.width(), d.height()),
        Err(e) => println!("image crate: ERROR {e}"),
    }
    // 2. What does the `tiff` crate give us directly?
    let mut dec = tiff::decoder::Decoder::new(BufReader::new(File::open(&path).unwrap())).unwrap();
    println!(
        "tiff crate: dims={:?} colortype={:?}",
        dec.dimensions(),
        dec.colortype()
    );
    match dec.read_image() {
        Ok(tiff::decoder::DecodingResult::F32(v)) => {
            let finite: Vec<f32> = v.iter().copied().filter(|x| x.is_finite()).collect();
            let min = finite.iter().copied().fold(f32::INFINITY, f32::min);
            let max = finite.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            println!("tiff crate: F32 len={} min={min} max={max}", v.len());
        }
        Ok(other) => println!(
            "tiff crate: other variant {:?}",
            std::mem::discriminant(&other)
        ),
        Err(e) => println!("tiff crate: ERROR {e}"),
    }
}
