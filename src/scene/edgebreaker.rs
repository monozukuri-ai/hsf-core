//! Connectivity-compressed Shells (flag 0x40, compression scheme 5): an
//! Edgebreaker-style stream holding the opcodes, an auxiliary table and the
//! quantized points and normals. Only the profile observed in the Creo 11
//! eDrawings publisher is decoded: stream layout 2, opcodes C/L/E/R/S, dummy
//! vertices, and points quantized within a stored box. Merges (M, M2), patched
//! vertices and boxes supplied by the toolkit stop with explicit diagnostics.
use super::*;

const GARBAGE: i32 = 0x8080_8080_u32 as i32;
const DUMMY: i32 = 0x8000_0003_u32 as i32;
/// Escape-coded widths: a value of all ones moves to the next width.
const RESIDUAL_WIDTHS: [u32; 9] = [2, 6, 10, 14, 18, 22, 26, 31, 32];
const ABSOLUTE_WIDTHS: [u32; 3] = [16, 31, 32];

pub(super) struct Decoded {
    pub vertices: Vec<[f32; 3]>,
    /// Empty when the stream stores none.
    pub normals: Vec<[f32; 3]>,
    pub normal_bits: u8,
    /// HOOPS face list: `3, a, b, c` per triangle.
    pub face_list: Vec<i32>,
}

fn invalid(message: &str) -> HsfError {
    error("hsf.invalid_shell_connectivity", Status::Invalid, message)
}
fn unsupported(message: &str) -> HsfError {
    error(
        "hsf.shell_connectivity_feature",
        Status::Unsupported,
        message,
    )
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl Reader<'_> {
    fn i32(&mut self) -> Result<i32, HsfError> {
        let b = self
            .bytes
            .get(self.pos..self.pos + 4)
            .ok_or_else(|| invalid("Connectivity table extends beyond its section"))?;
        self.pos += 4;
        Ok(i32::from_le_bytes(b.try_into().expect("four bytes")))
    }
    fn count(&mut self) -> Result<usize, HsfError> {
        let n = self.i32()?;
        let n = usize::try_from(n).map_err(|_| invalid("Negative connectivity table count"))?;
        // Every entry takes at least four bytes of the section.
        if n > (self.bytes.len() - self.pos) / 4 {
            return Err(invalid("Connectivity table count exceeds its section"));
        }
        Ok(n)
    }
}

struct Table {
    dummies: Vec<i32>,
    bounds: [f32; 6],
    bits: [u32; 3],
    normal_bits: [u32; 3],
}

/// The auxiliary table, in the reference reader's order: flags, the counts
/// present, the arrays, then bounds and quantization.
fn table(bytes: &[u8]) -> Result<Table, HsfError> {
    let mut r = Reader { bytes, pos: 0 };
    let flags = r.i32()?;
    if flags & !0xff != 0 {
        return Err(unsupported("Unknown connectivity table flags"));
    }
    let lengths = if flags & 1 != 0 { r.count()? } else { 0 };
    let merges = if flags & 2 != 0 { r.count()? } else { 0 };
    let dummies = if flags & 8 != 0 { r.count()? } else { 0 };
    let patches = if flags & 0x10 != 0 { r.count()? } else { 0 };
    if merges > 0 || patches > 0 {
        return Err(unsupported(
            "Split-loop merges and patched vertices are not implemented",
        ));
    }
    for _ in 0..lengths {
        r.i32()?;
    }
    // Delta-coded, ascending.
    let mut acc = 0i32;
    let mut out = Vec::with_capacity(dummies);
    for _ in 0..dummies {
        acc = acc.wrapping_add(r.i32()?);
        out.push(acc);
    }
    if flags & 0x20 == 0 {
        return Err(unsupported(
            "Connectivity-compressed points without stored bounds are not implemented",
        ));
    }
    let mut bounds = [0f32; 6];
    for b in &mut bounds {
        *b = f32::from_bits(r.i32()? as u32);
    }
    if bounds.iter().any(|b| !b.is_finite()) || (0..3).any(|i| bounds[i] > bounds[i + 3]) {
        return Err(invalid(
            "Connectivity point bounds are not finite and ordered",
        ));
    }
    let mut bits_of = |present: bool| -> Result<[u32; 3], HsfError> {
        if !present {
            return Ok([11; 3]);
        }
        let mut bits = [0u32; 3];
        for b in &mut bits {
            *b = u32::try_from(r.i32()?)
                .ok()
                .filter(|b| (1..=31).contains(b))
                .ok_or_else(|| unsupported("Quantization widths must be 1..31 bits"))?;
        }
        Ok(bits)
    };
    let bits = bits_of(flags & 0x40 != 0)?;
    let normal_bits = bits_of(flags & 0x80 != 0)?;
    Ok(Table {
        dummies: out,
        bounds,
        bits,
        normal_bits,
    })
}

