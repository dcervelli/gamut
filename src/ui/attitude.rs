//! Which way the camera faced, drawn: a camera in three dimensions over a
//! level floor, pitched and rolled as the file says it was, for the info
//! panel's Orientation section; and the level line drawn across the picture
//! while the pointer is on that section.
//!
//! The camera is the picture as shown given a body: its back is as wide
//! and as tall as the picture, so a portrait is drawn as a camera held on
//! its side, and the lens comes out of its front. It is drawn by hand with
//! the painter, each face a flat polygon shaded by how squarely it faces
//! the light, the faces turned away from the eye left out and the rest
//! painted from the farthest in — enough for a few convex boxes.

use ::image::metadata::Orientation;
use egui::{Color32, Pos2, Stroke, pos2, vec2};

use crate::image::attitude::Shown;
use crate::render::Color;

use crate::image::orient::Turn;

use super::chrome::Pass;
use super::icon;
use super::{Current, Thumb, standin};
use super::{loupe, minimap};
use crate::theme::Theme;

/// The diagram's height in the column.
pub(super) const HEIGHT: f32 = 132.0;
/// What the diagram grows by where the floor wears the arrows to where the
/// lens pointed and to north: enough for the floor's near corner and the
/// words at the end of an arrow toward the eye.
pub(super) const BEARING_ROOM: f32 = 48.0;

/// The world is the camera's own axes held level: x across to the right,
/// y down and z ahead. The eye looks at it from behind the camera and to its
/// right, and from above: turned this far about the vertical, so the lens
/// points away to the right of the eye ...
const YAW: f32 = 0.95;
/// ... and tipped this far, so the floor is seen from above.
const ELEVATION: f32 = 0.38;

/// How far the floor is below the camera's center, how far out it runs
/// either way, and how many lines cross it, in the units the body's longer
/// side is two of.
const FLOOR_DROP: f32 = 1.6;
const FLOOR_REACH: f32 = 2.2;
const FLOOR_LINES: usize = 5;

/// The body's depth, and the lens's length and radius against the body's
/// shorter side; the button on top's size and where it stands along it.
const BODY_DEPTH: f32 = 0.55;
const LENS_LENGTH: f32 = 0.7;
const LENS_RADIUS: f32 = 0.36;
const LENS_SIDES: usize = 24;
const BUTTON: [f32; 3] = [0.28, 0.14, 0.24];
const BUTTON_ALONG: f32 = 0.55;

/// The camera's back, which looks out along -z.
const BACK: Point = [0.0, 0.0, -1.0];
/// The screen on the back, as a share of the back's size.
const SCREEN: f32 = 0.72;
/// What the picture on the screen is multiplied by, so that it sits back
/// on the shaded body a little rather than glaring out of it.
const SCREEN_TINT: Color32 = Color32::from_gray(220);

/// How far ahead of the lens the line it looks along runs.
const SIGHT: f32 = 1.6;

/// How far across the floor the arrow to where the lens pointed runs, and
/// the one to north, shorter, so that the two are told apart where they lie
/// close; the arrowheads' length and half their width; the weight of their
/// shafts; and the gap between a tip and the words beyond it, in logical
/// pixels.
const HEADING_REACH: f32 = 2.0;
const NORTH_REACH: f32 = 1.3;
const ARROW_HEAD: [f32; 2] = [0.32, 0.14];
const ARROW_LINE: f32 = 1.5;
const ARROW_GAP: f32 = 3.0;

/// The most a picture's sides are drawn apart, so a panorama is still a
/// camera rather than a plank.
const MAX_ASPECT: f32 = 2.0;

/// How far across the diagram the scene runs, in the units above: what
/// sets its scale, kept the same whichever way the camera points so that
/// one picture's camera and the next one's are drawn the same size.
const SCENE_SPAN: f32 = 3.8;

/// The weight of the camera's edges and of the line it looks along, and of
/// the horizon drawn across the picture.
const EDGE: f32 = 1.0;
const HORIZON_LINE: f32 = 1.5;
/// The dashes the horizon and the line of sight are drawn in.
const DASH: f32 = 6.0;
const GAP: f32 = 4.0;
/// The lighter line through the middle that says only which way is level,
/// where the horizon is not on the picture or has no place: as heavy as the
/// horizon, in shorter dashes, and this share of the accent.
const LEVEL_DASH: f32 = 3.0;
const LEVEL_GAP: f32 = 3.0;
const LEVEL_INK: f32 = 0.7;

/// How squarely a face must face the eye to be drawn: the cosine of the
/// angle between its outward way and the way back to the eye.
const EDGE_ON: f32 = 0.03;

/// A point of the scene.
type Point = [f32; 3];

/// One flat face of a solid, in the camera's axes: its corners in order
/// round it, and the way it looks out.
struct Face {
    corners: Vec<Point>,
    outward: Point,
}

