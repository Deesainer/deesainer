use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const ZERO: Point = Point::new(0.0, 0.0);

    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    pub fn distance(self, other: Point) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }

    pub fn lerp(self, other: Point, t: f64) -> Point {
        Point::new(
            self.x + (other.x - self.x) * t,
            self.y + (other.y - self.y) * t,
        )
    }
}

impl std::ops::Add for Point {
    type Output = Point;
    fn add(self, o: Point) -> Point {
        Point::new(self.x + o.x, self.y + o.y)
    }
}

impl std::ops::Sub for Point {
    type Output = Point;
    fn sub(self, o: Point) -> Point {
        Point::new(self.x - o.x, self.y - o.y)
    }
}

impl std::ops::Mul<f64> for Point {
    type Output = Point;
    fn mul(self, s: f64) -> Point {
        Point::new(self.x * s, self.y * s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn from_points(a: Point, b: Point) -> Self {
        Rect::new(
            a.x.min(b.x),
            a.y.min(b.y),
            (a.x - b.x).abs(),
            (a.y - b.y).abs(),
        )
    }

    pub fn is_valid(&self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width >= 0.0
            && self.height >= 0.0
    }

    pub fn origin(&self) -> Point {
        Point::new(self.x, self.y)
    }

    pub fn max_x(&self) -> f64 {
        self.x + self.width
    }

    pub fn max_y(&self) -> f64 {
        self.y + self.height
    }

    pub fn center(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    pub fn translate(self, dx: f64, dy: f64) -> Self {
        Self::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    pub fn with_origin(self, x: f64, y: f64) -> Self {
        Self::new(x, y, self.width, self.height)
    }

    pub fn with_size(self, width: f64, height: f64) -> Self {
        Self::new(self.x, self.y, width, height)
    }

    pub fn inflate(self, d: f64) -> Self {
        Self::new(
            self.x - d,
            self.y - d,
            (self.width + 2.0 * d).max(0.0),
            (self.height + 2.0 * d).max(0.0),
        )
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.max_x() && p.y < self.max_y()
    }

    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.max_x() && o.x < self.max_x() && self.y < o.max_y() && o.y < self.max_y()
    }

    pub fn contains_rect(&self, o: &Rect) -> bool {
        o.x >= self.x && o.y >= self.y && o.max_x() <= self.max_x() && o.max_y() <= self.max_y()
    }

    pub fn union(self, other: Rect) -> Rect {
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = self.max_x().max(other.max_x());
        let y1 = self.max_y().max(other.max_y());
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    pub fn bounding(points: impl IntoIterator<Item = Point>) -> Option<Rect> {
        let mut it = points.into_iter();
        let first = it.next()?;
        let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x, first.y);
        for p in it {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
    }

    pub fn corners(&self) -> [Point; 4] {
        [
            Point::new(self.x, self.y),
            Point::new(self.max_x(), self.y),
            Point::new(self.max_x(), self.max_y()),
            Point::new(self.x, self.max_y()),
        ]
    }

    pub fn lerp(self, other: Rect, t: f64) -> Rect {
        Rect::new(
            self.x + (other.x - self.x) * t,
            self.y + (other.y - self.y) * t,
            self.width + (other.width - self.width) * t,
            self.height + (other.height - self.height) * t,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Default for Affine {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    pub const fn translate(x: f64, y: f64) -> Self {
        Affine {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: x,
            f: y,
        }
    }

    pub const fn scale(sx: f64, sy: f64) -> Self {
        Affine {
            a: sx,
            b: 0.0,
            c: 0.0,
            d: sy,
            e: 0.0,
            f: 0.0,
        }
    }

    pub fn rotate(degrees: f64) -> Self {
        let (s, c) = degrees.to_radians().sin_cos();
        Affine {
            a: c,
            b: s,
            c: -s,
            d: c,
            e: 0.0,
            f: 0.0,
        }
    }

    pub fn rotate_about(degrees: f64, center: Point) -> Self {
        Affine::translate(center.x, center.y)
            .then(Affine::rotate(degrees))
            .then(Affine::translate(-center.x, -center.y))
    }

    pub fn then(self, o: Affine) -> Affine {
        Affine {
            a: self.a * o.a + self.c * o.b,
            b: self.b * o.a + self.d * o.b,
            c: self.a * o.c + self.c * o.d,
            d: self.b * o.c + self.d * o.d,
            e: self.a * o.e + self.c * o.f + self.e,
            f: self.b * o.e + self.d * o.f + self.f,
        }
    }

    pub fn apply(&self, p: Point) -> Point {
        Point::new(
            self.a * p.x + self.c * p.y + self.e,
            self.b * p.x + self.d * p.y + self.f,
        )
    }

    pub fn determinant(&self) -> f64 {
        self.a * self.d - self.b * self.c
    }

    pub fn invert(&self) -> Option<Affine> {
        let det = self.determinant();
        if det.abs() < 1e-12 || !det.is_finite() {
            return None;
        }
        let inv = 1.0 / det;
        Some(Affine {
            a: self.d * inv,
            b: -self.b * inv,
            c: -self.c * inv,
            d: self.a * inv,
            e: (self.c * self.f - self.d * self.e) * inv,
            f: (self.b * self.e - self.a * self.f) * inv,
        })
    }

    pub fn transform_rect_bounds(&self, r: &Rect) -> Rect {
        Rect::bounding(r.corners().map(|p| self.apply(p))).unwrap_or(*r)
    }

    pub fn as_array(&self) -> [f64; 6] {
        [self.a, self.b, self.c, self.d, self.e, self.f]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Point, b: Point) -> bool {
        a.distance(b) < 1e-9
    }

    #[test]
    fn contains_is_half_open() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(r.contains(Point::new(0.0, 0.0)));
        assert!(!r.contains(Point::new(10.0, 5.0)));
    }

    #[test]
    fn union_covers_both() {
        let u = Rect::new(0.0, 0.0, 10.0, 10.0).union(Rect::new(5.0, -5.0, 10.0, 5.0));
        assert_eq!(u, Rect::new(0.0, -5.0, 15.0, 15.0));
    }

    #[test]
    fn validity_rejects_negative_and_non_finite() {
        assert!(Rect::new(0.0, 0.0, 1.0, 1.0).is_valid());
        assert!(!Rect::new(0.0, 0.0, -1.0, 1.0).is_valid());
        assert!(!Rect::new(f64::INFINITY, 0.0, 1.0, 1.0).is_valid());
    }

    #[test]
    fn affine_compose_and_invert() {
        let t = Affine::translate(10.0, 5.0)
            .then(Affine::rotate(90.0))
            .then(Affine::scale(2.0, 2.0));
        let p = Point::new(1.0, 0.0);
        let q = t.apply(p);
        assert!(close(q, Point::new(10.0, 7.0)), "{q:?}");
        let back = t.invert().unwrap().apply(q);
        assert!(close(back, p));
        assert!(Affine::scale(0.0, 1.0).invert().is_none());
    }

    #[test]
    fn rotate_about_keeps_center() {
        let c = Point::new(5.0, 5.0);
        assert!(close(Affine::rotate_about(37.0, c).apply(c), c));
    }
}
