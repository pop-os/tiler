// SPDX-License-Identifier: LGPL-3.0-only
// Copyright © 2021 System76

/// The positioning and dimensions of a rectangular object.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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

    pub fn area(self) -> u32 {
        self.width * self.height
    }
}
