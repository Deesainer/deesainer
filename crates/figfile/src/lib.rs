use std::collections::BTreeMap;
use std::io::Read;

pub const MAX_BYTES: usize = 512 * 1024 * 1024;
const MAX_DEPTH: usize = 64;
const MAX_ARRAY: usize = 16 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum FigError {
    #[error("not a .fig file")]
    NotFig,
    #[error("the archive has no canvas.fig")]
    NoCanvas,
    #[error("archive: {0}")]
    Zip(String),
    #[error("decompression failed: {0}")]
    Decompress(String),
    #[error("truncated or malformed data at byte {0}")]
    Malformed(usize),
    #[error("schema: {0}")]
    Schema(String),
    #[error("over the size limit")]
    TooLarge,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    Str(String),
    Bytes(Vec<u8>),
    Enum(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn path(&self, keys: &[&str]) -> Option<&Value> {
        keys.iter().try_fold(self, |v, k| v.get(k))
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Float(f) if f.is_finite() => Some(*f),
            Value::Int(i) => Some(*i as f64),
            Value::UInt(u) => Some(*u as f64),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::UInt(u) => Some(*u),
            Value::Int(i) if *i >= 0 => Some(*i as u64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) | Value::Enum(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> &[Value] {
        match self {
            Value::Array(a) => a,
            _ => &[],
        }
    }

    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Bytes(b) => Some(b),
            _ => None,
        }
    }

    pub fn num(&self, key: &str, default: f64) -> f64 {
        self.get(key).and_then(Value::as_f64).unwrap_or(default)
    }
}

#[derive(Debug, Clone)]
pub struct FigFile {
    pub magic: String,
    pub version: u32,
    pub message: Value,
    pub blobs: Vec<Vec<u8>>,
    pub images: BTreeMap<String, Vec<u8>>,
    pub meta: Option<String>,
}

pub fn read(bytes: &[u8]) -> Result<FigFile, FigError> {
    if bytes.starts_with(b"PK") {
        read_archive(bytes)
    } else {
        read_canvas(bytes, BTreeMap::new(), None)
    }
}

fn read_archive(bytes: &[u8]) -> Result<FigFile, FigError> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| FigError::Zip(e.to_string()))?;
    let mut canvas = None;
    let mut images = BTreeMap::new();
    let mut meta = None;
    let mut total = 0usize;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| FigError::Zip(e.to_string()))?;
        if f.is_dir() {
            continue;
        }
        let name = f
            .name()
            .map_err(|e| FigError::Zip(e.to_string()))?
            .into_owned();
        let mut buf = Vec::new();
        (&mut f)
            .take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut buf)
            .map_err(|e| FigError::Zip(e.to_string()))?;
        total = total.saturating_add(buf.len());
        if buf.len() > MAX_BYTES || total > MAX_BYTES * 2 {
            return Err(FigError::TooLarge);
        }
        if name == "canvas.fig" {
            canvas = Some(buf);
        } else if name == "meta.json" {
            meta = String::from_utf8(buf).ok();
        } else if let Some(img) = name.strip_prefix("images/") {
            if !img.is_empty() && !img.contains('/') {
                images.insert(img.to_owned(), buf);
            }
        }
    }
    let canvas = canvas.ok_or(FigError::NoCanvas)?;
    read_canvas(&canvas, images, meta)
}