/// Per component: the initial boundary loop length and, for each split (S),
/// how far along the loop its third vertex lies. The component ends at the
/// closing (E) that finds no pending split.
fn prepass(ops: &[u8]) -> Result<(usize, Vec<i64>), HsfError> {
    let (mut d, mut splits) = (0i64, 0usize);
    let (mut ids, mut depths, mut offsets) = (vec![], vec![], vec![]);
    for &op in ops {
        match op {
            0 => d -= 1,
            1 | 3 => d += 1,
            2 => {
                d += 3;
                let Some(depth) = depths.pop() else {
                    break;
                };
                let id: usize = ids.pop().expect("paired with depths");
                offsets[id] = d - depth - 2;
            }
            4 => {
                ids.push(splits);
                d -= 1;
                depths.push(d);
                offsets.push(0);
                splits += 1;
            }
            _ => {
                return Err(unsupported(
                    "Merge (M, M2) connectivity opcodes are not implemented",
                ));
            }
        }
    }
    let length = usize::try_from(d)
        .ok()
        .filter(|l| (3..=ops.len().saturating_mul(3).saturating_add(3)).contains(l))
        .ok_or_else(|| invalid("Connectivity component has an impossible boundary length"))?;
    Ok((length, offsets))
}

struct Connectivity {
    faces: Vec<[i32; 3]>,
    /// Half-edges, three per triangle: start vertex and twin (GARBAGE on the boundary).
    edges: Vec<[i32; 2]>,
    vertices: usize,
}