/// Draws the camera as `shown` says it faced into `rect`, its body the
/// shape of a picture `size` pixels across and down, and on its screen
/// `thumb`, the file's thumbnail read through the turn in force, where
/// there is one. Where the file says the bearing the lens pointed on,
/// `heading`, the floor wears an arrow along that way, written with the
/// bearing — marked as from `magnetic` north where it is — and another to
/// north. With no `shown`, for a file that gives the bearing and not how
/// the camera was tilted, the camera is left out and [`NO_TILT`] said in
/// its place, over the floor and its arrows.
#[allow(clippy::too_many_arguments)]
pub(super) fn diagram(
    theme: &Theme,
    grid: icon::Grid,
    painter: &egui::Painter,
    rect: egui::Rect,
    shown: Option<&Shown>,
    size: [f32; 2],
    thumb: Option<(Thumb, Turn)>,
    heading: Option<f64>,
    magnetic: bool,
) {
    // Placed from the top by the height without the arrows, so that the
    // camera is drawn the same size and in the same place with them.
    let scale = rect.width().min(HEIGHT * 1.4) / SCENE_SPAN;
    let center =
        pos2(rect.center().x, rect.top() + HEIGHT / 2.0) + vec2(-0.3 * scale, -0.35 * scale);
    let eye = |point: Point| view(point);
    let screen = |point: Point| {
        let [x, y, _] = eye(point);
        pos2(center.x + x * scale, center.y + y * scale)
    };

    // The floor: lines across it each way, dim, and its edge.
    let dim: Color32 = theme.text_dim.into();
    let floor_stroke = Stroke::new(grid.line_width(EDGE), dim.gamma_multiply(0.45));
    for line in 0..FLOOR_LINES {
        let at = -FLOOR_REACH + 2.0 * FLOOR_REACH * line as f32 / (FLOOR_LINES - 1) as f32;
        for ends in [
            [
                [at, FLOOR_DROP, -FLOOR_REACH],
                [at, FLOOR_DROP, FLOOR_REACH],
            ],
            [
                [-FLOOR_REACH, FLOOR_DROP, at],
                [FLOOR_REACH, FLOOR_DROP, at],
            ],
        ] {
            painter.line_segment([screen(ends[0]), screen(ends[1])], floor_stroke);
        }
    }
    // With no tilt there is no camera to draw, nor gravity to hang from
    // it: the arrows, and over where the camera would stand the words
    // saying why it does not, on a patch of the panel's ground.
    let Some(shown) = shown else {
        floor_arrows(theme, grid, painter, &screen, heading, magnetic);
        let galley = painter.layout_no_wrap(
            NO_TILT.to_string(),
            egui::FontId::proportional(BEARING_SIZE),
            Color32::PLACEHOLDER,
        );
        let at = screen([0.0, 0.0, 0.0]);
        let extent = galley.size();
        painter.rect_filled(
            egui::Rect::from_center_size(at, extent + vec2(4.0, 0.0)),
            2.0,
            theme.bar_background,
        );
        painter.galley(at - extent / 2.0, galley, dim);
        return;
    };
    let axes = camera_axes(shown.gravity);
    // Gravity, straight down from the camera to the floor.
    painter.extend(egui::Shape::dashed_line(
        &[screen([0.0, 0.0, 0.0]), screen([0.0, FLOOR_DROP, 0.0])],
        Stroke::new(grid.line_width(EDGE), dim),
        DASH / 2.0,
        GAP / 2.0,
    ));
    floor_arrows(theme, grid, painter, &screen, heading, magnetic);
    let accent: Color32 = theme.accent.into();

    // The solids, in the camera's axes and then carried into the world's.
    let aspect = (size[0] / size[1].max(1.0)).clamp(1.0 / MAX_ASPECT, MAX_ASPECT);
    let half = if aspect >= 1.0 {
        [1.0, 1.0 / aspect]
    } else {
        [aspect, 1.0]
    };
    let short = half[0].min(half[1]);
    let body = cuboid([0.0, 0.0, 0.0], [half[0], half[1], BODY_DEPTH / 2.0]);
    // The parts that stand on the body, each with the way the face it
    // stands on looks out. The button on top says which way is up: the
    // camera's top is its -y.
    let parts = [
        (
            [0.0, -1.0, 0.0],
            cuboid(
                [BUTTON_ALONG * half[0], -half[1] - BUTTON[1] / 2.0, 0.0],
                [BUTTON[0] / 2.0 * short, BUTTON[1] / 2.0, BUTTON[2] / 2.0],
            ),
        ),
        (
            [0.0, 0.0, 1.0],
            cylinder(
                BODY_DEPTH / 2.0,
                BODY_DEPTH / 2.0 + LENS_LENGTH * short,
                LENS_RADIUS * short,
            ),
        ),
    ];
    let world = |point: Point| carry(axes, point);
    // The screen's corners clockwise from its top left as the eye behind
    // the camera sees them: the picture's left is the camera's -x and its
    // top the camera's -y, as the back is seen from behind.
    let glass_corners: [Pos2; 4] =
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(x, y)| {
            screen(world([
                x * SCREEN * half[0],
                y * SCREEN * half[1],
                -BODY_DEPTH / 2.0,
            ]))
        });

    let edge = Stroke::new(grid.line_width(EDGE), accent);
    // Facing away from the eye, which looks along +z, or so nearly edge-on
    // that it is a sliver, whose sharp corners would be stroked as spikes.
    let facing = |outward: Point| view(world(outward))[2] < -EDGE_ON;
    let depth = |faces: &[Face]| {
        let corners = faces.iter().flat_map(|face| &face.corners);
        corners
            .clone()
            .map(|&point| eye(world(point))[2])
            .sum::<f32>()
            / corners.count() as f32
    };
    // Each solid is convex, so of its own faces only those facing the eye
    // are drawn and none of them covers another. Between solids, a part
    // stands on the far side of its face's plane from the body, so it is
    // in front of the body where that face is toward the eye and behind it
    // where the face is away: ordering by where the faces' middles are
    // would draw a small part over the body it is hidden by.
    let (mut front, mut behind): (Vec<_>, Vec<_>) =
        parts.iter().partition(|(stands_on, _)| facing(*stands_on));
    behind.sort_by(|a, b| depth(&b.1).total_cmp(&depth(&a.1)));
    front.sort_by(|a, b| depth(&b.1).total_cmp(&depth(&a.1)));
    let solids = behind
        .into_iter()
        .map(|(_, faces)| (faces, false))
        .chain(std::iter::once((&body, true)))
        .chain(front.into_iter().map(|(_, faces)| (faces, false)));
    let drawn: Vec<(Vec<Pos2>, Color32, bool)> = solids
        .flat_map(|(faces, is_body)| faces.iter().map(move |face| (face, is_body)))
        .filter(|(face, _)| facing(face.outward))
        .map(|(face, is_body)| {
            let normal = world(face.outward);
            let light = dot(normal, LIGHT).max(0.0);
            let fill = mix(theme.bar_background, theme.accent, 0.12 + 0.5 * light);
            let mut points: Vec<Pos2> = face
                .corners
                .iter()
                .map(|&point| screen(world(point)))
                .collect();
            // Clockwise on screen, which is the way round the painter
            // feathers a polygon's edge outward.
            if signed_area(&points) < 0.0 {
                points.reverse();
            }
            (points, fill, is_body && face.outward == BACK)
        })
        .collect();
    // The line the lens looks along, behind the camera where it is behind
    // it and in front where it is in front: drawn first, and the solids
    // over it.
    let lens_front = BODY_DEPTH / 2.0 + LENS_LENGTH * short;
    let sight = [
        screen(world([0.0, 0.0, lens_front])),
        screen(world([0.0, 0.0, lens_front + SIGHT])),
    ];
    painter.extend(egui::Shape::dashed_line(&sight, edge, DASH, GAP));
    for (points, fill, back) in drawn {
        // The back wears a screen, which says which side of the box the
        // eye is looking at.
        let screen = back.then(|| {
            let middle = points
                .iter()
                .fold(vec2(0.0, 0.0), |sum, point| sum + point.to_vec2())
                / points.len() as f32;
            points
                .iter()
                .map(|point| (middle + (point.to_vec2() - middle) * SCREEN).to_pos2())
                .collect::<Vec<Pos2>>()
        });
        painter.add(egui::Shape::convex_polygon(
            points.clone(),
            fill,
            Stroke::NONE,
        ));
        outline(painter, &points, edge);
        if let Some(screen) = screen {
            let glass = mix(theme.bar_background, theme.accent, 0.05);
            painter.add(egui::Shape::convex_polygon(
                screen.clone(),
                glass,
                Stroke::NONE,
            ));
            if let Some((thumb, turn)) = thumb {
                painter.add(picture_on_screen(
                    &thumb,
                    turn,
                    size,
                    aspect,
                    glass_corners,
                    grid.scale(),
                ));
            }
            outline(painter, &screen, Stroke::new(edge.width, dim));
        }
    }
}