fn read_canvas(
    bytes: &[u8],
    images: BTreeMap<String, Vec<u8>>,
    meta: Option<String>,
) -> Result<FigFile, FigError> {
    if bytes.len() < 12 || !bytes[..8].iter().all(|b| b.is_ascii_graphic()) {
        return Err(FigError::NotFig);
    }
    let magic = String::from_utf8_lossy(&bytes[..8]).into_owned();
    if !magic.starts_with("fig") {
        return Err(FigError::NotFig);
    }
    let version = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    let mut chunks = Vec::new();
    let mut off = 12usize;
    while off + 4 <= bytes.len() {
        let n = u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]])
            as usize;
        off += 4;
        let end = off.checked_add(n).ok_or(FigError::Malformed(off))?;
        let chunk = bytes.get(off..end).ok_or(FigError::Malformed(off))?;
        chunks.push(chunk);
        off = end;
    }
    let (schema_raw, data_raw) = match chunks.as_slice() {
        [s, d, ..] => (*s, *d),
        _ => return Err(FigError::Malformed(off)),
    };
    let schema = Schema::parse(&decompress(schema_raw)?)?;
    let data = decompress(data_raw)?;
    let root = schema
        .defs
        .iter()
        .position(|d| d.name == "Message")
        .ok_or_else(|| FigError::Schema("no Message definition".into()))?;
    let mut r = Reader::new(&data);
    let mut message = schema.decode(&mut r, root, 0)?;
    let mut blobs = Vec::new();
    if let Value::Object(fields) = &mut message {
        if let Some((_, Value::Array(list))) = fields.iter_mut().find(|(k, _)| k == "blobs") {
            for b in list.drain(..) {
                blobs.push(
                    b.get("bytes")
                        .and_then(Value::as_bytes)
                        .unwrap_or(&[])
                        .to_vec(),
                );
            }
        }
    }
    Ok(FigFile {
        magic,
        version,
        message,
        blobs,
        images,
        meta,
    })
}

fn decompress(chunk: &[u8]) -> Result<Vec<u8>, FigError> {
    if chunk.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) {
        let mut src = chunk;
        let dec = ruzstd::decoding::StreamingDecoder::new(&mut src)
            .map_err(|e| FigError::Decompress(e.to_string()))?;
        let mut out = Vec::new();
        dec.take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut out)
            .map_err(|e| FigError::Decompress(e.to_string()))?;
        if out.len() > MAX_BYTES {
            return Err(FigError::TooLarge);
        }
        Ok(out)
    } else {
        miniz_oxide::inflate::decompress_to_vec_with_limit(chunk, MAX_BYTES)
            .map_err(|e| FigError::Decompress(format!("{e:?}")))
    }
}

struct Reader<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, i: 0 }
    }

    fn byte(&mut self) -> Result<u8, FigError> {
        let v = *self.b.get(self.i).ok_or(FigError::Malformed(self.i))?;
        self.i += 1;
        Ok(v)
    }

    fn var_u32(&mut self) -> Result<u32, FigError> {
        let mut v: u32 = 0;
        let mut shift = 0;
        loop {
            let c = self.byte()?;
            v |= u32::from(c & 127) << shift;
            shift += 7;
            if c & 128 == 0 || shift >= 35 {
                return Ok(v);
            }
        }
    }

    fn var_i32(&mut self) -> Result<i32, FigError> {
        let v = self.var_u32()?;
        Ok(if v & 1 == 0 {
            (v >> 1) as i32
        } else {
            !((v >> 1) as i32)
        })
    }

    fn var_u64(&mut self) -> Result<u64, FigError> {
        let mut v: u64 = 0;
        let mut shift = 0;
        loop {
            let c = self.byte()?;
            v |= u64::from(c & 127) << shift;
            shift += 7;
            if c & 128 == 0 || shift >= 70 {
                return Ok(v);
            }
        }
    }

    fn var_i64(&mut self) -> Result<i64, FigError> {
        let v = self.var_u64()?;
        Ok(if v & 1 == 0 {
            (v >> 1) as i64
        } else {
            !((v >> 1) as i64)
        })
    }

    fn var_f32(&mut self) -> Result<f32, FigError> {
        if *self.b.get(self.i).ok_or(FigError::Malformed(self.i))? == 0 {
            self.i += 1;
            return Ok(0.0);
        }
        let s = self
            .b
            .get(self.i..self.i + 4)
            .ok_or(FigError::Malformed(self.i))?;
        self.i += 4;
        let bits = u32::from_le_bytes([s[0], s[1], s[2], s[3]]);
        Ok(f32::from_bits(bits.rotate_left(23)))
    }

    fn string(&mut self) -> Result<String, FigError> {
        let rest = self.b.get(self.i..).ok_or(FigError::Malformed(self.i))?;
        let n = rest
            .iter()
            .position(|c| *c == 0)
            .ok_or(FigError::Malformed(self.i))?;
        let s = String::from_utf8_lossy(&rest[..n]).into_owned();
        self.i += n + 1;
        Ok(s)
    }

    fn bytes(&mut self, n: usize) -> Result<Vec<u8>, FigError> {
        let s = self
            .b
            .get(self.i..self.i + n)
            .ok_or(FigError::Malformed(self.i))?;
        self.i += n;
        Ok(s.to_vec())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Enum,
    Struct,
    Message,
}

