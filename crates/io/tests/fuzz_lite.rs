#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

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
fn mutated_svgs_never_panic() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64">
      <defs><radialGradient id="r"><stop offset="0" stop-color="#fff"/><stop offset="1" stop-color="#000"/></radialGradient></defs>
      <g transform="rotate(10 32 32)"><path d="M4 4 L60 4 Q60 60 4 60 Z" fill="url(#r)" stroke="#f00" stroke-dasharray="2 2"/></g>
      <circle cx="32" cy="32" r="8" fill="#0f0" opacity="0.5"/></svg>"##;
    let mut rng = Lcg(7);
    let bytes = svg.as_bytes();
    for _ in 0..1500 {
        let mut b = bytes.to_vec();
        for _ in 0..(1 + rng.next() % 6) {
            let i = (rng.next() as usize) % b.len();
            b[i] = {
                const A: &[u8] = b" <>/=\"#0123456789.-MLQZ";
                A[(rng.next() as usize) % A.len()]
            };
        }
        let text = String::from_utf8_lossy(&b);
        let _ = deesainer_io::import_svg(&text, "fuzz");
    }
}