/// Rebuild triangles on a doubly linked boundary loop: each opcode closes one
/// triangle against the gate edge; splits (S) park a second loop for later.
fn connectivity(ops: &[u8], budget: &mut Budget) -> Result<Connectivity, HsfError> {
    budget.charge_output(ops.len() as u64 * 48)?;
    let mut edges: Vec<[i32; 2]> = Vec::with_capacity(ops.len() * 3);
    let mut faces = Vec::with_capacity(ops.len());
    let mut vertices = 0usize;
    let mut k = 0usize;
    let vid = |v: usize| i32::try_from(v).map_err(|_| invalid("Too many connectivity vertices"));
    let link = |edges: &mut Vec<[i32; 2]>, e: i32, to: usize| {
        if e != GARBAGE {
            edges[e as usize][1] = to as i32;
        }
    };
    while k < ops.len() {
        let (length, offsets) = prepass(&ops[k..])?;
        budget.charge_output(length as u64 * 32)?;
        let mut next: Vec<usize> = (1..=length).collect();
        next[length - 1] = 0;
        let mut prev: Vec<usize> = (0..length).map(|i| (i + length - 1) % length).collect();
        let mut vertex: Vec<i32> = (0..length)
            .map(|i| vid(vertices + i))
            .collect::<Result<_, _>>()?;
        let mut edge = vec![GARBAGE; length];
        vertices += length;
        let (mut free, mut parked): (Vec<usize>, Vec<usize>) = (vec![], vec![]);
        let mut gate = 0usize;
        let mut split = 0usize;
        loop {
            let op = *ops
                .get(k)
                .ok_or_else(|| invalid("Connectivity opcodes end inside a component"))?;
            k += 1;
            if free.is_empty() {
                budget.charge_output(32)?;
                free.push(next.len());
                next.push(0);
                prev.push(0);
                vertex.push(0);
                edge.push(GARBAGE);
            }
            let t = edges.len();
            let (a, b) = (vertex[gate], vertex[next[gate]]);
            link(&mut edges, edge[gate], t);
            edges.extend([[a, edge[gate]], [b, GARBAGE], [GARBAGE, GARBAGE]]);
            let mut done = false;
            let c = match op {
                // C: a new vertex; the gate keeps its edge to the next vertex.
                0 => {
                    let c = vid(vertices)?;
                    vertices += 1;
                    edges[t + 2][0] = c;
                    let n = free.pop().expect("refilled above");
                    let p = prev[gate];
                    next[p] = n;
                    prev[n] = p;
                    next[n] = gate;
                    prev[gate] = n;
                    vertex[n] = vertex[gate];
                    vertex[gate] = c;
                    edge[gate] = (t + 1) as i32;
                    edge[n] = (t + 2) as i32;
                    c
                }
                // L: the triangle takes the previous vertex.
                1 => {
                    let p = prev[gate];
                    let c = vertex[p];
                    edges[t + 2] = [c, edge[p]];
                    link(&mut edges, edge[p], t + 2);
                    vertex[gate] = c;
                    free.push(p);
                    let pp = prev[p];
                    prev[gate] = pp;
                    next[pp] = gate;
                    edge[gate] = (t + 1) as i32;
                    c
                }
                // E: closes the loop; continue with a parked one, if any.
                2 => {
                    let (p, n) = (prev[gate], next[gate]);
                    let c = vertex[p];
                    edges[t + 1][1] = edge[n];
                    link(&mut edges, edge[n], t + 1);
                    edges[t + 2] = [c, edge[p]];
                    link(&mut edges, edge[p], t + 2);
                    free.extend([gate, p, n]);
                    match parked.pop() {
                        Some(g) => gate = g,
                        None => done = true,
                    }
                    c
                }
                // R: the triangle takes the vertex after the next.
                3 => {
                    let n = next[gate];
                    let nn = next[n];
                    let c = vertex[nn];
                    edges[t + 1][1] = edge[n];
                    link(&mut edges, edge[n], t + 1);
                    edges[t + 2] = [c, GARBAGE];
                    free.push(n);
                    next[gate] = nn;
                    prev[nn] = gate;
                    edge[gate] = (t + 2) as i32;
                    c
                }
                // S: the third vertex lies further along; the loop splits in two.
                4 => {
                    let offset = *offsets
                        .get(split)
                        .ok_or_else(|| invalid("Split without a precomputed offset"))?;
                    split += 1;
                    let steps = usize::try_from(offset)
                        .ok()
                        .filter(|s| *s < next.len())
                        .ok_or_else(|| invalid("Split offset leaves the boundary loop"))?;
                    let mut w = gate;
                    for _ in 0..=steps {
                        w = next[w];
                    }
                    let nw = next[w];
                    let c = vertex[nw];
                    edges[t + 2] = [c, GARBAGE];
                    let n = free.pop().expect("refilled above");
                    parked.push(n);
                    let p = prev[gate];
                    next[p] = n;
                    prev[n] = p;
                    next[n] = nw;
                    prev[nw] = n;
                    edge[n] = (t + 2) as i32;
                    vertex[n] = vertex[gate];
                    vertex[gate] = c;
                    edge[gate] = (t + 1) as i32;
                    prev[gate] = w;
                    next[w] = gate;
                    c
                }
                _ => return Err(unsupported("Unknown connectivity opcode")),
            };
            if a == b || b == c || c == a {
                return Err(invalid("Connectivity produced a degenerate triangle"));
            }
            faces.push([a, b, c]);
            if done {
                break;
            }
        }
    }
    Ok(Connectivity {
        faces,
        edges,
        vertices,
    })
}

/// Drop dummy vertices (hole plugs) and the triangles touching them; renumber
/// the remaining vertices densely.
fn remove_dummies(mut conn: Connectivity, dummies: &[i32]) -> Result<Kept, HsfError> {
    let mut dummy = vec![false; conn.vertices];
    for &d in dummies {
        let d = usize::try_from(d)
            .ok()
            .filter(|d| *d < conn.vertices)
            .ok_or_else(|| invalid("Dummy vertex out of range"))?;
        dummy[d] = true;
    }
    let mut shift = vec![0i32; conn.vertices];
    let mut removed = 0i32;
    for v in 0..conn.vertices {
        if dummy[v] {
            removed += 1;
        } else {
            shift[v] = removed;
        }
    }
    let faces = conn
        .faces
        .into_iter()
        .filter(|f| f.iter().all(|v| !dummy[*v as usize]))
        .map(|f| f.map(|v| v - shift[v as usize]))
        .collect();
    let edges = &mut conn.edges;
    let mut i = 0;
    while i < edges.len() {
        let s = edges[i][0];
        if s >= 0 && dummy[s as usize] {
            let base = i - i % 3;
            for j in base..base + 3 {
                let twin = edges[j][1];
                if twin != GARBAGE {
                    edges[twin as usize][1] = GARBAGE;
                }
                edges[j] = [DUMMY, GARBAGE];
            }
            i = base + 3;
            continue;
        }
        if s >= 0 {
            edges[i][0] = s - shift[s as usize];
        }
        i += 1;
    }
    Ok((faces, conn.edges, conn.vertices - removed as usize))
}