/// What the diagram says where the camera would stand, for a file that
/// gives the bearing its lens pointed along and not how it was tilted.
pub(super) const NO_TILT: &str = "No pitch/roll information";

/// The arrows across the floor, drawn through `screen`, where the file
/// says the `heading` the lens pointed along: see [`diagram`].
fn floor_arrows(
    theme: &Theme,
    grid: icon::Grid,
    painter: &egui::Painter,
    screen: &impl Fn(Point) -> Pos2,
    heading: Option<f64>,
    magnetic: bool,
) {
    let accent: Color32 = theme.accent.into();
    // Where the lens pointed and where north is, out from under the camera
    // across the floor. The lens points along +z as far as it points
    // anywhere level, and bearings run clockwise seen from above, toward
    // +x; north is the lens's bearing back from it.
    if let Some(heading) = heading {
        let stroke = |ink| Stroke::new(grid.line_width(ARROW_LINE), ink);
        let north = (-heading as f32).to_radians();
        let bearing = degrees(heading.round() as u32 % 360, magnetic);
        // North over the lens's way, whose arrow runs out past its words
        // where the two lie close.
        floor_arrow(
            painter,
            screen,
            0.0,
            HEADING_REACH,
            &bearing,
            stroke(accent),
            theme.bar_background.into(),
        );
        floor_arrow(
            painter,
            screen,
            north,
            NORTH_REACH,
            "N",
            stroke(theme.text_primary.into()),
            theme.bar_background.into(),
        );
    }
}

/// An arrow on the floor from under the camera out `reach` along the way
/// `angle` radians clockwise from the lens's, seen from above, in
/// `stroke`, and `words` beyond its tip on a patch of `ground`, the
/// panel's, so that the other arrow passing under them leaves them clear.
fn floor_arrow(
    painter: &egui::Painter,
    screen: &impl Fn(Point) -> Pos2,
    angle: f32,
    reach: f32,
    words: &str,
    stroke: Stroke,
    ground: Color32,
) {
    let (sin, cos) = angle.sin_cos();
    // `along` out the way and `aside` square to it, to its right.
    let at = |along: f32, aside: f32| {
        screen([
            along * sin + aside * cos,
            FLOOR_DROP,
            along * cos - aside * sin,
        ])
    };
    let [length, half] = ARROW_HEAD;
    painter.line_segment([at(0.0, 0.0), at(reach - length, 0.0)], stroke);
    let mut head = vec![
        at(reach, 0.0),
        at(reach - length, half),
        at(reach - length, -half),
    ];
    if signed_area(&head) < 0.0 {
        head.reverse();
    }
    painter.add(egui::Shape::convex_polygon(
        head,
        stroke.color,
        Stroke::NONE,
    ));
    // The words beyond the tip, their middle as far out along the arrow
    // as keeps the whole of them clear of it.
    let tip = at(reach, 0.0);
    let out = (tip - at(reach - length, 0.0)).normalized();
    let galley = painter.layout_no_wrap(
        words.to_string(),
        egui::FontId::proportional(BEARING_SIZE),
        Color32::PLACEHOLDER,
    );
    let extent = galley.size();
    let clear = (out.x.abs() * extent.x + out.y.abs() * extent.y) / 2.0;
    let middle = tip + out * (ARROW_GAP + clear);
    painter.rect_filled(
        egui::Rect::from_center_size(middle, extent + vec2(2.0, 0.0)),
        2.0,
        ground,
    );
    painter.galley(middle - extent / 2.0, galley, stroke.color);
}

/// `thumb` laid on the camera's screen at `corners`, clockwise from its top
/// left, read through `turn` as the picture on screen is. The body is no
/// further from square than [`MAX_ASPECT`], so a picture `size` across and
/// down that is, drawn `aspect` wide to its height, gives up its ends.
fn picture_on_screen(
    thumb: &Thumb,
    turn: Turn,
    size: [f32; 2],
    aspect: f32,
    corners: [Pos2; 4],
    scale: f32,
) -> egui::Mesh {
    let side = corners
        .iter()
        .flat_map(|a| corners.iter().map(move |b| a.distance(*b)))
        .fold(0.0, f32::max);
    let texture = thumb.for_side(side * scale);
    // The share of the picture kept each way as it is shown, then in the
    // thumbnail's own axes, which a quarter turn swaps.
    let picture = size[0] / size[1].max(1.0);
    let kept = [(aspect / picture).min(1.0), (picture / aspect).min(1.0)];
    let kept = turn.size(kept);
    let within = egui::Rect::from_center_size(pos2(0.5, 0.5), vec2(kept[0], kept[1]));
    let mut mesh = egui::Mesh::with_texture(texture.id);
    for (pos, uv) in corners.into_iter().zip(standin::uvs(turn, within)) {
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv,
            color: SCREEN_TINT,
        });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    mesh
}

/// Strokes round `points` edge by edge, each corner rounded. A closed
/// polygon's own stroke joins its corners with miters that egui does not
/// limit, so a sharp corner seen in perspective grows a spike.
fn outline(painter: &egui::Painter, points: &[Pos2], stroke: Stroke) {
    for (&a, &b) in points.iter().zip(points.iter().cycle().skip(1)) {
        painter.line_segment([a, b], stroke);
    }
    for &point in points {
        painter.circle_filled(point, stroke.width / 2.0, stroke.color);
    }
}

/// Where the light comes from: above, to the left and behind the eye.
const LIGHT: Point = [-0.36, -0.8, -0.48];

/// The camera's three axes — across, down and ahead — in the world's,
/// from gravity in the camera's, a unit vector. Gravity in the world pulls
/// straight down +y, so each axis's y is how far it lies along gravity; the
/// lens is pointed straight ahead, +z, as far as gravity leaves it free to
/// be, and across is square to it and level with it as far as the roll
/// allows, on whichever side keeps down agreeing with gravity.
fn camera_axes(gravity: [f64; 3]) -> [Point; 3] {
    let [gx, gy, gz] = gravity.map(|value| value as f32);
    let level = (1.0 - gz * gz).max(0.0).sqrt();
    let ahead = [0.0, gz, level];
    // With the lens pointed straight up or down, anything level is square
    // to it.
    let (z, side) = if level > 1e-4 {
        (-gx * gz / level, gy)
    } else {
        (0.0, 1.0)
    };
    let x = (1.0 - gx * gx - z * z).max(0.0).sqrt().copysign(side);
    let across = [x, gx, z];
    // Right-handed, as the camera's own axes are: across by down is ahead.
    let down = cross(ahead, across);
    [across, down, ahead]
}

/// A point in the camera's axes, in the world's.
fn carry([across, down, ahead]: [Point; 3], [x, y, z]: Point) -> Point {
    std::array::from_fn(|axis| across[axis] * x + down[axis] * y + ahead[axis] * z)
}

