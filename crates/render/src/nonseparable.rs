use tiny_skia::{BlendMode, Pixmap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Hue,
    Saturation,
    Color,
    Luminosity,
}

pub(crate) fn mode(blend: BlendMode) -> Option<Mode> {
    match blend {
        BlendMode::Hue => Some(Mode::Hue),
        BlendMode::Saturation => Some(Mode::Saturation),
        BlendMode::Color => Some(Mode::Color),
        BlendMode::Luminosity => Some(Mode::Luminosity),
        _ => None,
    }
}

fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut out = c;
    if n < 0.0 && l - n > 1e-6 {
        out = out.map(|v| l + (v - l) * l / (l - n));
    }
    if x > 1.0 && x - l > 1e-6 {
        out = out.map(|v| l + (v - l) * (1.0 - l) / (x - l));
    }
    out
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color(c.map(|v| v + d))
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let mx = c[0].max(c[1]).max(c[2]);
    let mn = c[0].min(c[1]).min(c[2]);
    if mx <= mn {
        return [0.0; 3];
    }
    c.map(|v| (v - mn) * s / (mx - mn))
}

pub(crate) fn blend(mode: Mode, cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    match mode {
        Mode::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        Mode::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        Mode::Color => set_lum(cs, lum(cb)),
        Mode::Luminosity => set_lum(cb, lum(cs)),
    }
}

pub(crate) fn over(mode: Mode, src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
    let (sa, da) = (src[3], dst[3]);
    let un = |p: [f32; 4], a: f32| {
        if a > 0.0 {
            [p[0] / a, p[1] / a, p[2] / a]
        } else {
            [0.0; 3]
        }
    };
    let (cs, cb) = (un(src, sa), un(dst, da));
    let mixed = blend(mode, cb, cs).map(|v| v.clamp(0.0, 1.0));
    let mut out = [0.0; 4];
    for k in 0..3 {
        out[k] = sa * (1.0 - da) * cs[k] + sa * da * mixed[k] + (1.0 - sa) * dst[k];
    }
    out[3] = sa + da * (1.0 - sa);
    out
}

pub(crate) fn composite(
    dst: &mut Pixmap,
    src: &Pixmap,
    offset: (i32, i32),
    opacity: f32,
    mode: Mode,
) {
    let (dw, dh) = (dst.width() as i32, dst.height() as i32);
    let (sw, sh) = (src.width() as i32, src.height() as i32);
    let opacity = opacity.clamp(0.0, 1.0);
    let src_data = src.data();
    let dst_data = dst.data_mut();
    let unit = |v: u8| f32::from(v) / 255.0;
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    for sy in 0..sh {
        let dy = sy + offset.1;
        if dy < 0 || dy >= dh {
            continue;
        }
        for sx in 0..sw {
            let dx = sx + offset.0;
            if dx < 0 || dx >= dw {
                continue;
            }
            let si = ((sy * sw + sx) * 4) as usize;
            let di = ((dy * dw + dx) * 4) as usize;
            let (Some(s), Some(d)) = (src_data.get(si..si + 4), dst_data.get(di..di + 4)) else {
                continue;
            };
            if s[3] == 0 {
                continue;
            }
            let s = [unit(s[0]), unit(s[1]), unit(s[2]), unit(s[3])].map(|v| v * opacity);
            let d = [unit(d[0]), unit(d[1]), unit(d[2]), unit(d[3])];
            let o = over(mode, s, d);
            if let Some(px) = dst_data.get_mut(di..di + 4) {
                let a = byte(o[3]);
                px[0] = byte(o[0]).min(a);
                px[1] = byte(o[1]).min(a);
                px[2] = byte(o[2]).min(a);
                px[3] = a;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_brings_negative_channels_back_into_range() {
        let cb = [195.0 / 255.0, 60.0 / 255.0, 62.0 / 255.0];
        let out = blend(Mode::Color, cb, [0.95, 0.75, 0.2]);
        let l = lum(cb);
        assert!((lum(out) - l).abs() < 1e-4, "luminosity is kept");
        assert!(out.iter().all(|v| (0.0..=1.0).contains(v)), "{out:?}");
        let px = over(
            Mode::Color,
            [0.95 * 0.85, 0.75 * 0.85, 0.2 * 0.85, 0.85],
            [cb[0], cb[1], cb[2], 1.0],
        );
        let got = px.map(|v| (v * 255.0).round() as i32);
        assert_eq!(&got[..3], &[146, 95, 9]);
    }

    #[test]
    fn luminosity_and_hue_keep_their_defining_parts() {
        let cb = [0.2, 0.5, 0.8];
        let cs = [0.9, 0.3, 0.1];
        let l = blend(Mode::Luminosity, cb, cs);
        assert!((lum(l) - lum(cs)).abs() < 1e-4);
        let h = blend(Mode::Hue, cb, cs);
        assert!((lum(h) - lum(cb)).abs() < 1e-4);
        assert!((sat(h) - sat(cb)).abs() < 1e-3);
    }

    #[test]
    fn composite_skips_pixels_outside_and_transparent() {
        let mut dst = Pixmap::new(4, 4).unwrap();
        dst.fill(tiny_skia::Color::from_rgba8(100, 100, 100, 255));
        let mut src = Pixmap::new(2, 2).unwrap();
        src.fill(tiny_skia::Color::from_rgba8(255, 0, 0, 255));
        composite(&mut dst, &src, (3, 3), 1.0, Mode::Color);
        assert_eq!(&dst.data()[..4], &[100, 100, 100, 255]);
        let i = (3 * 4 + 3) * 4;
        assert_ne!(&dst.data()[i..i + 4], &[100, 100, 100, 255]);
    }
}
