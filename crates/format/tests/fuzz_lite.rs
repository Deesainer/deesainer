#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use deesainer_color::Rgba;
use deesainer_doc::Document;

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

#[test]
fn mutated_files_never_panic() {
    let mut doc = Document::with_frame(100.0, 100.0, Rgba::WHITE).unwrap();
    let data = vec![7u8; 300];
    doc.insert_asset(deesainer_format::asset_id(&data), Arc::new(data));
    let good = deesainer_format::write(&doc, Some(b"png")).unwrap();
    let mut rng = Lcg(42);
    for _ in 0..2000 {
        let mut bytes = good.clone();
        let flips = 1 + rng.next() % 8;
        for _ in 0..flips {
            let i = (rng.next() as usize) % bytes.len();
            bytes[i] = rng.next() as u8;
        }
        if rng.next().is_multiple_of(5) {
            let cut = (rng.next() as usize) % bytes.len();
            bytes.truncate(cut);
        }
        if let Ok(d) = deesainer_format::read(&bytes) {
            d.validate().unwrap();
        }
    }
}