/// Big-endian bit fields in little-endian 32-bit words, escape coded.
struct Bits<'a> {
    bytes: &'a [u8],
    word: usize,
    bit: u32,
}
impl Bits<'_> {
    fn word(&self, i: usize) -> Result<u32, HsfError> {
        self.bytes
            .get(i * 4..i * 4 + 4)
            .map(|b| u32::from_le_bytes(b.try_into().expect("four bytes")))
            .ok_or_else(|| invalid("Connectivity points end early"))
    }
    fn get(&mut self, widths: &[u32]) -> Result<i32, HsfError> {
        for &width in widths {
            let mask = if width == 32 {
                u32::MAX
            } else {
                (1 << width) - 1
            };
            let end = self.bit + width;
            let mut v = self.word(self.word)?;
            if end <= 32 {
                v = v.checked_shr(32 - end).unwrap_or(0) & mask;
            } else {
                v = v.checked_shl(end - 32).unwrap_or(0) & mask;
                self.word += 1;
                v |= self.word(self.word)?.checked_shr(64 - end).unwrap_or(0);
            }
            self.bit = if end > 32 { end - 32 } else { end };
            if v == mask && width < 32 {
                continue;
            }
            // Biased by 2^(width-1)-1, so small negative residuals stay short.
            let bias = (1u32 << (width - 1)) - 1;
            return Ok(v.wrapping_sub(bias) as i32);
        }
        Err(invalid("Connectivity value escapes past the widest field"))
    }
    fn triples(&mut self, n: usize, widths: &[u32]) -> Result<Vec<[i32; 3]>, HsfError> {
        (0..n)
            .map(|_| Ok([self.get(widths)?, self.get(widths)?, self.get(widths)?]))
            .collect()
    }
}

/// Triangles, half-edges and vertex count once hole plugs are removed.
type Kept = (Vec<[i32; 3]>, Vec<[i32; 2]>, usize);
/// Quantized X, Y and Z codes per vertex.
type Quantized = Vec<[i32; 3]>;

fn next_edge(i: usize) -> usize {
    i - i % 3 + (i + 1) % 3
}
fn prev_edge(i: usize) -> usize {
    i - i % 3 + (i + 2) % 3
}