/// A point of the world as the eye sees it: x across the diagram, y down it
/// and z away from the eye.
fn view([x, y, z]: Point) -> Point {
    let (sin, cos) = YAW.sin_cos();
    let (x, z) = (x * cos + z * sin, z * cos - x * sin);
    let (sin, cos) = ELEVATION.sin_cos();
    let (y, z) = (y * cos - z * sin, y * sin + z * cos);
    [x, y, z]
}

/// A box about `center`, `half` its size each way, as its six faces.
fn cuboid(center: Point, half: Point) -> Vec<Face> {
    let mut faces = Vec::with_capacity(6);
    for axis in 0..3 {
        let [b, c] = [(axis + 1) % 3, (axis + 2) % 3];
        for side in [-1.0f32, 1.0] {
            let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .into_iter()
                .map(|(along_b, along_c)| {
                    let mut point = center;
                    point[axis] += side * half[axis];
                    point[b] += along_b * half[b];
                    point[c] += along_c * half[c];
                    point
                })
                .collect();
            let mut outward = [0.0; 3];
            outward[axis] = side;
            faces.push(Face { corners, outward });
        }
    }
    faces
}

/// A cylinder along the camera's z from `back` to `front`, as its sides and
/// its front face.
fn cylinder(back: f32, front: f32, radius: f32) -> Vec<Face> {
    let angle = |step: f32| std::f32::consts::TAU * step / LENS_SIDES as f32;
    let at = |step: usize, z: f32| {
        let (sin, cos) = angle(step as f32).sin_cos();
        [radius * cos, radius * sin, z]
    };
    let mut faces: Vec<Face> = (0..LENS_SIDES)
        .map(|step| {
            let (sin, cos) = angle(step as f32 + 0.5).sin_cos();
            Face {
                corners: vec![
                    at(step, back),
                    at(step, front),
                    at(step + 1, front),
                    at(step + 1, back),
                ],
                outward: [cos, sin, 0.0],
            }
        })
        .collect();
    faces.push(Face {
        corners: (0..LENS_SIDES).map(|step| at(step, front)).collect(),
        outward: [0.0, 0.0, 1.0],
    });
    faces
}

/// Twice the area `points` enclose, positive where they go round clockwise
/// on screen, whose y runs down.
fn signed_area(points: &[Pos2]) -> f32 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| a.x * b.y - b.x * a.y)
        .sum()
}

fn cross(a: Point, b: Point) -> Point {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: Point, b: Point) -> f32 {
    let length = (b[0] * b[0] + b[1] * b[1] + b[2] * b[2]).sqrt();
    (a[0] * b[0] + a[1] * b[1] + a[2] * b[2]) / length
}

/// `from` taken `share` of the way to `to`.
fn mix(from: Color, to: Color, share: f32) -> Color32 {
    let from: Color32 = from.into();
    let to: Color32 = to.into();
    let channel =
        |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * share).round() as u8;
    Color32::from_rgb(
        channel(from.r(), to.r()),
        channel(from.g(), to.g()),
        channel(from.b(), to.b()),
    )
}

/// Which way the camera faced in the picture on screen, as the info
/// panel's Orientation section, the horizon grid and the pointer's readout
/// read it: the file's attitude worked out in the picture as shown, which
/// the turn in force decides. `None` where the file does not say.
pub(super) fn shown(current: &Current) -> Option<Shown> {
    let attitude = current.exif.attitude.as_ref()?;
    let face = current.picture_face();
    let size = current.turn.size([face.image.width, face.image.height]);
    Some(
        attitude.shown(
            current
                .exif
                .orientation
                .unwrap_or(Orientation::NoTransforms),
            current.turn,
            size,
        ),
    )
}

/// The spacings the horizon grid's uprights are drawn at, in degrees round
/// the camera, which its key steps through; and the one it starts at.
pub const STEPS: [f32; 4] = [5.0, 10.0, 15.0, 30.0];
pub const DEFAULT_STEP: f32 = 10.0;

/// The spacing after `step` among [`STEPS`], round to the first after the
/// last; the default from one that is not among them.
pub fn cycle(step: f32) -> f32 {
    match STEPS.iter().position(|&each| each == step) {
        Some(at) => STEPS[(at + 1) % STEPS.len()],
        None => DEFAULT_STEP,
    }
}

/// A spacing as the horizon grid's button writes it after its mark.
pub fn label(step: f32) -> String {
    format!("{step}\u{00b0}")
}

/// Which line [`paint`] draws across the picture for `shown`: the true
/// horizon where the file says what focal length it was taken at and the
/// horizon falls on the picture, the level line through the middle
/// otherwise, and neither where the lens pointed too steeply for a horizon.
pub(super) fn line(current: &Current, shown: &Shown) -> Option<Line> {
    let horizon = shown.horizon?;
    let size = current.size();
    let (sin, cos) = (horizon as f32).to_radians().sin_cos();
    Some(match foot(current, shown) {
        Some(foot) if across(foot, [cos, sin], size).is_some() => Line::Horizon,
        _ => Line::Level,
    })
}

/// The point of the true horizon nearest the middle of the picture on
/// screen, in its pixels: `None` where it has no place.
fn foot(current: &Current, shown: &Shown) -> Option<[f32; 2]> {
    let size = current.size();
    let [x, y] = shown.horizon_through(size.map(f64::from), current.exif.focal_equivalent)?;
    Some([size[0] / 2.0 + x as f32, size[1] / 2.0 + y as f32])
}

/// What the horizon grid leaves clear inside the content area: the
/// loupe's glass and the minimap's thumbnail, which the image layer draws
/// under the whole interface, as the grid does.
#[derive(Clone, Copy)]
enum Hole {
    Glass(Pos2, f32),
    Thumbnail(egui::Rect),
}

impl Hole {
    /// How far along the segment from `from` by `along` the hole covers,
    /// as shares of it: `None` where it misses it.
    fn covers(self, from: Pos2, along: egui::Vec2) -> Option<(f32, f32)> {
        match self {
            Hole::Glass(center, radius) => {
                let offset = from - center;
                let a = along.length_sq();
                let b = offset.dot(along);
                let c = offset.length_sq() - radius * radius;
                let root = (b * b - a * c).sqrt();
                (root.is_finite() && root > 0.0).then(|| ((-b - root) / a, (-b + root) / a))
            }
            Hole::Thumbnail(rect) => {
                let start = from - rect.min;
                span([start.x, start.y], [along.x, along.y], rect.size().into())
            }
        }
    }

    /// Whether the hole reaches into `rect`.
    fn touches(self, rect: egui::Rect) -> bool {
        match self {
            Hole::Glass(center, radius) => rect.distance_sq_to_pos(center) < radius * radius,
            Hole::Thumbnail(thumbnail) => thumbnail.intersects(rect),
        }
    }
}