#[derive(Debug, Clone)]
struct Field {
    name: String,
    ty: i32,
    array: bool,
    value: u32,
}

#[derive(Debug, Clone)]
struct Def {
    name: String,
    kind: Kind,
    fields: Vec<Field>,
}

#[derive(Debug, Clone)]
pub struct Schema {
    defs: Vec<Def>,
}

impl Schema {
    pub fn parse(b: &[u8]) -> Result<Self, FigError> {
        let mut r = Reader::new(b);
        let n = r.var_u32()? as usize;
        if n > 100_000 {
            return Err(FigError::Schema("too many definitions".into()));
        }
        let mut defs = Vec::with_capacity(n);
        for _ in 0..n {
            let name = r.string()?;
            let kind = match r.byte()? {
                0 => Kind::Enum,
                1 => Kind::Struct,
                2 => Kind::Message,
                k => return Err(FigError::Schema(format!("unknown kind {k}"))),
            };
            let fc = r.var_u32()? as usize;
            if fc > 100_000 {
                return Err(FigError::Schema("too many fields".into()));
            }
            let mut fields = Vec::with_capacity(fc);
            for _ in 0..fc {
                let fname = r.string()?;
                let ty = r.var_i32()?;
                let array = r.byte()? != 0;
                let value = r.var_u32()?;
                fields.push(Field {
                    name: fname,
                    ty,
                    array,
                    value,
                });
            }
            defs.push(Def { name, kind, fields });
        }
        for d in &defs {
            for f in &d.fields {
                if d.kind != Kind::Enum && (f.ty < -8 || f.ty as usize >= defs.len() && f.ty >= 0) {
                    return Err(FigError::Schema(format!(
                        "bad type in {}.{}",
                        d.name, f.name
                    )));
                }
            }
        }
        Ok(Self { defs })
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.defs.iter().map(|d| d.name.as_str())
    }

    fn builtin(&self, r: &mut Reader<'_>, ty: i32) -> Result<Value, FigError> {
        Ok(match ty {
            -1 => Value::Bool(r.byte()? != 0),
            -2 => Value::UInt(u64::from(r.byte()?)),
            -3 => Value::Int(i64::from(r.var_i32()?)),
            -4 => Value::UInt(u64::from(r.var_u32()?)),
            -5 => Value::Float(f64::from(r.var_f32()?)),
            -6 => Value::Str(r.string()?),
            -7 => Value::Int(r.var_i64()?),
            -8 => Value::UInt(r.var_u64()?),
            _ => return Err(FigError::Schema(format!("unknown builtin {ty}"))),
        })
    }

