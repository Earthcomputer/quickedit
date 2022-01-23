use serde::Deserialize;

pub type ChunkPos = glam::IVec2;
pub type BlockPos = glam::IVec3;

macro_rules! glam_conversions {
    ($t:ty, $extensions:ident, $(($to_name:ident, $into_name:ident, $to_glam:ty, ($($field:ident, $to_elem:ty),*))),*) => {
        pub trait $extensions {
            $(
                fn $to_name(&self) -> $to_glam;
                fn $into_name(self) -> $to_glam;
            )*
        }
        impl $extensions for $t {
            $(
                fn $to_name(&self) -> $to_glam {
                    <$to_glam>::new($(self.$field as $to_elem),*)
                }
                fn $into_name(self) -> $to_glam {
                    <$to_glam>::new($(self.$field as $to_elem),*)
                }
            )*
        }
    }
}
glam_conversions!(glam::IVec2, IVec2Extensions,
    (to_float, into_float, glam::Vec2, (x, f32, y, f32)),
    (to_double, into_double, glam::DVec2, (x, f64, y, f64))
);
glam_conversions!(glam::Vec2, Vec2Extensions,
    (to_int, into_int, glam::IVec2, (x, i32, y, i32)),
    (to_double, into_double, glam::DVec2, (x, f64, y, f64))
);
glam_conversions!(glam::DVec2, DVec2Extensions,
    (to_int, into_int, glam::IVec2, (x, i32, y, i32)),
    (to_float, into_float, glam::Vec2, (x, f32, y, f32))
);
glam_conversions!(glam::IVec3, IVec3Extensions,
    (to_float, into_float, glam::Vec3, (x, f32, y, f32, z, f32)),
    (to_double, into_double, glam::DVec3, (x, f64, y, f64, z, f64))
);
glam_conversions!(glam::Vec3, Vec3Extensions,
    (to_int, into_int, glam::IVec3, (x, i32, y, i32, z, i32)),
    (to_double, into_double, glam::DVec3, (x, f64, y, f64, z, f64))
);
glam_conversions!(glam::DVec3, DVec3Extensions,
    (to_int, into_int, glam::IVec3, (x, i32, y, i32, z, i32)),
    (to_float, into_float, glam::Vec3, (x, f32, y, f32, z, f32))
);
glam_conversions!(glam::IVec4, IVec4Extensions,
    (to_float, into_float, glam::Vec4, (x, f32, y, f32, z, f32, w, f32)),
    (to_double, into_double, glam::DVec4, (x, f64, y, f64, z, f64, w, f64))
);
glam_conversions!(glam::Vec4, Vec4Extensions,
    (to_int, into_int, glam::IVec4, (x, i32, y, i32, z, i32, w, i32)),
    (to_double, into_double, glam::DVec4, (x, f64, y, f64, z, f64, w, f64))
);
glam_conversions!(glam::DVec4, DVec4Extensions,
    (to_int, into_int, glam::IVec4, (x, i32, y, i32, z, i32, w, i32)),
    (to_float, into_float, glam::Vec4, (x, f32, y, f32, z, f32, w, f32))
);
macro_rules! float_range_impl {
    ($t:ty, $ext:ident, $($field:ident),*) => {
        pub trait $ext {
            fn contains(&self, other: &$t) -> bool;
            fn is_empty(&self) -> bool;
        }

        impl $ext for std::ops::Range<$t> {
            fn contains(&self, other: &$t) -> bool {
                $(
                    if self.start.$field > other.$field || self.end.$field <= other.$field {
                        return false;
                    }
                )*
                true
            }
            fn is_empty(&self) -> bool {
                $(
                    if self.start.$field >= self.end.$field {
                        return true;
                    }
                )*
                false
            }
        }
        impl $ext for std::ops::RangeInclusive<$t> {
            fn contains(&self, other: &$t) -> bool {
                $(
                    if self.start().$field > other.$field || self.end().$field < other.$field {
                        return false;
                    }
                )*
                true
            }
            fn is_empty(&self) -> bool {
                $(
                    if self.start().$field > self.end().$field {
                        return true;
                    }
                )*
                false
            }
        }
    }
}
trait IntoIteratorHack : IntoIterator {
    type Item;
    type IntoIter;
    fn into_iter(self) -> <Self as IntoIteratorHack>::IntoIter;
}
macro_rules! int_range_impl {
    ($t:ty, $ext:ident, $iter:ident, $($field:ident),*) => {
        pub trait $ext {
            fn contains(&self, other: &$t) -> bool;
            fn is_empty(&self) -> bool;
            fn iter(self) -> $iter;
        }

        impl $ext for std::ops::RangeInclusive<$t> {
            fn contains(&self, other: &$t) -> bool {
                $(
                    if self.start().$field > other.$field || self.end().$field < other.$field {
                        return false;
                    }
                )*
                true
            }
            fn is_empty(&self) -> bool {
                $(
                    if self.start().$field > self.end().$field {
                        return true;
                    }
                )*
                false
            }
            fn iter(self) -> $iter {
                $iter {
                    start: *self.start(),
                    end: *self.end(),
                    next: Some(*self.start()),
                    next_back: Some(*self.end()),
                }
            }
        }

        impl $ext for std::ops::Range<$t> {
            fn contains(&self, other: &$t) -> bool {
                $(
                    if self.start.$field > other.$field || self.end.$field <= other.$field {
                        return false;
                    }
                )*
                true
            }
            fn is_empty(&self) -> bool {
                $(
                    if self.start.$field >= self.end.$field {
                        return true;
                    }
                )*
                false
            }
            fn iter(self) -> $iter {
                $iter {
                    start: self.start,
                    end: self.end - <$t>::ONE,
                    next: Some(self.start),
                    next_back: Some(self.end - <$t>::ONE),
                }
            }
        }

        #[derive(Copy, Clone, Debug)]
        pub struct $iter {
            start: $t,
            end: $t,
            next: Option<$t>,
            next_back: Option<$t>,
        }

        impl Iterator for $iter {
            type Item = $t;
            fn next(&mut self) -> Option<$t> {
                let current = self.next?;
                if self.next_back == Some(current) {
                    self.next = None;
                    self.next_back = None;
                    return Some(current);
                }
                $(
                    if self.next.unwrap().$field < self.end.$field {
                        self.next.as_mut().unwrap().$field += 1;
                        return Some(current);
                    }
                    self.next.as_mut().unwrap().$field = self.start.$field;
                )*
                self.next = None;
                self.next_back = None;
                Some(current)
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                let len = self.len();
                (len, Some(len))
            }
        }

        impl DoubleEndedIterator for $iter {
            fn next_back(&mut self) -> Option<$t> {
                let current = self.next_back?;
                if self.next == Some(current) {
                    self.next = None;
                    self.next_back = None;
                    return Some(current);
                }
                $(
                    if self.next_back.unwrap().$field > self.start.$field {
                        self.next_back.as_mut().unwrap().$field -= 1;
                        return Some(current);
                    }
                    self.next_back.as_mut().unwrap().$field = self.end.$field;
                )*
                self.next = None;
                self.next_back = None;
                Some(current)
            }
        }

        impl ExactSizeIterator for $iter {
            fn len(&self) -> usize {
                if self.is_empty() {
                    return 0;
                }
                let mut len = 1usize;
                $(
                    len *= (self.end.$field - self.start.$field + 1) as usize;
                )*
                len
            }

            fn is_empty(&self) -> bool {
                $(
                    if self.start.$field > self.end.$field {
                        return true;
                    }
                )*
                false
            }
        }
    }
}
int_range_impl!(glam::IVec2, IVec2RangeExtensions, IVec2RangeIter, x, y);
float_range_impl!(glam::Vec2, Vec2RangeExtensions, x, y);
float_range_impl!(glam::DVec2, DVec2RangeExtensions, x, y);
int_range_impl!(glam::IVec3, IVec3RangeExtensions, IVec3RangeIter, x, y, z);
float_range_impl!(glam::Vec3, Vec3RangeExtensions, x, y, z);
float_range_impl!(glam::DVec3, DVec3RangeExtensions, x, y, z);
int_range_impl!(glam::IVec4, IVec4RangeExtensions, IVec4RangeIter, x, y, z, w);
float_range_impl!(glam::Vec4, Vec4RangeExtensions, x, y, z, w);
float_range_impl!(glam::DVec4, DVec4RangeExtensions, x, y, z, w);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum Axis {
    #[serde(rename = "x")]
    X,
    #[serde(rename = "y")]
    Y,
    #[serde(rename = "z")]
    Z,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum Direction {
    #[serde(rename = "north")]
    North,
    #[serde(rename = "south")]
    South,
    #[serde(rename = "east")]
    East,
    #[serde(rename = "west")]
    West,
    #[serde(rename = "up")]
    Up,
    #[serde(rename = "down")]
    Down,
}

#[allow(non_upper_case_globals)]
impl Direction {
    pub const NegX: Direction = Direction::West;
    pub const NegY: Direction = Direction::Down;
    pub const NegZ: Direction = Direction::North;
    pub const PosX: Direction = Direction::East;
    pub const PosY: Direction = Direction::Up;
    pub const PosZ: Direction = Direction::South;

    pub const ALL: [Direction; 6] = [
        Direction::North,
        Direction::South,
        Direction::East,
        Direction::West,
        Direction::Up,
        Direction::Down,
    ];

    pub const HORIZONTAL: [Direction; 4] = [
        Direction::North,
        Direction::South,
        Direction::East,
        Direction::West,
    ];

    pub const VERTICAL: [Direction; 2] = [
        Direction::Up,
        Direction::Down,
    ];

    pub fn axis(self) -> Axis {
        match self {
            Direction::North => Axis::Z,
            Direction::South => Axis::Z,
            Direction::East => Axis::X,
            Direction::West => Axis::X,
            Direction::Up => Axis::Y,
            Direction::Down => Axis::Y,
        }
    }

    pub fn opposite(self) -> Self {
        match self {
            Direction::North => Direction::South,
            Direction::South => Direction::North,
            Direction::East => Direction::West,
            Direction::West => Direction::East,
            Direction::Up => Direction::Down,
            Direction::Down => Direction::Up,
        }
    }

    pub fn forward(self) -> BlockPos {
        match self {
            Direction::North => BlockPos::new(0, 0, -1),
            Direction::South => BlockPos::new(0, 0, 1),
            Direction::East => BlockPos::new(1, 0, 0),
            Direction::West => BlockPos::new(-1, 0, 0),
            Direction::Up => BlockPos::new(0, 1, 0),
            Direction::Down => BlockPos::new(0, -1, 0),
        }
    }

    pub fn from_vector(vector: glam::Vec3) -> Self {
        return *Direction::ALL.iter().max_by(|dir1, dir2| {
            let dir1_dot = vector.dot(dir1.forward().to_float());
            let dir2_dot = vector.dot(dir2.forward().to_float());
            dir1_dot.partial_cmp(&dir2_dot).unwrap()
        }).unwrap();
    }

    pub fn transform(self, transform: &glam::Mat4) -> Self {
        let forward = self.forward().to_float();
        let forward_transformed = transform.transform_vector3(forward);
        return Direction::from_vector(forward_transformed);
    }
}