/// Where the picture is on screen, and the strokes drawn over it: with a
/// painter cut to the content area, so that a picture zoomed past it does
/// not carry the grid over the bars, and the holes the strokes are broken
/// around.
struct Canvas<'a> {
    painter: egui::Painter,
    screen: Box<dyn Fn([f32; 2]) -> Pos2 + 'a>,
    size: [f32; 2],
    accent: Color32,
    width: f32,
    holes: Vec<Hole>,
}

impl<'a> Canvas<'a> {
    fn new(pass: &'a Pass, painter: &egui::Painter, current: &Current) -> Self {
        let size = current.size();
        let placement = pass.view.placement(size, pass.input.viewport);
        let scale = pass.input.scale;
        let content = egui::Rect::from(pass.content);
        let width = pass.grid.line_width(HORIZON_LINE);
        // Each hole a halo's half-width wider, so that no stroke's edge
        // reaches into it.
        let margin = (width + HALO) / 2.0;
        let glass = pass.input.loupe.map(|loupe| {
            Hole::Glass(
                pos2(loupe.glass[0], loupe.glass[1]),
                loupe::GLASS_RADIUS + margin,
            )
        });
        let thumbnail = pass
            .input
            .minimap_on_screen
            .then(|| minimap::thumbnail(pass.content, size))
            .flatten()
            .map(|rect| Hole::Thumbnail(egui::Rect::from(rect).expand(margin)));
        Canvas {
            painter: painter.with_clip_rect(content.intersect(painter.clip_rect())),
            screen: Box::new(move |[x, y]| {
                let [x, y] = placement.screen_point([x, y]);
                pos2(x / scale, y / scale)
            }),
            size,
            accent: pass.theme.accent.into(),
            width,
            holes: glass.into_iter().chain(thumbnail).collect(),
        }
    }

    fn screen(&self, point: [f32; 2]) -> Pos2 {
        (self.screen)(point)
    }

    /// The stroke every line is drawn in, in `ink`.
    fn stroke(&self, ink: Color32) -> Stroke {
        Stroke::new(self.width, ink)
    }

    /// The white just wider than the stroke drawn under a line, so that
    /// it is seen on whatever the picture is behind it.
    fn halo(&self) -> Stroke {
        Stroke::new(self.width + HALO, HALO_INK)
    }

    /// A dashed line from `from` to `to`, each dash on its own halo.
    fn dashed(&self, ink: Color32, dash: f32, gap: f32, from: [f32; 2], to: [f32; 2]) {
        self.dashed_along(
            &self.painter,
            &[self.screen(from), self.screen(to)],
            ink,
            dash,
            gap,
        );
    }

    /// A dashed line along `path` on screen with `painter`, each dash on its
    /// own halo, the gaps left clear.
    fn dashed_along(
        &self,
        painter: &egui::Painter,
        path: &[Pos2],
        ink: Color32,
        dash: f32,
        gap: f32,
    ) {
        painter.extend(self.dashes(painter, path, self.halo(), dash, gap));
        painter.extend(self.dashes(painter, path, self.stroke(ink), dash, gap));
    }

    /// The strokes along `path` on screen in `stroke` that `painter` can
    /// show, dashed `dash` and `gap` long where those are given: only the
    /// stretches inside its clip and out of the holes are drawn, since a
    /// curve running far off the picture would otherwise be millions of
    /// dashes, each stretch begun where its dash began, so that the dashes
    /// keep their places along the path however much of it is in view.
    fn strokes(
        &self,
        painter: &egui::Painter,
        path: &[Pos2],
        stroke: Stroke,
        dashes: Option<(f32, f32)>,
    ) -> Vec<egui::Shape> {
        let rect = painter.clip_rect().expand(stroke.width);
        let Some((dash, gap)) = dashes else {
            return visible(path, rect, &self.holes, None)
                .into_iter()
                .map(|run| egui::Shape::line(run, stroke))
                .collect();
        };
        // Dashed first and the dashes cut round the holes after: a stretch
        // begun back where its dash began would otherwise begin inside the
        // hole it came out of.
        let mut dashed = Vec::new();
        for run in visible(path, rect, &[], Some(dash + gap)) {
            egui::Shape::dashed_line_many(&run, stroke, dash, gap, &mut dashed);
        }
        if self.holes.is_empty() {
            return dashed;
        }
        let mut shapes = Vec::new();
        for shape in dashed {
            if let egui::Shape::LineSegment { points, stroke } = shape {
                for piece in visible(&points, rect, &self.holes, None) {
                    shapes.push(egui::Shape::line_segment([piece[0], piece[1]], stroke));
                }
            }
        }
        shapes
    }

    /// The dashes along `path` on screen in `stroke` that `painter` can
    /// show; see [`Canvas::strokes`].
    fn dashes(
        &self,
        painter: &egui::Painter,
        path: &[Pos2],
        stroke: Stroke,
        dash: f32,
        gap: f32,
    ) -> Vec<egui::Shape> {
        self.strokes(painter, path, stroke, Some((dash, gap)))
    }

    /// A solid line along `path` on screen with `painter`, on its halo.
    fn solid(&self, painter: &egui::Painter, path: Vec<Pos2>, ink: Color32) {
        painter.extend(self.strokes(painter, &path, self.halo(), None));
        painter.extend(self.strokes(painter, &path, self.stroke(ink), None));
    }

    /// A short square mark from `at` on a line running `cos, sin`, pointing
    /// to the side of it that is up.
    fn tick(&self, at: Pos2, [cos, sin]: [f32; 2], ink: Color32) {
        let up = vec2(sin, -cos) * 10.0;
        self.solid(&self.painter, vec![at, at + up], ink);
    }

    /// `words` on a tag of its own, `rise` beyond `at` in the direction
    /// `up` on screen, so that it can be read whatever the picture is
    /// behind it. Left out where a hole reaches into the tag.
    fn tag(&self, at: Pos2, up: egui::Vec2, rise: f32, words: String) {
        let font = egui::FontId::proportional(BEARING_SIZE);
        let painter = &self.painter;
        let galley = painter.layout_no_wrap(words, font, Color32::PLACEHOLDER);
        let middle = at + up * (rise + galley.size().y / 2.0);
        let rect = egui::Rect::from_center_size(middle, galley.size() + vec2(6.0, 2.0));
        if self.in_hole(rect) {
            return;
        }
        painter.rect_filled(rect, 3.0, LABEL_GROUND);
        painter.galley(rect.center() - galley.size() / 2.0, galley, self.accent);
    }

    /// Whether any hole reaches into `rect`, for what is drawn whole or
    /// not at all.
    fn in_hole(&self, rect: egui::Rect) -> bool {
        self.holes.iter().any(|hole| hole.touches(rect))
    }

