//! Connectivity-compressed Shells (flag 0x40, scheme 5), as written by the Creo
//! 11 eDrawings publisher. Streams are built here: opcodes, the auxiliary
//! table, and escape-coded points in big-endian bit fields of little-endian
//! words. Bounds 0..65535 at 16 bits make each decoded coordinate its code.
use hsf_core::*;

const RESIDUAL: [u32; 9] = [2, 6, 10, 14, 18, 22, 26, 31, 32];
const ABSOLUTE: [u32; 3] = [16, 31, 32];

#[derive(Default)]
struct Bits {
    words: Vec<u32>,
    bit: u32,
}
impl Bits {
    fn raw(&mut self, value: u32, width: u32) {
        for i in (0..width).rev() {
            if self.bit == 0 {
                self.words.push(0);
            }
            *self.words.last_mut().unwrap() |= ((value >> i) & 1) << (31 - self.bit);
            self.bit = (self.bit + 1) % 32;
        }
    }
    fn put(&mut self, v: i32, widths: &[u32]) {
        for &w in widths {
            let bias = (1i64 << (w - 1)) - 1;
            let code = i64::from(v) + bias;
            if w == 32 {
                return self.raw(code as u32, 32);
            }
            let all_ones = (1i64 << w) - 1;
            if (0..all_ones).contains(&code) {
                return self.raw(code as u32, w);
            }
            self.raw(all_ones as u32, w);
        }
    }
    fn bytes(&self) -> Vec<u8> {
        self.words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }
}

fn pad4(b: &mut Vec<u8>) {
    while b.len() % 4 != 0 {
        b.push(0);
    }
}

/// A layout-2 stream: header, opcodes, table (dummies, 0..65535 bounds, 16
/// bits), points (absolute triples, then residuals).
fn stream(
    ops: &[u8],
    dummies: &[i32],
    absolute: &[[i32; 3]],
    residual: &[[i32; 3]],
    count: i32,
) -> Vec<u8> {
    let mut table = vec![];
    let flags: i32 = 0x60 | if dummies.is_empty() { 0 } else { 8 };
    table.extend(flags.to_le_bytes());
    if !dummies.is_empty() {
        table.extend((dummies.len() as i32).to_le_bytes());
        let mut prev = 0;
        for d in dummies {
            table.extend((d - prev).to_le_bytes());
            prev = *d;
        }
    }
    for b in [0f32, 0., 0., 65535., 65535., 65535.] {
        table.extend(b.to_le_bytes());
    }
    for _ in 0..3 {
        table.extend(16i32.to_le_bytes());
    }
    let mut bits = Bits::default();
    bits.put(0, &RESIDUAL);
    bits.put(absolute.len() as i32, &ABSOLUTE);
    bits.put(residual.len() as i32, &ABSOLUTE);
    for p in absolute {
        p.iter().for_each(|v| bits.put(*v, &ABSOLUTE));
    }
    for r in residual {
        r.iter().for_each(|v| bits.put(*v, &RESIDUAL));
    }
    let points = bits.bytes();
    let mut s = vec![2, 0, 3, 0];
    for n in [ops.len(), table.len(), points.len()] {
        s.extend((n as i32).to_le_bytes());
    }
    s.extend(count.to_le_bytes());
    s.extend(0i32.to_le_bytes());
    s.extend(ops);
    pad4(&mut s);
    s.extend(table);
    pad4(&mut s);
    s.extend(points);
    s
}

fn shell_record(stream: &[u8], version: &str) -> Vec<u8> {
    let mut r = vec![b'S'];
    if version == "21.15" {
        r.push(0);
    }
    r.extend([0x51, 0, 5]);
    r.extend((stream.len() as i32).to_le_bytes());
    r.extend(stream);
    r
}

fn read(record: &[u8], version: &str) -> Result<ParseOutcome<Scene>, HsfError> {
    let data = [format!(";; HSF V{version} \n").as_bytes(), record, &[0]].concat();
    let mut budget = Budget::new(Limits::default());
    let p = parse(
        &data,
        &ParseOptions {
            mode: ParseMode::Strict,
            ..Default::default()
        },
        &mut budget,
    )?;
    scene(
        &p,
        &SceneOptions {
            mode: ParseMode::Strict,
            ..Default::default()
        },
        &mut budget,
    )
}

fn mesh(record: &[u8]) -> Mesh {
    read(record, "19.10").unwrap().data.meshes.remove(0)
}

fn points(m: &Mesh) -> Vec<[i32; 3]> {
    m.vertices.iter().map(|v| v.map(|c| c as i32)).collect()
}

#[test]
fn one_triangle_from_the_initial_loop() {
    let abs = [[1, 2, 3], [40, 2, 3], [1, 50, 3]];
    let m = mesh(&shell_record(&stream(&[2], &[], &abs, &[], 3), "19.10"));
    assert_eq!(points(&m), abs);
    assert_eq!(m.triangles, [[0, 1, 2]]);
}

#[test]
fn quad_predicts_its_fourth_point_across_the_shared_edge() {
    // R then E over a loop of four: (0,1,2), (0,2,3). Vertex 3 is predicted as
    // q2 - q1 + q0 = (0,10,0) and stored as prediction minus point.
    let abs = [[0, 0, 0], [10, 0, 0], [10, 10, 0]];
    let m = mesh(&shell_record(
        &stream(&[3, 2], &[], &abs, &[[-1, -2, 0]], 4),
        "19.10",
    ));
    assert_eq!(points(&m), [[0, 0, 0], [10, 0, 0], [10, 10, 0], [1, 12, 0]]);
    assert_eq!(m.triangles, [[0, 1, 2], [0, 2, 3]]);
    // The 21.15 framing (precision byte) reads the same stream.
    let m = read(
        &shell_record(&stream(&[3, 2], &[], &abs, &[[-1, -2, 0]], 4), "21.15"),
        "21.15",
    )
    .unwrap()
    .data
    .meshes
    .remove(0);
    assert_eq!(m.triangles.len(), 2);
}

