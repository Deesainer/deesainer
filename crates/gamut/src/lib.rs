pub fn display_p3() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::display_p3()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

#[cfg(target_os = "macos")]
mod macos;

const P3_RED: [f64; 2] = [0.6800, 0.3200];
const P3_GREEN: [f64; 2] = [0.2650, 0.6900];

pub fn is_p3_primaries(red: [f64; 2], green: [f64; 2]) -> bool {
    (red[0] - P3_RED[0]).abs() < 0.02
        && (red[1] - P3_RED[1]).abs() < 0.02
        && (green[0] - P3_GREEN[0]).abs() < 0.02
        && (green[1] - P3_GREEN[1]).abs() < 0.02
}

fn be32(b: &[u8], o: usize) -> Option<u32> {
    let s = b.get(o..o + 4)?;
    Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

pub fn primaries_from_icc(icc: &[u8]) -> Option<[[f64; 2]; 3]> {
    if icc.len() < 132 {
        return None;
    }
    let count = be32(icc, 128)? as usize;
    if count == 0 || count > 64 {
        return None;
    }
    let mut red = None;
    let mut green = None;
    let mut blue = None;
    for i in 0..count {
        let base = 132 + i * 12;
        let sig = icc.get(base..base + 4)?;
        if sig != b"rXYZ" && sig != b"gXYZ" && sig != b"bXYZ" {
            continue;
        }
        let off = be32(icc, base + 4)? as usize;
        if off + 20 > icc.len() || icc.get(off..off + 4)? != b"XYZ " {
            return None;
        }
        let channel = |k: usize| -> Option<f64> {
            Some(f64::from(be32(icc, off + 8 + k * 4)? as i32) / 65536.0)
        };
        let (x, y, z) = (channel(0)?, channel(1)?, channel(2)?);
        let sum = x + y + z;
        if sum.abs() < 1e-9 {
            return None;
        }
        let chromaticity = [x / sum, y / sum];
        match sig {
            b"rXYZ" => red = Some(chromaticity),
            b"gXYZ" => green = Some(chromaticity),
            b"bXYZ" => blue = Some(chromaticity),
            _ => {}
        }
    }
    Some([red?, green?, blue?])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_i32(b: &mut [u8], o: usize, v: i32) {
        b[o..o + 4].copy_from_slice(&v.to_be_bytes());
    }

    fn icc_from(tags: &[(&[u8], Option<[f64; 2]>)]) -> Vec<u8> {
        let count = tags.len();
        let data_base = 132 + count * 12;
        let mut icc = vec![0u8; data_base + count * 20];
        put_i32(&mut icc, 128, count as i32);
        for (i, (sig, chromaticity)) in tags.iter().enumerate() {
            let base = 132 + i * 12;
            icc[base..base + 4].copy_from_slice(sig);
            put_i32(&mut icc, base + 4, (data_base + i * 20) as i32);
            put_i32(&mut icc, base + 8, 20);
            let off = data_base + i * 20;
            icc[off..off + 4].copy_from_slice(b"XYZ ");
            if let Some([x, y]) = chromaticity {
                put_i32(&mut icc, off + 8, (x * 65536.0).round() as i32);
                put_i32(&mut icc, off + 12, (y * 65536.0).round() as i32);
                put_i32(&mut icc, off + 16, ((1.0 - x - y) * 65536.0).round() as i32);
            }
        }
        icc
    }

    fn icc_with(red: [f64; 2], green: [f64; 2], blue: [f64; 2]) -> Vec<u8> {
        icc_from(&[
            (b"rXYZ", Some(red)),
            (b"gXYZ", Some(green)),
            (b"bXYZ", Some(blue)),
        ])
    }

    const P3: [[f64; 2]; 3] = [[0.6800, 0.3200], [0.2650, 0.6900], [0.1500, 0.0600]];
    const SRGB: [[f64; 2]; 3] = [[0.6400, 0.3300], [0.3000, 0.6000], [0.1500, 0.0600]];

    #[test]
    fn parses_p3_primaries() {
        let primaries = primaries_from_icc(&icc_with(P3[0], P3[1], P3[2])).unwrap();
        assert!(is_p3_primaries(primaries[0], primaries[1]));
    }

    #[test]
    fn color_lcd_style_primaries_count_as_p3() {
        let primaries = primaries_from_icc(&icc_with([0.682, 0.319], P3[1], P3[2])).unwrap();
        assert!(is_p3_primaries(primaries[0], primaries[1]));
    }

    #[test]
    fn srgb_primaries_are_not_p3() {
        let primaries = primaries_from_icc(&icc_with(SRGB[0], SRGB[1], SRGB[2])).unwrap();
        assert!(!is_p3_primaries(primaries[0], primaries[1]));
    }

    #[test]
    fn rejects_garbage_and_malformed_profiles() {
        assert!(primaries_from_icc(&[0u8; 64]).is_none());
        let mut icc = icc_with(P3[0], P3[1], P3[2]);
        let off = be32(&icc, 136).unwrap() as usize;
        icc[off..off + 4].copy_from_slice(b"NOPE");
        assert!(primaries_from_icc(&icc).is_none());
        let mut icc = icc_with(P3[0], P3[1], P3[2]);
        put_i32(&mut icc, 136, 10_000);
        assert!(primaries_from_icc(&icc).is_none());
    }

    #[test]
    fn ignores_non_xyz_tags() {
        let icc = icc_from(&[
            (b"desc", None),
            (b"cprt", None),
            (b"rXYZ", Some(P3[0])),
            (b"gXYZ", Some(P3[1])),
            (b"bXYZ", Some(P3[2])),
        ]);
        let primaries = primaries_from_icc(&icc).unwrap();
        assert!(is_p3_primaries(primaries[0], primaries[1]));
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn non_macos_is_false() {
        assert!(!display_p3());
    }
}