    /// The painter cut to the picture on screen, for what is drawn as a
    /// curve and so cannot be cut to it point by point.
    fn clipped(&self) -> egui::Painter {
        let corners = [self.screen([0.0, 0.0]), self.screen(self.size)];
        let picture = egui::Rect::from_two_pos(corners[0], corners[1]);
        self.painter
            .with_clip_rect(picture.intersect(self.painter.clip_rect()))
    }

    /// Every way `elevation` degrees above level, seen on the picture: the
    /// curve a cone of ways round the camera comes out as through a lens,
    /// a degree round at a time, in runs broken where it goes behind the
    /// lens.
    fn level_curve(&self, current: &Current, shown: &Shown, elevation: f64) -> Vec<Vec<Pos2>> {
        let size = self.size;
        let picture = size.map(f64::from);
        let focal = current.exif.focal_equivalent;
        let mut runs: Vec<Vec<Pos2>> = vec![Vec::new()];
        for degree in -CURVE_REACH..=CURVE_REACH {
            match shown.toward(picture, focal, f64::from(degree), elevation) {
                Some([x, y]) => runs
                    .last_mut()
                    .expect("never empty")
                    .push(self.screen([size[0] / 2.0 + x as f32, size[1] / 2.0 + y as f32])),
                None => runs.push(Vec::new()),
            }
        }
        runs.retain(|run| run.len() > 1);
        runs
    }
}

/// The horizon grid, drawn across the picture with `painter`: the line at
/// the angle `shown` says the horizon runs, from one edge of the picture to
/// the other, with a short square mark pointing up. Where [`line()`] says it
/// is the true horizon, it is drawn in full, with a dot where it passes
/// nearest the middle, under uprights `step` degrees apart round the
/// camera and the bearings they stand on. Otherwise the line through the
/// middle is drawn, lighter, saying only which way is level — and where the
/// horizon falls outside the picture, a chevron at the edge points the way
/// it lies. Nothing where the lens pointed too steeply for a horizon.
pub(super) fn paint(
    pass: &Pass,
    painter: &egui::Painter,
    current: &Current,
    shown: &Shown,
    step: f32,
) {
    let Some(kind) = line(current, shown) else {
        return;
    };
    let Some(horizon) = shown.horizon else {
        return;
    };
    let canvas = Canvas::new(pass, painter, current);
    let size = canvas.size;
    let middle = [size[0] / 2.0, size[1] / 2.0];
    let (sin, cos) = (horizon as f32).to_radians().sin_cos();
    let accent = canvas.accent;
    let level = accent.gamma_multiply(LEVEL_INK);
    let foot = foot(current, shown);

    if let (Line::Horizon, Some(foot)) = (kind, foot)
        && let Some([from, to]) = across(foot, [cos, sin], size)
    {
        // The uprights first, under the horizon: one every `step` degrees
        // round from where the lens pointed — on the round bearings, where
        // the file says what bearing that was — and over each the bearing
        // it stands on, where it crosses the horizon on the picture.
        let mut labels: Vec<(Pos2, String)> = Vec::new();
        let heading = current.exif.heading;
        let step = f64::from(step);
        let first = heading.map_or(0.0, |heading| (heading / step).ceil() * step - heading);
        let mut azimuth = first - (90.0 / step).ceil() * step;
        while azimuth < 90.0 {
            let this = azimuth;
            azimuth += step;
            let Some(([from, to], crossing)) = upright(current, shown, this) else {
                continue;
            };
            canvas.dashed(level, LEVEL_DASH, LEVEL_GAP, from, to);
            if let Some(heading) = heading
                && (0.0..=size[0]).contains(&crossing[0])
                && (0.0..=size[1]).contains(&crossing[1])
            {
                let bearing = (heading + this).round().rem_euclid(360.0) as u32;
                labels.push((
                    canvas.screen(crossing),
                    compass(bearing, current.exif.magnetic),
                ));
            }
        }
        // And the level curves, the same `step` apart above and below the
        // horizon, which is the one at nothing.
        let clipped = canvas.clipped();
        let mut elevation = step;
        while elevation < 90.0 {
            for signed in [elevation, -elevation] {
                for run in canvas.level_curve(current, shown, signed) {
                    canvas.dashed_along(&clipped, &run, level, LEVEL_DASH, LEVEL_GAP);
                }
            }
            elevation += step;
        }
        // The horizon on a halo the whole way along, the gaps too: the
        // one line the grid is read from.
        let painter = &canvas.painter;
        let ends = [canvas.screen(from), canvas.screen(to)];
        painter.extend(canvas.strokes(painter, &ends, canvas.halo(), None));
        painter.extend(canvas.dashes(painter, &ends, canvas.stroke(accent), DASH, GAP));
        let at = canvas.screen(foot);
        canvas.tick(at, [cos, sin], accent);
        let dot = canvas.width * 1.5;
        if !canvas.in_hole(egui::Rect::from_center_size(at, vec2(dot, dot) * 2.0)) {
            painter.circle_filled(at, dot, accent);
        }
        // Each bearing over the horizon where its upright crosses it,
        // above the dashes, on a tag of its own.
        for (at, words) in labels {
            canvas.tag(at, vec2(sin, -cos), BEARING_RISE, words);
        }
        return;
    }
    let Some([from, to]) = across(middle, [cos, sin], size) else {
        return;
    };
    canvas.dashed(level, LEVEL_DASH, LEVEL_GAP, from, to);
    canvas.tick(canvas.screen(middle), [cos, sin], level);
    // The horizon off the picture: a chevron at the edge, on the way from
    // the middle to it, pointing out toward it.
    if let Some(foot) = foot {
        let offset = [foot[0] - middle[0], foot[1] - middle[1]];
        let reach = edge_reach(offset, middle);
        let at = canvas.screen([middle[0] + offset[0] * reach, middle[1] + offset[1] * reach]);
        let out = (canvas.screen(foot) - canvas.screen(middle)).normalized();
        let side = out.rot90() * CHEVRON;
        let tip = at - out * 2.0;
        let back = tip - out * CHEVRON;
        canvas.solid(&canvas.painter, vec![back + side, tip, back - side], accent);
    }
}

/// What the horizon grid draws, with `painter`, for a picture whose file
/// gives the `heading` its lens pointed along but not how the camera was
/// tilted: a ring round the middle of the picture, the one point whose
/// bearing that is, and the bearing over it. Without the pitch and the
/// roll no other point can be given a bearing, nor any line be called
/// level, so nothing else is drawn.
pub(super) fn paint_heading(pass: &Pass, painter: &egui::Painter, current: &Current, heading: f64) {
    let canvas = Canvas::new(pass, painter, current);
    let middle = canvas.screen([canvas.size[0] / 2.0, canvas.size[1] / 2.0]);
    let reach = CENTER_RING + (canvas.width + HALO) / 2.0;
    if canvas.in_hole(egui::Rect::from_center_size(
        middle,
        vec2(reach, reach) * 2.0,
    )) {
        return;
    }
    let painter = &canvas.painter;
    painter.circle_stroke(middle, CENTER_RING, canvas.halo());
    painter.circle_stroke(middle, CENTER_RING, canvas.stroke(canvas.accent));
    let bearing = heading.round().rem_euclid(360.0) as u32;
    canvas.tag(
        middle,
        vec2(0.0, -1.0),
        CENTER_RING + BEARING_RISE,
        compass(bearing, current.exif.magnetic),
    );
}

