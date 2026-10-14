use std::sync::OnceLock;

pub const SRGB_TO_P3: [[f32; 3]; 3] = [
    [0.82246196, 0.17753805, 0.0],
    [0.0331942, 0.9668058, 0.0],
    [0.01708263, 0.07239744, 0.9105195],
];

pub const P3_TO_SRGB: [[f32; 3]; 3] = [
    [1.2249402, -0.22494018, 0.0],
    [-0.042056955, 1.0420569, 0.0],
    [-0.019637553, -0.07863604, 1.0982736],
];

fn decode_lut() -> &'static [f32; 256] {
    static LUT: OnceLock<[f32; 256]> = OnceLock::new();
    LUT.get_or_init(|| {
        std::array::from_fn(|i| {
            let c = i as f64 / 255.0;
            let lin = if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            };
            lin as f32
        })
    })
}

fn encode_lut() -> &'static [u8; 4096] {
    static LUT: OnceLock<[u8; 4096]> = OnceLock::new();
    LUT.get_or_init(|| {
        std::array::from_fn(|i| {
            let lin = i as f64 / 4095.0;
            let g = if lin <= 0.0031308 {
                12.92 * lin
            } else {
                1.055 * lin.powf(1.0 / 2.4) - 0.055
            };
            (g.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
        })
    })
}

pub fn to_p3_premultiplied(rgba: &mut [u8]) {
    let dec = decode_lut();
    let enc = encode_lut();
    for px in rgba.as_chunks_mut::<4>().0 {
        let a = f32::from(px[3]) / 255.0;
        if a <= 0.0 {
            continue;
        }
        let [r, g, b] = [0, 1, 2].map(|i| (f32::from(px[i]) / a).min(255.0) as usize);
        let [lr, lg, lb] = [dec[r], dec[g], dec[b]];
        let mut out = [0u8; 3];
        for (i, row) in SRGB_TO_P3.iter().enumerate() {
            let lin = row[0] * lr + row[1] * lg + row[2] * lb;
            out[i] = enc[(lin.clamp(0.0, 1.0) * 4095.0).round() as usize];
        }
        for c in 0..3 {
            px[c] = (f32::from(out[c]) * a + 0.5) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(px: [u8; 4]) -> [u8; 4] {
        let mut buf = px;
        to_p3_premultiplied(&mut buf);
        buf
    }

    fn close(a: [u8; 4], b: [u8; 4]) {
        for i in 0..3 {
            assert!(
                (i16::from(a[i]) - i16::from(b[i])).abs() <= 1,
                "channel {i}: got {} expected {}",
                a[i],
                b[i]
            );
        }
        assert_eq!(a[3], b[3], "alpha must not change");
    }

    #[test]
    fn white_and_black_are_fixed_points() {
        close(convert([255, 255, 255, 255]), [255, 255, 255, 255]);
        close(convert([0, 0, 0, 255]), [0, 0, 0, 255]);
    }

    #[test]
    fn zero_alpha_is_left_untouched() {
        assert_eq!(convert([0, 0, 0, 0]), [0, 0, 0, 0]);
        assert_eq!(convert([12, 34, 56, 0]), [12, 34, 56, 0]);
    }

    #[test]
    fn pure_primaries_move_to_p3() {
        close(convert([255, 0, 0, 255]), [234, 51, 35, 255]);
        close(convert([0, 255, 0, 255]), [117, 251, 76, 255]);
        close(convert([0, 0, 255, 255]), [0, 0, 245, 255]);
    }

    #[test]
    fn premultiplied_round_trip() {
        close(convert([128, 0, 0, 128]), [117, 26, 18, 128]);
    }

    #[test]
    #[allow(clippy::needless_range_loop)]
    fn matrices_are_inverses() {
        let m = SRGB_TO_P3;
        let inv = P3_TO_SRGB;
        let mul = |a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]| -> [[f32; 3]; 3] {
            let mut out = [[0.0; 3]; 3];
            for i in 0..3 {
                for j in 0..3 {
                    for k in 0..3 {
                        out[i][j] += a[i][k] * b[k][j];
                    }
                }
            }
            out
        };
        let ident = mul(&inv, &m);
        for i in 0..3 {
            for j in 0..3 {
                let want = if i == j { 1.0 } else { 0.0 };
                assert!(
                    (ident[i][j] - want).abs() < 1e-5,
                    "not an inverse at ({i},{j}): {}",
                    ident[i][j]
                );
            }
        }
    }
}
