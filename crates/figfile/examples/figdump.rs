#![allow(clippy::unwrap_used, clippy::expect_used)]

fn main() {
    let path = std::env::args().nth(1).expect("usage: figdump FILE");
    let bytes = std::fs::read(&path).expect("read");
    let f = deesainer_figfile::read(&bytes).expect("parse");
    println!(
        "{} v{} blobs={} images={}",
        f.magic,
        f.version,
        f.blobs.len(),
        f.images.len()
    );
    for n in f
        .message
        .get("nodeChanges")
        .map(|v| v.as_array())
        .unwrap_or(&[])
    {
        let s = |k: &str| n.get(k).and_then(|v| v.as_str()).unwrap_or("").to_owned();
        println!("{:20} {}", s("type"), s("name"));
    }
}