/// The radius of the ring [`paint_heading`] draws round the middle of the
/// picture, on screen.
const CENTER_RING: f32 = 6.0;

/// The upright `azimuth` degrees round from where the lens pointed, cut to
/// the picture on screen: from the point every upright meets at, out
/// through where its way crosses the horizon — and that crossing, in the
/// picture's pixels. Past the meeting point the line is the way behind the
/// camera, and is left out. `None` where it misses the picture, or its way
/// is behind the lens.
fn upright(current: &Current, shown: &Shown, azimuth: f64) -> Option<([[f32; 2]; 2], [f32; 2])> {
    let horizon = shown.horizon?;
    let size = current.size();
    let picture = size.map(f64::from);
    let focal = current.exif.focal_equivalent;
    let middle = [size[0] / 2.0, size[1] / 2.0];
    let place = |[x, y]: [f64; 2]| [middle[0] + x as f32, middle[1] + y as f32];
    let crossing = place(shown.bearing(picture, focal, azimuth)?);
    let vanishing = shown.vanishing(picture, focal).map(place);
    let (sin, cos) = (horizon as f32).to_radians().sin_cos();
    let direction = match vanishing {
        Some(point) => [crossing[0] - point[0], crossing[1] - point[1]],
        None => [-sin, cos],
    };
    let length = direction[0].hypot(direction[1]);
    if length < 1e-3 {
        return None;
    }
    let direction = direction.map(|value| value / length);
    let behind = vanishing.map_or(f32::NEG_INFINITY, |_| -length);
    let (enter, leave) = span(crossing, direction, size)?;
    let enter = enter.max(behind);
    if enter >= leave {
        return None;
    }
    let at = |t: f32| {
        [
            crossing[0] + t * direction[0],
            crossing[1] + t * direction[1],
        ]
    };
    Some(([at(enter), at(leave)], crossing))
}

/// The two lines through the pixel `at` under the pointer, drawn with
/// `painter` while the horizon grid is on: the upright it stands on, and
/// the curve through every way as far above or below level as its own —
/// a curve, since a cone of ways round the camera is seen through a lens
/// as one — both solid, to tell them from the grid's dashes. Nothing where
/// the horizon has no place to start from.
pub(super) fn pointer(
    pass: &Pass,
    painter: &egui::Painter,
    current: &Current,
    shown: &Shown,
    at: [u32; 2],
) {
    if line(current, shown) != Some(Line::Horizon) {
        return;
    }
    let canvas = Canvas::new(pass, painter, current);
    let size = canvas.size;
    let picture = size.map(f64::from);
    let focal = current.exif.focal_equivalent;
    let offset = [
        f64::from(at[0]) + 0.5 - picture[0] / 2.0,
        f64::from(at[1]) + 0.5 - picture[1] / 2.0,
    ];
    let Some(way) = shown.direction(picture, focal, offset) else {
        return;
    };
    let Some(azimuth) = way.azimuth else {
        return;
    };
    let painter = canvas.clipped();
    let accent = canvas.accent;
    if let Some(([from, to], _)) = upright(current, shown, azimuth) {
        canvas.solid(
            &painter,
            vec![canvas.screen(from), canvas.screen(to)],
            accent,
        );
    }
    for run in canvas.level_curve(current, shown, way.elevation) {
        canvas.solid(&painter, run, accent);
    }
}

/// How far either way round the camera the pointer's level curve is
/// followed, in degrees: as far as anything ahead of the lens can be.
const CURVE_REACH: i32 = 89;

/// Which line [`paint`] draws across the picture.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Line {
    /// The true horizon, where it falls on the picture.
    Horizon,
    /// The line through the middle, which says only which way is level.
    Level,
}

impl Line {
    /// What the pill at the section's heading calls it.
    pub(super) fn word(self) -> &'static str {
        match self {
            Line::Horizon => "Horizon",
            Line::Level => "Level",
        }
    }
}

/// How much wider than its stroke the white under a line is: a hair
/// either side, enough to part it from the picture without boxing it in.
const HALO: f32 = 1.0;
const HALO_INK: Color32 = Color32::WHITE;

/// The tag a bearing is written on over the horizon, so that it can be
/// read whatever the picture is behind it.
const LABEL_GROUND: Color32 = Color32::WHITE;

/// The size a bearing is written at, and how far above the horizon.
const BEARING_SIZE: f32 = 11.0;
const BEARING_RISE: f32 = 6.0;

/// A bearing as written over its upright: the four points of the compass by
/// their letters, and the rest in degrees. A bearing from `magnetic` north
/// is always in degrees, with an M after them, since N would read as true
/// north.
fn compass(bearing: u32, magnetic: bool) -> String {
    match (bearing, magnetic) {
        (0, false) => "N".to_string(),
        (90, false) => "E".to_string(),
        (180, false) => "S".to_string(),
        (270, false) => "W".to_string(),
        _ => degrees(bearing, magnetic),
    }
}

/// A bearing in whole degrees, with an M after it where it is from
/// `magnetic` north.
pub(super) fn degrees(bearing: u32, magnetic: bool) -> String {
    format!("{bearing}\u{00b0}{}", magnetic_mark(magnetic))
}

/// What follows a bearing from `magnetic` north, to tell it from one from
/// true north: an M, and nothing after a bearing from true north.
pub(super) fn magnetic_mark(magnetic: bool) -> &'static str {
    if magnetic { "M" } else { "" }
}

/// How far the chevron marking a horizon off the picture runs back and
/// out to either side.
const CHEVRON: f32 = 7.0;

/// Where the line through `point` along `direction` enters and leaves a
/// picture `size` across and down: `None` where it misses it.
fn across(point: [f32; 2], direction: [f32; 2], size: [f32; 2]) -> Option<[[f32; 2]; 2]> {
    let (enter, leave) = span(point, direction, size)?;
    let at = |t: f32| [point[0] + t * direction[0], point[1] + t * direction[1]];
    Some([at(enter), at(leave)])
}

/// How far along `direction` from `point` the line enters and leaves a
/// picture `size` across and down: `None` where it misses it.
fn span(point: [f32; 2], direction: [f32; 2], size: [f32; 2]) -> Option<(f32, f32)> {
    let (mut enter, mut leave) = (f32::NEG_INFINITY, f32::INFINITY);
    for axis in 0..2 {
        if direction[axis].abs() < 1e-6 {
            if !(0.0..=size[axis]).contains(&point[axis]) {
                return None;
            }
            continue;
        }
        let a = -point[axis] / direction[axis];
        let b = (size[axis] - point[axis]) / direction[axis];
        enter = enter.max(a.min(b));
        leave = leave.min(a.max(b));
    }
    (enter < leave).then_some((enter, leave))
}

