// Copyright 2021 System76 <info@system76.com>
// SPDX-License-Identifier: MPL-2.0

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Point {
    x: u32,
    y: u32,
}

impl Point {
    pub fn distance(self, other: Point) -> f64 {
        (((other.x - self.x).pow(2) + (other.y - self.y).pow(2)) as f64).sqrt()
    }

    pub fn distance_from_rect(&self, rect: &Rect) -> f64 {
        self.distance(rect.north())
            .min(self.distance(rect.south()))
            .min(self.distance(rect.east()))
            .min(self.distance(rect.west()))
    }
}

/// The positioning and dimensions of a rectangular object.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn area(&self) -> u32 {
        self.width * self.height
    }

    pub fn distance_downward(&self, other: &Rect) -> f64 {
        self.south().distance(other.north())
    }

    pub fn distance_eastward(&self, other: &Rect) -> f64 {
        self.west().distance(other.east())
    }

    pub fn distance_upward(&self, other: &Rect) -> f64 {
        self.north().distance(other.south())
    }

    pub fn distance_westward(&self, other: &Rect) -> f64 {
        self.east().distance(other.west())
    }

    pub fn is_below(&self, other: &Rect) -> bool {
        self.y < other.y
    }

    pub fn is_above(&self, other: &Rect) -> bool {
        self.y > other.y
    }

    pub fn is_left(&self, other: &Rect) -> bool {
        self.x > other.x
    }

    pub fn is_right(&self, other: &Rect) -> bool {
        self.x < other.x
    }

    pub fn east(&self) -> Point {
        Point {
            x: self.x_end(),
            y: self.y_center(),
        }
    }

    pub fn north(&self) -> Point {
        Point {
            x: self.x_center(),
            y: self.y,
        }
    }

    pub fn south(&self) -> Point {
        Point {
            x: self.x_center(),
            y: self.y_end(),
        }
    }

    pub fn west(&self) -> Point {
        Point {
            x: self.x,
            y: self.y_center(),
        }
    }

    pub fn x_center(&self) -> u32 {
        self.x + self.width / 2
    }

    pub fn x_end(&self) -> u32 {
        self.x + self.width
    }

    pub fn y_center(&self) -> u32 {
        self.y + self.height / 2
    }

    pub fn y_end(&self) -> u32 {
        self.y + self.height
    }
}