#[test]
fn split_parks_a_loop_and_its_offset_comes_from_the_prepass() {
    // A hexagon: S (0,1,3) splits off (3,1,2), closed by E; the parked loop
    // gives R (0,3,4) and E (0,4,5). Points are reached as 0,1,3, then 4, 5, 2.
    let target = [
        [0, 0, 0],
        [20, 0, 0],
        [30, 17, 0],
        [20, 34, 0],
        [0, 34, 0],
        [-10, 17, 0],
    ]
    .map(|p: [i32; 3]| p.map(|v| v + 100));
    let (q0, q1, q2, q3, q4, q5) = (
        target[0], target[1], target[2], target[3], target[4], target[5],
    );
    let predict = |a: [i32; 3], b: [i32; 3], c: [i32; 3]| -> [i32; 3] {
        std::array::from_fn(|i| b[i] - c[i] + a[i])
    };
    let residual = |p: [i32; 3], q: [i32; 3]| -> [i32; 3] { std::array::from_fn(|i| p[i] - q[i]) };
    let r4 = residual(predict(q0, q3, q1), q4);
    let r5 = residual(predict(q0, q4, q3), q5);
    let r2 = residual(predict(q3, q1, q0), q2);
    let m = mesh(&shell_record(
        &stream(&[4, 2, 3, 2], &[], &[q0, q1, q3], &[r4, r5, r2], 6),
        "19.10",
    ));
    assert_eq!(points(&m), target);
    assert_eq!(m.triangles, [[0, 1, 3], [3, 1, 2], [0, 3, 4], [0, 4, 5]]);
}

#[test]
fn dummy_vertices_and_their_triangles_are_removed() {
    // The quad again, with vertex 3 a hole plug: only (0,1,2) remains.
    let abs = [[0, 0, 0], [10, 0, 0], [10, 10, 0]];
    let m = mesh(&shell_record(&stream(&[3, 2], &[3], &abs, &[], 3), "19.10"));
    assert_eq!(points(&m), abs);
    assert_eq!(m.triangles, [[0, 1, 2]]);
}

#[test]
fn unsupported_and_malformed_streams_stop_at_the_shell() {
    let abs = [[0, 0, 0], [10, 0, 0], [10, 10, 0]];
    let good = stream(&[3, 2], &[], &abs, &[[0, 0, 0]], 4);
    let code = |record: Vec<u8>| read(&record, "19.10").unwrap_err().diagnostic.code;
    // Merges (M) are not implemented.
    assert_eq!(
        code(shell_record(&stream(&[5, 2], &[], &abs, &[], 3), "19.10")),
        "hsf.shell_connectivity_feature"
    );
    // Another stream layout.
    let mut other = good.clone();
    other[0] = 1;
    assert_eq!(
        code(shell_record(&other, "19.10")),
        "hsf.shell_connectivity_scheme"
    );
    // Another Shell compression scheme.
    let mut record = shell_record(&good, "19.10");
    record[3] = 4;
    assert_eq!(code(record), "hsf.shell_connectivity_scheme");
    // Sections beyond the stream.
    let mut long = good.clone();
    long[12..16].copy_from_slice(&4096i32.to_le_bytes());
    assert_eq!(
        code(shell_record(&long, "19.10")),
        "hsf.invalid_shell_connectivity"
    );
    // A point count the opcodes do not produce.
    let mut count = good.clone();
    count[16..20].copy_from_slice(&5i32.to_le_bytes());
    assert_eq!(
        code(shell_record(&count, "19.10")),
        "hsf.invalid_shell_connectivity"
    );
    // Too few residuals for the vertices reached.
    assert_eq!(
        code(shell_record(&stream(&[3, 2], &[], &abs, &[], 4), "19.10")),
        "hsf.invalid_shell_connectivity"
    );
}

#[test]
fn layout_reports_the_stream_and_point_count() {
    let abs = [[0, 0, 0], [10, 0, 0], [10, 10, 0]];
    let s = stream(&[3, 2], &[], &abs, &[[0, 0, 0]], 4);
    let data = [
        b";; HSF V19.10 \n".as_slice(),
        &shell_record(&s, "19.10"),
        &[0],
    ]
    .concat();
    let p = parse(
        &data,
        &ParseOptions::default(),
        &mut Budget::new(Limits::default()),
    )
    .unwrap();
    let shell = p.data.records[0].shell().unwrap();
    let c = shell.connectivity.as_ref().unwrap();
    assert_eq!(
        (c.scheme, c.stream_scheme, c.points_scheme, c.opcode_count),
        (5, 2, 3, 2)
    );
    assert_eq!(c.stream.length, s.len() as u64);
    assert_eq!(shell.vertex_count, 4);
}

#[test]
fn every_single_byte_mutation_finishes_without_panicking() {
    let target = [
        [0, 0, 0],
        [20, 0, 0],
        [30, 17, 0],
        [20, 34, 0],
        [0, 34, 0],
        [-10, 17, 0],
    ];
    let s = stream(&[4, 2, 3, 2], &[1], &target[..3], &target[3..], 5);
    for i in 0..s.len() {
        for flip in [0x01u8, 0x80, 0xff] {
            let mut m = s.clone();
            m[i] ^= flip;
            let _ = read(&shell_record(&m, "19.10"), "19.10");
        }
    }
    for n in 0..s.len() {
        let _ = read(&shell_record(&s[..n], "19.10"), "19.10");
    }
}