/// The stretches of `path` inside `rect` and out of the `holes`: where the
/// path runs out of `rect` or into a hole and back, a stretch apiece. With
/// a `period`, a dash and a gap, each stretch is begun as far back along
/// its first segment as the dash it enters in began — which, out of a hole,
/// is back inside it, so a path to be dashed is cut to `rect` alone.
fn visible(path: &[Pos2], rect: egui::Rect, holes: &[Hole], period: Option<f32>) -> Vec<Vec<Pos2>> {
    let mut runs = Vec::new();
    let mut run: Option<Vec<Pos2>> = None;
    let mut travelled = 0.0;
    for pair in path.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let along = to - from;
        let length = along.length();
        if length <= 0.0 {
            continue;
        }
        let start = from - rect.min;
        let mut shown: Vec<(f32, f32)> =
            span([start.x, start.y], [along.x, along.y], rect.size().into())
                .map(|(enter, leave)| (enter.max(0.0), leave.min(1.0)))
                .into_iter()
                .collect();
        for hole in holes {
            if let Some((into, out)) = hole.covers(from, along) {
                shown = shown
                    .into_iter()
                    .flat_map(|(enter, leave)| [(enter, leave.min(into)), (enter.max(out), leave)])
                    .collect();
            }
            shown.retain(|(enter, leave)| enter < leave);
        }
        shown.retain(|(enter, leave)| enter < leave);
        shown.sort_by(|a, b| a.0.total_cmp(&b.0));
        if shown.first().is_none_or(|&(enter, _)| enter > 0.0) {
            runs.extend(run.take());
        }
        for (enter, leave) in shown {
            let end = from + along * leave;
            match &mut run {
                Some(points) if enter == 0.0 => points.push(end),
                _ => {
                    runs.extend(run.take());
                    let back = period.map_or(0.0, |period| {
                        (travelled + length * enter).rem_euclid(period)
                    });
                    let begin = from + along * enter - along / length * back;
                    run = Some(vec![begin, end]);
                }
            }
            if leave < 1.0 {
                runs.extend(run.take());
            }
        }
        travelled += length;
    }
    runs.extend(run);
    runs
}

/// The share of `offset` from the middle that reaches the picture's edge,
/// `middle` being half the picture each way.
fn edge_reach(offset: [f32; 2], middle: [f32; 2]) -> f32 {
    offset
        .iter()
        .zip(middle)
        .filter(|(along, _)| along.abs() > 1e-6)
        .map(|(along, half)| half / along.abs())
        .fold(1.0, f32::min)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bearing from true north names the four points of the compass by
    /// their letters; one from magnetic north is always in degrees, marked.
    #[test]
    fn a_magnetic_bearing_is_marked() {
        assert_eq!(compass(0, false), "N");
        assert_eq!(compass(270, false), "W");
        assert_eq!(compass(349, false), "349\u{00b0}");
        assert_eq!(compass(0, true), "0\u{00b0}M");
        assert_eq!(compass(349, true), "349\u{00b0}M");
    }

    /// A line through the picture is cut at its edges; one that passes it
    /// by, level or slanted, is not drawn at all.
    #[test]
    fn a_line_is_cut_to_the_picture() {
        let size = [400.0, 300.0];
        assert_eq!(
            across([200.0, 100.0], [1.0, 0.0], size),
            Some([[0.0, 100.0], [400.0, 100.0]])
        );
        assert_eq!(across([200.0, -10.0], [1.0, 0.0], size), None);
        let (sin, cos) = 0.1f32.sin_cos();
        let [from, to] = across([200.0, 150.0], [cos, sin], size).unwrap();
        assert!(from[0].abs() < 1e-3 && (to[0] - 400.0).abs() < 1e-3);
        assert_eq!(across([200.0, -400.0], [cos, sin], size), None);
    }

    /// A path running far out of view is dashed only where it is in view,
    /// each stretch starting where a dash along the whole path would.
    #[test]
    fn only_what_is_in_view_is_dashed() {
        let rect = egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 100.0));
        let path = [
            pos2(-1.0e6, 50.0),
            pos2(50.0, 50.0),
            pos2(50.0, 1.0e6),
            pos2(50.0, 1.0e6 + 7.0),
        ];
        let runs = visible(&path, rect, &[], Some(6.0));
        assert_eq!(runs.len(), 1);
        let run = &runs[0];
        assert_eq!(run.len(), 3);
        // A million and one from the start is 6·166 666 and 4 on, so the
        // dash it enters in began 4 back from the edge.
        assert!((run[0].x + 4.0).abs() < 0.1 && run[0].y == 50.0);
        assert_eq!(run[1], pos2(50.0, 50.0));
        assert_eq!(run[2], pos2(50.0, 100.0));
        assert!(
            visible(
                &[pos2(-10.0, -10.0), pos2(-10.0, 200.0)],
                rect,
                &[],
                Some(6.0)
            )
            .is_empty()
        );
    }

    /// A line across the loupe's glass and the minimap's thumbnail is
    /// broken around both, and what lies wholly inside one is not drawn.
    #[test]
    fn the_glass_and_the_thumbnail_are_left_clear() {
        let rect = egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(400.0, 100.0));
        let holes = [
            Hole::Glass(pos2(100.0, 50.0), 20.0),
            Hole::Thumbnail(egui::Rect::from_min_max(
                pos2(250.0, 0.0),
                pos2(300.0, 100.0),
            )),
        ];
        let path = [pos2(0.0, 50.0), pos2(400.0, 50.0)];
        let runs = visible(&path, rect, &holes, None);
        let ends: Vec<[f32; 2]> = runs
            .iter()
            .map(|run| [run[0].x, run[run.len() - 1].x])
            .collect();
        let expected = [[0.0, 80.0], [120.0, 250.0], [300.0, 400.0]];
        assert_eq!(ends.len(), expected.len());
        for (got, want) in ends.iter().flatten().zip(expected.iter().flatten()) {
            assert!((got - want).abs() < 1e-3, "{ends:?}");
        }
        let inside = [pos2(95.0, 50.0), pos2(105.0, 50.0)];
        assert!(visible(&inside, rect, &holes, None).is_empty());
    }

    /// The edge is reached along the way to a horizon off the picture.
    #[test]
    fn a_horizon_off_the_picture_is_marked_at_its_edge() {
        assert_eq!(edge_reach([0.0, -600.0], [200.0, 150.0]), 0.25);
        assert_eq!(edge_reach([0.0, -100.0], [200.0, 150.0]), 1.0);
    }
}