    fn field(&self, r: &mut Reader<'_>, f: &Field, depth: usize) -> Result<Value, FigError> {
        let one = |s: &Self, r: &mut Reader<'_>| -> Result<Value, FigError> {
            if f.ty < 0 {
                s.builtin(r, f.ty)
            } else {
                s.decode(r, f.ty as usize, depth + 1)
            }
        };
        if !f.array {
            return one(self, r);
        }
        let n = r.var_u32()? as usize;
        if n > MAX_ARRAY || n > r.b.len().saturating_sub(r.i) {
            return Err(FigError::Malformed(r.i));
        }
        if f.ty == -2 {
            return Ok(Value::Bytes(r.bytes(n)?));
        }
        let mut out = Vec::with_capacity(n.min(4096));
        for _ in 0..n {
            out.push(one(self, r)?);
        }
        Ok(Value::Array(out))
    }

    fn decode(&self, r: &mut Reader<'_>, index: usize, depth: usize) -> Result<Value, FigError> {
        if depth > MAX_DEPTH {
            return Err(FigError::Malformed(r.i));
        }
        let def = self
            .defs
            .get(index)
            .ok_or_else(|| FigError::Schema(format!("no definition {index}")))?;
        match def.kind {
            Kind::Enum => {
                let v = r.var_u32()?;
                Ok(def.fields.iter().find(|f| f.value == v).map_or_else(
                    || Value::UInt(u64::from(v)),
                    |f| Value::Enum(f.name.clone()),
                ))
            }
            Kind::Struct => {
                let mut out = Vec::with_capacity(def.fields.len());
                for f in &def.fields {
                    out.push((f.name.clone(), self.field(r, f, depth)?));
                }
                Ok(Value::Object(out))
            }
            Kind::Message => {
                let mut out = Vec::new();
                loop {
                    let id = r.var_u32()?;
                    if id == 0 {
                        break;
                    }
                    let f = def
                        .fields
                        .iter()
                        .find(|f| f.value == id)
                        .ok_or(FigError::Malformed(r.i))?;
                    out.push((f.name.clone(), self.field(r, f, depth)?));
                }
                Ok(Value::Object(out))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathCmd {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    QuadTo(f32, f32, f32, f32),
    CubicTo(f32, f32, f32, f32, f32, f32),
    Close,
}

pub fn path_commands(blob: &[u8]) -> Vec<PathCmd> {
    let mut out = Vec::new();
    let mut i = 0;
    let f = |i: usize| -> Option<f32> {
        blob.get(i..i + 4)
            .map(|s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    };
    while let Some(&c) = blob.get(i) {
        i += 1;
        let n = match c {
            0 => 0,
            1 | 2 => 2,
            3 => 4,
            4 => 6,
            _ => break,
        };
        let vals: Option<Vec<f32>> = (0..n).map(|k| f(i + 4 * k)).collect();
        let Some(v) = vals else { break };
        i += 4 * n;
        out.push(match c {
            0 => PathCmd::Close,
            1 => PathCmd::MoveTo(v[0], v[1]),
            2 => PathCmd::LineTo(v[0], v[1]),
            3 => PathCmd::QuadTo(v[0], v[1], v[2], v[3]),
            _ => PathCmd::CubicTo(v[0], v[1], v[2], v[3], v[4], v[5]),
        });
    }
    out
}

pub fn network_segments(blob: &[u8]) -> Vec<[(f32, f32); 4]> {
    let u = |i: usize| -> Option<u32> {
        blob.get(i..i + 4)
            .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    };
    let f = |i: usize| u(i).map(f32::from_bits);
    let (Some(nv), Some(ns)) = (u(0), u(4)) else {
        return Vec::new();
    };
    let (nv, ns) = (nv as usize, ns as usize);
    if nv > 1_000_000 || ns > 1_000_000 {
        return Vec::new();
    }
    let vbase = 12;
    let vertex = |k: usize| -> Option<(f32, f32)> {
        let o = vbase + k * 12;
        Some((f(o + 4)?, f(o + 8)?))
    };
    let sbase = vbase + nv * 12;
    let mut out = Vec::new();
    for s in 0..ns {
        let o = sbase + s * 28;
        let (Some(a), Some(tax), Some(tay), Some(b), Some(tbx), Some(tby)) = (
            u(o + 4),
            f(o + 8),
            f(o + 12),
            u(o + 16),
            f(o + 20),
            f(o + 24),
        ) else {
            break;
        };
        let (Some(pa), Some(pb)) = (vertex(a as usize), vertex(b as usize)) else {
            continue;
        };
        out.push([pa, (pa.0 + tax, pa.1 + tay), (pb.0 + tbx, pb.1 + tby), pb]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var_u32(out: &mut Vec<u8>, mut v: u32) {
        loop {
            let b = (v & 127) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                return;
            }
            out.push(b | 128);
        }
    }
    fn var_i32(out: &mut Vec<u8>, v: i32) {
        var_u32(out, ((v << 1) ^ (v >> 31)) as u32);
    }
    fn string(out: &mut Vec<u8>, s: &str) {
        out.extend_from_slice(s.as_bytes());
        out.push(0);
    }
    fn var_f32(out: &mut Vec<u8>, v: f32) {
        if v == 0.0 {
            out.push(0);
            return;
        }
        out.extend_from_slice(&v.to_bits().rotate_right(23).to_le_bytes());
    }

    fn sample() -> Vec<u8> {
        let mut s = Vec::new();
        var_u32(&mut s, 3);
        string(&mut s, "Kind");
        s.push(0);
        var_u32(&mut s, 2);
        for (n, v) in [("A", 0), ("B", 1)] {
            string(&mut s, n);
            var_i32(&mut s, 0);
            s.push(0);
            var_u32(&mut s, v);
        }
        string(&mut s, "Size");
        s.push(1);
        var_u32(&mut s, 2);
        for n in ["x", "y"] {
            string(&mut s, n);
            var_i32(&mut s, -5);
            s.push(0);
            var_u32(&mut s, 0);
        }
        string(&mut s, "Message");
        s.push(2);
        var_u32(&mut s, 5);
        for (n, ty, arr, id) in [
            ("name", -6, 0, 1),
            ("kind", 0, 0, 2),
            ("size", 1, 0, 3),
            ("data", -2, 1, 4),
            ("list", 1, 1, 5),
        ] {
            string(&mut s, n);
            var_i32(&mut s, ty);
            s.push(arr);
            var_u32(&mut s, id);
        }
        let mut d = Vec::new();
        var_u32(&mut d, 1);
        string(&mut d, "Hello");
        var_u32(&mut d, 2);
        var_u32(&mut d, 1);
        var_u32(&mut d, 3);
        var_f32(&mut d, 1.5);
        var_f32(&mut d, 0.0);
        var_u32(&mut d, 4);
        var_u32(&mut d, 3);
        d.extend_from_slice(&[9, 8, 7]);
        var_u32(&mut d, 5);
        var_u32(&mut d, 1);
        var_f32(&mut d, -2.25);
        var_f32(&mut d, 100.0);
        var_u32(&mut d, 0);
        let mut file = b"fig-kiwi".to_vec();
        file.extend_from_slice(&15u32.to_le_bytes());
        for chunk in [s, d] {
            let c = miniz_oxide::deflate::compress_to_vec(&chunk, 6);
            file.extend_from_slice(&(c.len() as u32).to_le_bytes());
            file.extend_from_slice(&c);
        }
        file
    }

    #[test]
    fn decodes_a_self_described_document() {
        let f = read(&sample()).unwrap();
        assert_eq!(f.magic, "fig-kiwi");
        assert_eq!(f.version, 15);
        let m = &f.message;
        assert_eq!(m.get("name").and_then(Value::as_str), Some("Hello"));
        assert_eq!(m.get("kind").and_then(Value::as_str), Some("B"));
        assert_eq!(m.path(&["size", "x"]).and_then(Value::as_f64), Some(1.5));
        assert_eq!(m.path(&["size", "y"]).and_then(Value::as_f64), Some(0.0));
        assert_eq!(
            m.get("data").and_then(Value::as_bytes),
            Some(&[9u8, 8, 7][..])
        );
        let list = m.get("list").unwrap().as_array();
        assert_eq!(list[0].num("x", 0.0), -2.25);
        assert_eq!(list[0].num("y", 0.0), 100.0);
    }

    #[test]
    fn non_finite_numbers_read_as_missing() {
        let n = Value::Object(vec![
            ("gap".into(), Value::Float(f64::NAN)),
            ("size".into(), Value::Float(f64::INFINITY)),
            ("x".into(), Value::Float(2.5)),
        ]);
        assert_eq!(n.num("gap", 0.0), 0.0);
        assert_eq!(n.get("size").and_then(Value::as_f64), None);
        assert_eq!(n.num("x", 0.0), 2.5);
    }

    #[test]
    fn truncated_input_is_an_error_not_a_panic() {
        let full = sample();
        for n in 0..full.len() {
            let _ = read(&full[..n]);
        }
        let mut bad = full.clone();
        for i in (12..bad.len()).step_by(3) {
            bad[i] ^= 0x5A;
            let _ = read(&bad);
        }
    }

    #[test]
    fn path_blob_round_trip() {
        let mut b = vec![1u8];
        for v in [0.0f32, 0.0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.push(2);
        for v in [10.0f32, 5.0] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.push(0);
        assert_eq!(
            path_commands(&b),
            vec![
                PathCmd::MoveTo(0.0, 0.0),
                PathCmd::LineTo(10.0, 5.0),
                PathCmd::Close
            ]
        );
    }
}
