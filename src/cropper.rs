use itertools::Itertools;
use std::collections::HashMap;

use crate::{aspect_ratio::AspectRatio, geometry::Geometry, wallpapers::Face};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    X,
    Y,
}

impl std::fmt::Display for Direction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", if self == &Self::X { "X" } else { "Y" })
    }
}

#[derive(Debug)]
struct FaceArea {
    area: u32,
    start: u32,
}

pub struct Cropper {
    pub faces: Vec<Face>,
    pub width: u32,
    pub height: u32,
}

fn sort_faces_by_direction(faces: Vec<Face>, direction: Direction) -> Vec<Face> {
    let mut faces = faces;
    faces.sort_by_key(|face| match direction {
        Direction::X => face.xmin,
        Direction::Y => face.ymin,
    });
    faces
}

impl Cropper {
    pub fn new(faces: &[Face], width: u32, height: u32) -> Self {
        Self {
            faces: faces.to_vec(),
            width,
            height,
        }
    }

    pub fn crop_rect(&self, aspect_ratio: &AspectRatio) -> (u32, u32, Direction) {
        use std::cmp::min;
        let AspectRatio {
            w: target_w,
            h: target_h,
        } = aspect_ratio;

        // Calculate width and height that can be cropped while maintaining aspect ratio
        let crop_w = min(self.width, self.height * target_w / target_h);
        let crop_h = min(self.height, self.width * target_h / target_w);

        // Choose the larger dimension to get the largest possible cropped rectangle
        let (crop_w, crop_h) = if crop_w * target_h > crop_h * target_w {
            (crop_w, crop_h)
        } else {
            (crop_h * target_w / target_h, crop_h)
        };

        (
            crop_w,
            crop_h,
            if crop_w == self.width {
                Direction::Y
            } else {
                Direction::X
            },
        )
    }

    pub fn clamp(
        &self,
        val: f64,
        direction: Direction,
        target_width: u32,
        target_height: u32,
    ) -> Geometry {
        let val = val as u32;
        let (x, y) = match direction {
            Direction::X => (val.clamp(0, self.width - target_width), 0),
            Direction::Y => (0, val.clamp(0, self.height - target_height)),
        };

        Geometry {
            x,
            y,
            w: target_width,
            h: target_height,
        }
    }

    fn crop_single_face(
        &self,
        direction: Direction,
        target_width: u32,
        target_height: u32,
    ) -> Geometry {
        let face = &self.faces[0];
        let mid = match direction {
            Direction::X => (f64::from(face.xmin + face.xmax) - f64::from(target_width)) / 2.0,
            Direction::Y => (f64::from(face.ymin + face.ymax) - f64::from(target_height)) / 2.0,
        };
        self.clamp(mid, direction, target_width, target_height)
    }

    /// trivial crops, either same aspect ratio (entire image), no facec or single face
    fn crop_trivial(
        &self,
        direction: Direction,
        target_width: u32,
        target_height: u32,
    ) -> Option<Geometry> {
        // entire image
        if self.width == target_width && self.height == target_height {
            return Some(Geometry {
                x: 0,
                y: 0,
                w: target_width,
                h: target_height,
            });
        }

        // midpoint of image
        if self.faces.is_empty() {
            let (x, y) = match direction {
                Direction::X => ((self.width - target_width) / 2, 0),
                Direction::Y => (0, (self.height - target_height) / 2),
            };
            return Some(Geometry {
                x,
                y,
                w: target_width,
                h: target_height,
            });
        }

        if self.faces.len() == 1 {
            return Some(self.crop_single_face(direction, target_width, target_height));
        }

        // multiple faces, more processing is needed
        None
    }

    /// creates a range for a sliding window of target geometry to check for face intersections
    fn sliding_window_range(
        &self,
        faces: &[Face],
        direction: Direction,
        target: u32,
    ) -> impl Iterator<Item = (u32, u32)> {
        // cannot exceed the image dimensions
        let img_max = match direction {
            Direction::X => self.width - target,
            Direction::Y => self.height - target,
        };

        // the min can only be first face - half of target width
        let (first_min, first_max) = faces[0].dir_bounds(direction);
        let start = {
            // prevent subtract overflow
            let tmp = first_min + first_max;
            if tmp < target {
                0
            } else {
                (tmp - target) / 2
            }
        };
        let start = std::cmp::min(start, img_max);

        // the max can only be last face + half of target width
        let (last_min, last_max) = faces[faces.len() - 1].dir_bounds(direction);
        let end = std::cmp::min((last_min + last_max + target) / 2, img_max);

        (start..=end).map(move |rect_start| (rect_start, rect_start + target))
    }