/// Quantized points (and normals), in the order a depth-first walk over the
/// triangles first reaches them: each component's first triangle is stored
/// absolutely, every later vertex as a residual from the parallelogram
/// prediction across the edge it is reached through.
fn points(
    bytes: &[u8],
    edges: &[[i32; 2]],
    count: usize,
    normals: bool,
    budget: &mut Budget,
) -> Result<(Quantized, Quantized), HsfError> {
    let mut bits = Bits {
        bytes,
        word: 0,
        bit: 0,
    };
    // The first value announces trailing strip data, which the triangles make redundant.
    bits.get(&RESIDUAL_WIDTHS)?;
    let mut amount = || -> Result<usize, HsfError> {
        usize::try_from(bits.get(&ABSOLUTE_WIDTHS)?)
            .ok()
            .filter(|n| *n <= count)
            .ok_or_else(|| invalid("Connectivity point count out of range"))
    };
    let (absolute, residual) = (amount()?, amount()?);
    budget.charge_output((absolute + residual) as u64 * 24 + count as u64 * 32)?;
    let abs_points = bits.triples(absolute, &ABSOLUTE_WIDTHS)?;
    let res_points = bits.triples(residual, &RESIDUAL_WIDTHS)?;
    let (abs_normals, res_normals) = if normals {
        (
            bits.triples(absolute, &ABSOLUTE_WIDTHS)?,
            bits.triples(residual, &RESIDUAL_WIDTHS)?,
        )
    } else {
        (vec![], vec![])
    };
    let mut q: Vec<Option<[i32; 3]>> = vec![None; count];
    let mut qn: Vec<[i32; 3]> = if normals { vec![[0; 3]; count] } else { vec![] };
    let (mut ai, mut ri) = (0usize, 0usize);
    let start = |e: usize| -> Result<Option<usize>, HsfError> {
        let s = edges[e][0];
        if s == DUMMY {
            return Ok(None);
        }
        usize::try_from(s)
            .ok()
            .filter(|s| *s < count)
            .map(Some)
            .ok_or_else(|| invalid("Connectivity vertex out of range"))
    };
    let mut stack = vec![];
    for e in 0..edges.len() {
        let Some(a) = start(e)? else { continue };
        if q[a].is_some() {
            continue;
        }
        let (e1, e2) = (next_edge(e), next_edge(next_edge(e)));
        if start(e1)?.is_none() || start(e2)?.is_none() {
            continue;
        }
        for ee in [e, e1, e2] {
            let v = start(ee)?.expect("checked above");
            if q[v].is_none() {
                let p = *abs_points
                    .get(ai)
                    .ok_or_else(|| invalid("Too few absolute connectivity points"))?;
                q[v] = Some(p);
                if normals {
                    qn[v] = abs_normals[ai];
                }
                ai += 1;
            }
            if edges[ee][1] >= 0 {
                stack.push(ee);
            }
        }
        while let Some(h) = stack.pop() {
            let t = edges[h][1] as usize;
            let Some(v) = start(prev_edge(t))? else {
                continue;
            };
            if q[v].is_some() {
                continue;
            }
            let back = (edges[t][1] & 0x7fff_ffff) as usize;
            let corner = |e: usize| -> Result<usize, HsfError> {
                start(e)?.ok_or_else(|| invalid("Prediction through a dummy vertex"))
            };
            let (pa, pb, pc) = (corner(t)?, corner(next_edge(t))?, corner(prev_edge(back))?);
            let predict =
                |values: &dyn Fn(usize) -> Option<[i32; 3]>| -> Result<[i32; 3], HsfError> {
                    let (a, b, c) = (values(pa), values(pb), values(pc));
                    let (Some(a), Some(b), Some(c)) = (a, b, c) else {
                        return Err(invalid("Prediction from an unknown vertex"));
                    };
                    Ok(std::array::from_fn(|i| {
                        b[i].wrapping_sub(c[i]).wrapping_add(a[i])
                    }))
                };
            let r = *res_points
                .get(ri)
                .ok_or_else(|| invalid("Too few connectivity point residuals"))?;
            let p = predict(&|v| q[v])?;
            q[v] = Some(std::array::from_fn(|i| p[i].wrapping_sub(r[i])));
            if normals {
                let n = predict(&|v| q[v].map(|_| qn[v]))?;
                let r = res_normals[ri];
                qn[v] = std::array::from_fn(|i| n[i].wrapping_sub(r[i]));
            }
            ri += 1;
            for ee in [next_edge(t), next_edge(next_edge(t))] {
                if edges[ee][1] >= 0 {
                    stack.push(ee);
                }
            }
        }
    }
    let q = q
        .into_iter()
        .map(|p| p.ok_or_else(|| invalid("Connectivity vertex without a point")))
        .collect::<Result<_, _>>()?;
    Ok((q, qn))
}

fn dequantize(q: &[[i32; 3]], bounds: [f32; 6], bits: [u32; 3]) -> Vec<[f32; 3]> {
    let max: [i32; 3] = bits.map(|b| ((1u32 << b) - 1) as i32);
    let scale: [f32; 3] = std::array::from_fn(|i| {
        if bounds[i + 3] == bounds[i] {
            0.0
        } else {
            (bounds[i + 3] - bounds[i]) / max[i] as f32
        }
    });
    q.iter()
        .map(|p| {
            std::array::from_fn(|i| {
                if p[i] == max[i] {
                    bounds[i + 3]
                } else {
                    p[i] as f32 * scale[i] + bounds[i]
                }
            })
        })
        .collect()
}

pub(super) fn decode(
    stream: &[u8],
    layout: &ConnectivityLayout,
    budget: &mut Budget,
) -> Result<Decoded, HsfError> {
    let sections = connectivity_sections(stream)?;
    let t = table(&stream[sections.table.clone()])?;
    let ops = &stream[sections.ops.clone()];
    let conn = connectivity(ops, budget)?;
    let (faces, edges, count) = remove_dummies(conn, &t.dummies)?;
    if count != sections.points_count {
        return Err(invalid(
            "Connectivity vertex count differs from the stream header",
        ));
    }
    let normals = layout.normals_scheme > 0;
    let (q, qn) = points(
        &stream[sections.points.clone()],
        &edges,
        count,
        normals,
        budget,
    )?;
    budget.charge_output(count as u64 * 24 + faces.len() as u64 * 16)?;
    let vertices = dequantize(&q, t.bounds, t.bits);
    let normals = if normals {
        dequantize(&qn, [-1., -1., -1., 1., 1., 1.], t.normal_bits)
    } else {
        vec![]
    };
    let mut face_list = Vec::with_capacity(faces.len() * 4);
    for f in faces {
        face_list.extend([3, f[0], f[1], f[2]]);
    }
    Ok(Decoded {
        vertices,
        normals,
        normal_bits: t.normal_bits[0] as u8,
        face_list,
    })
}
