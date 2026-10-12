use tiny_skia::Pixmap;

fn boxes(sigma: f64) -> [usize; 3] {
    let n = 3.0;
    let w_ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = w_ideal.floor() as i64;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wu = wl + 2;
    let m_ideal = (12.0 * sigma * sigma - n * (wl * wl) as f64 - 4.0 * n * wl as f64 - 3.0 * n)
        / (-4.0 * wl as f64 - 4.0);
    let m = m_ideal.round() as i64;
    let mut out = [0usize; 3];
    for (i, slot) in out.iter_mut().enumerate() {
        let w = if (i as i64) < m { wl } else { wu };
        *slot = (w.max(1) as usize - 1) / 2;
    }
    out
}

pub fn blur(pixmap: &mut Pixmap, radius: f64) {
    if !radius.is_finite() || radius < 0.5 {
        return;
    }
    let sigma = (radius / 2.0).min(250.0);
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    if w == 0 || h == 0 {
        return;
    }
    let radii = boxes(sigma);
    let pad: usize = radii.iter().sum();
    let (pw, ph) = (w + 2 * pad, h + 2 * pad);
    let Some(len) = pw.checked_mul(ph).and_then(|n| n.checked_mul(4)) else {
        return;
    };
    let mut padded = vec![0u8; len];
    let data = pixmap.data_mut();
    for y in 0..h {
        let (from, to) = (y * w * 4, ((y + pad) * pw + pad) * 4);
        if let (Some(src), Some(dst)) =
            (data.get(from..from + w * 4), padded.get_mut(to..to + w * 4))
        {
            dst.copy_from_slice(src);
        }
    }
    let mut tmp = vec![0u8; len];
    for r in radii {
        if r == 0 {
            continue;
        }
        box_pass(&padded, &mut tmp, pw, ph, r, true);
        box_pass(&tmp, &mut padded, pw, ph, r, false);
    }
    for y in 0..h {
        let (to, from) = (y * w * 4, ((y + pad) * pw + pad) * 4);
        if let (Some(src), Some(dst)) =
            (padded.get(from..from + w * 4), data.get_mut(to..to + w * 4))
        {
            dst.copy_from_slice(src);
        }
    }
}

fn box_pass(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize, horizontal: bool) {
    let (outer, inner) = if horizontal { (h, w) } else { (w, h) };
    let idx = |o: usize, i: usize| -> usize {
        if horizontal {
            (o * w + i) * 4
        } else {
            (i * w + o) * 4
        }
    };
    let span = (2 * r + 1) as u32;
    let px = |data: &[u8], k: usize| -> [u32; 4] {
        let mut out = [0u32; 4];
        for (c, slot) in out.iter_mut().enumerate() {
            *slot = u32::from(data.get(k + c).copied().unwrap_or(0));
        }
        out
    };
    for o in 0..outer {
        let mut acc = [0u32; 4];
        for i in 0..=r.min(inner.saturating_sub(1)) {
            let v = px(src, idx(o, i));
            for (a, v) in acc.iter_mut().zip(v) {
                *a += v;
            }
        }
        for i in 0..inner {
            let k = idx(o, i);
            for (c, a) in acc.iter().enumerate() {
                if let Some(slot) = dst.get_mut(k + c) {
                    *slot = ((a + span / 2) / span).min(255) as u8;
                }
            }
            let add = i + r + 1;
            if add < inner {
                let v = px(src, idx(o, add));
                for (a, v) in acc.iter_mut().zip(v) {
                    *a += v;
                }
            }
            if i >= r {
                let v = px(src, idx(o, i - r));
                for (a, v) in acc.iter_mut().zip(v) {
                    *a = a.saturating_sub(v);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_keeps_its_strength_at_the_edges() {
        let mut p = Pixmap::new(60, 60).unwrap();
        for y in 0..60 {
            for x in 0..7 {
                let k = (y * 60 + x) * 4;
                p.data_mut()[k..k + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
        blur(&mut p, 8.0);
        let a = p.data()[(30 * 60 + 1) * 4 + 3];
        assert!(a >= 137, "{a}: the blur fades at the edge");
    }

    #[test]
    fn blur_spreads_and_preserves_mass_roughly() {
        let mut p = Pixmap::new(41, 41).unwrap();
        let k = (20 * 41 + 20) * 4;
        p.data_mut()[k..k + 4].copy_from_slice(&[255, 255, 255, 255]);
        let before: u32 = p
            .data()
            .iter()
            .skip(3)
            .step_by(4)
            .map(|a| u32::from(*a))
            .sum();
        blur(&mut p, 8.0);
        let after: u32 = p
            .data()
            .iter()
            .skip(3)
            .step_by(4)
            .map(|a| u32::from(*a))
            .sum();
        assert!(p.data()[k + 3] < 255);
        assert!(p.data()[((20 * 41) + 22) * 4 + 3] > 0);
        assert!(after * 3 > before, "{after} vs {before}");
    }
}