    pub fn crop(&self, aspect_ratio: &AspectRatio) -> Geometry {
        let (target_width, target_height, direction) = self.crop_rect(aspect_ratio);
        let target = match direction {
            Direction::X => target_width,
            Direction::Y => target_height,
        };

        if let Some(cropped_geom) = self.crop_trivial(direction, target_width, target_height) {
            return cropped_geom;
        }

        // handle multiple faces
        let faces = sort_faces_by_direction(self.faces.clone(), direction);

        let mut max_faces = 0.0;
        let mut face_areas: Vec<FaceArea> = vec![];

        // slides a window of target geometry across the image, counting faces and intersections
        for (rect_start, rect_end) in self.sliding_window_range(&faces, direction, target) {
            let mut num_faces: f32 = 0.0;
            let mut faces_area = 0;

            for face in &faces {
                // check number of faces in decimal within enclosed within larger rectangle
                let (min_, max_) = face.dir_bounds(direction);

                // no intersection, we overshot the final box
                if min_ > rect_end {
                    break;
                }
                // no intersection
                else if max_ < rect_start {
                    continue;
                }
                // full intersection
                else if min_ >= rect_start && max_ <= rect_end {
                    num_faces += 1.0;
                    faces_area += face.area();
                    continue;
                }

                // partial intersection
                if min_ <= rect_end && max_ > rect_end {
                    num_faces += (rect_end - min_) as f32 / (max_ - min_) as f32;
                    faces_area += (rect_end - min_)
                        * match direction {
                            Direction::X => face.ymax - face.ymin,
                            Direction::Y => face.xmax - face.xmin,
                        };
                    continue;
                }
            }

            // update max_boxes
            if num_faces > 0.0 {
                if num_faces > max_faces {
                    max_faces = num_faces;
                    face_areas = vec![FaceArea {
                        area: faces_area,
                        start: rect_start,
                    }];
                } else if (num_faces - max_faces).abs() < f32::EPSILON {
                    face_areas.push(FaceArea {
                        area: faces_area,
                        start: rect_start,
                    });
                }
            }
        }

        face_areas.sort_by_key(|face_info| (face_info.area, face_info.start));
        // use the match with the maximum area of face coverage
        let max_face_area = face_areas.last().expect("face_areas is empty!").area;
        face_areas.retain(|face_info| face_info.area == max_face_area);

        self.clamp(
            f64::from(face_areas[face_areas.len() / 2].start),
            direction,
            target_width,
            target_height,
        )
    }

    /// shows cropping candidate rectangles for multiple faces
    pub fn crop_candidates(&self, aspect_ratio: &AspectRatio) -> Vec<Geometry> {
        let (target_width, target_height, direction) = self.crop_rect(aspect_ratio);
        let target = match direction {
            Direction::X => target_width,
            Direction::Y => target_height,
        };

        if let Some(cropped_geom) = self.crop_trivial(direction, target_width, target_height) {
            return vec![cropped_geom];
        }

        // handle multiple faces
        let faces = sort_faces_by_direction(self.faces.clone(), direction);
        let mut face_areas: Vec<FaceArea> = vec![];

        // slides a window of target geometry across the image, counting faces and intersections
        for (rect_start, rect_end) in self.sliding_window_range(&faces, direction, target) {
            // check number of faces in decimal within enclosed within larger rectangle
            for face in &faces {
                let (min_, max_) = face.dir_bounds(direction);

                // no intersection, we overshot the final box
                if min_ > rect_end {
                    break;
                }
                // no intersection
                else if max_ < rect_start {
                    continue;
                }
                // full intersection
                else if min_ >= rect_start && max_ <= rect_end {
                    face_areas.push(FaceArea {
                        area: face.area(),
                        start: rect_start,
                    });
                    continue;
                }
            }
        }

        face_areas.sort_by_key(|face_info| (face_info.area, face_info.start));

        // group faces by area
        let faces_by_area: HashMap<_, Vec<_>> =
            face_areas
                .iter()
                .fold(HashMap::new(), |mut acc, face_info| {
                    acc.entry(face_info.area).or_default().push(face_info.start);
                    acc
                });

        faces_by_area
            .values()
            .map(|faces| {
                let mid = faces[faces.len() / 2];
                self.clamp(f64::from(mid), direction, target_width, target_height)
            })
            .sorted_by_key(|geom| match direction {
                Direction::X => geom.x,
                Direction::Y => geom.y,
            })
            .collect()
    }
}
