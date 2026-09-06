//! Intermediate PaintList stream and software rasterization fallback.
#![forbid(unsafe_code)]

use kurbo::{Point, Rect};

pub enum PaintCommand {
    FillRect(Rect, [u8; 4]),
    StrokeRect(Rect, f32, [u8; 4]),
    DrawText(Point, String, f32, [u8; 4]),
}

pub struct PaintList {
    pub commands: Vec<PaintCommand>,
}

impl PaintList {
    pub fn new() -> Self {
        Self { commands: Vec::new() }
    }

    pub fn clear(&mut self) {
        self.commands.clear();
    }
}

pub trait RenderBackend: Send + 'static {
    fn render(&mut self, paint_list: &PaintList);
}
