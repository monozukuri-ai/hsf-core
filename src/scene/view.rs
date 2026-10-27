use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Handedness {
    Left,
    Right,
}

/// Stored camera values only. No active-camera selection, matrix application,
/// unit inference or CAD sheet/view mapping is performed.
#[derive(Clone, Debug, Serialize)]
pub struct Camera {
    pub projection_bits: u8,
    pub projection: String,
    pub stretched: bool,
    pub position: [f32; 3],
    pub target: [f32; 3],
    pub up_vector: [f32; 3],
    pub field_width: f32,
    pub field_height: f32,
    pub oblique_y_degrees: Option<f32>,
    pub oblique_x_degrees: Option<f32>,
    /// Scaled relative to the camera-to-target distance; not a world distance.
    pub near_limit: Option<f32>,
}
#[derive(Clone, Debug, Serialize)]
pub struct NamedView {
    pub id: String,
    pub source_record_id: String,
    pub source: SourceSpan,
    /// Lexical context, not the segment to which this camera is applied.
    pub context_node_id: String,
    pub name: TextValue,
    pub camera_source: SourceSpan,
    pub camera: Camera,
    /// Degenerate camera bases are preserved with unsupported status.
    pub status: Status,
}

pub(super) fn decode(doc: &HsfDocument, r: &Record) -> Result<(Camera, bool), HsfError> {
    let (p, source) = if let Some(l) = &r.camera {
        (l.projection, &l.settings)
    } else {
        let l = r.view.as_ref().expect("framed view");
        (l.projection, &l.camera)
    };
    let bytes = doc.source_bytes(source).expect("framed camera");
    let mut values = bytes
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().expect("four")));
    let mut value = || values.next().expect("framed floats");
    let position = [value(), value(), value()];
    let target = [value(), value(), value()];
    let up_vector = [value(), value(), value()];
    let field_width = value();
    let field_height = value();
    let oblique_y_degrees = (p & 4 != 0).then(&mut value);
    let oblique_x_degrees = (p & 8 != 0).then(&mut value);
    let near_limit = (p & 16 != 0).then(&mut value);
    if bytes
        .chunks_exact(4)
        .any(|b| !f32::from_le_bytes(b.try_into().expect("four")).is_finite())
    {
        return Err(fail(
            r,
            if r.camera.is_some() {
                "hsf.nonfinite_camera"
            } else {
                "hsf.nonfinite_view"
            },
            Status::Invalid,
            "Camera contains a nonfinite value",
        ));
    }
    if field_width <= 0. || field_height <= 0. {
        return Err(fail(
            r,
            if r.camera.is_some() {
                "hsf.invalid_camera_field"
            } else {
                "hsf.invalid_view_field"
            },
            Status::Invalid,
            "Camera field dimensions must be positive",
        ));
    }
    let direction =
        std::array::from_fn::<_, 3, _>(|i| f64::from(target[i]) - f64::from(position[i]));
    let up = up_vector.map(f64::from);
    let cross = [
        direction[1] * up[2] - direction[2] * up[1],
        direction[2] * up[0] - direction[0] * up[2],
        direction[0] * up[1] - direction[1] * up[0],
    ];
    let usable = cross.iter().any(|v| *v != 0.);
    Ok((
        Camera {
            projection_bits: p,
            projection: if p & 1 != 0 {
                "perspective"
            } else {
                "orthographic"
            }
            .into(),
            stretched: p & 2 != 0,
            position,
            target,
            up_vector,
            field_width,
            field_height,
            oblique_y_degrees,
            oblique_x_degrees,
            near_limit,
        },
        usable,
    ))
}

/// Ordered camera assignments to a segment. No projection matrix or CAD sheet is inferred.
#[derive(Clone, Debug, Serialize)]
pub struct SegmentCamera {
    pub node_id: String,
    pub source_record_id: String,
    pub source: SourceSpan,
    pub camera_source: SourceSpan,
    pub camera: Camera,
    pub status: Status,
}
