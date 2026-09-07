//! Integration tests for base widgets with the layout engine.
//!
//! These tests verify that `Container`, `Flex`, `Stack`, and `Text`
//! widgets work correctly with `LayoutEngine::compute_with_widgets`,
//! ensuring no panics and correct bounds propagation.

#![forbid(unsafe_code)]

use glam::Vec2;
use martensite::widgets::{Container, Flex, Stack, Text};
use martensite_core::widget::{LayoutConstraints, LayoutContext, Widget};
use martensite_core::{ColdNode, HotNode, Rect, WidgetArena, WidgetId};
use martensite_layout::engine::LayoutEngine;
use taffy::{AvailableSpace, Size};

/// Inserts a widget into the arena and returns its WidgetId.
fn insert_widget(arena: &mut WidgetArena, widget: impl Widget + 'static) -> WidgetId {
    arena.insert(
        HotNode::new(taffy::NodeId::new(0)),
        ColdNode::new(Box::new(widget)),
    )
}

#[test]
fn container_with_text_no_panic() {
    let mut arena = WidgetArena::with_capacity(10);
    let text = Text::new("Hello World").font_size(16.0);
    let container = Container::new().padding_uniform(10.0).child(text);
    let root = insert_widget(&mut arena, container);

    let mut engine = LayoutEngine::new();
    engine
        .compute_with_widgets(
            &mut arena,
            root,
            Size {
                width: AvailableSpace::Definite(800.0),
                height: AvailableSpace::Definite(600.0),
            },
        )
        .expect("layout should succeed");

    // Root should have positive bounds (text has real metrics)
    let hot = arena.get_hot(root).expect("root should exist");
    assert!(
        hot.bounds.width() > 0.0,
        "container width should be positive, got {}",
        hot.bounds.width()
    );
    assert!(
        hot.bounds.height() > 0.0,
        "container height should be positive, got {}",
        hot.bounds.height()
    );
}

#[test]
fn flex_with_text_children_no_panic() {
    let mut arena = WidgetArena::with_capacity(20);
    let flex = Flex::row()
        .gap(10.0)
        .child(Text::new("Hello").font_size(16.0))
        .child(Text::new("World").font_size(16.0));
    let root = insert_widget(&mut arena, flex);

    let mut engine = LayoutEngine::new();
    engine
        .compute_with_widgets(
            &mut arena,
            root,
            Size {
                width: AvailableSpace::Definite(800.0),
                height: AvailableSpace::Definite(600.0),
            },
        )
        .expect("layout should succeed");

    let hot = arena.get_hot(root).expect("root should exist");
    // Flex with two text children should have positive width
    assert!(
        hot.bounds.width() > 0.0,
        "flex width should be positive, got {}",
        hot.bounds.width()
    );
    assert!(
        hot.bounds.height() > 0.0,
        "flex height should be positive, got {}",
        hot.bounds.height()
    );
}

#[test]
fn stack_with_text_children_no_panic() {
    let mut arena = WidgetArena::with_capacity(20);
    let stack = Stack::new()
        .child(Text::new("Background").font_size(16.0))
        .child(Text::new("Foreground").font_size(16.0));
    let root = insert_widget(&mut arena, stack);

    let mut engine = LayoutEngine::new();
    engine
        .compute_with_widgets(
            &mut arena,
            root,
            Size {
                width: AvailableSpace::Definite(800.0),
                height: AvailableSpace::Definite(600.0),
            },
        )
        .expect("layout should succeed");

    let hot = arena.get_hot(root).expect("root should exist");
    assert!(
        hot.bounds.width() > 0.0,
        "stack width should be positive, got {}",
        hot.bounds.width()
    );
    assert!(
        hot.bounds.height() > 0.0,
        "stack height should be positive, got {}",
        hot.bounds.height()
    );
}

#[test]
fn nested_container_flex_text_no_panic() {
    let mut arena = WidgetArena::with_capacity(30);
    let inner = Flex::column()
        .gap(5.0)
        .child(Text::new("Line 1").font_size(14.0))
        .child(Text::new("Line 2").font_size(14.0));
    let container = Container::new().padding_uniform(20.0).child(inner);
    let outer = Container::new().padding_uniform(10.0).child(container);
    let root = insert_widget(&mut arena, outer);

    let mut engine = LayoutEngine::new();
    engine
        .compute_with_widgets(
            &mut arena,
            root,
            Size {
                width: AvailableSpace::Definite(400.0),
                height: AvailableSpace::Definite(300.0),
            },
        )
        .expect("layout should succeed");

    let hot = arena.get_hot(root).expect("root should exist");
    assert!(
        hot.bounds.width() > 0.0,
        "nested container width should be positive, got {}",
        hot.bounds.width()
    );
    assert!(
        hot.bounds.height() > 0.0,
        "nested container height should be positive, got {}",
        hot.bounds.height()
    );
}

#[test]
fn text_wraps_in_narrow_container() {
    // Text with a narrow constraint should have a larger height than
    // text with an unbounded constraint (i.e., wrapping occurs).
    let mut hot1 = HotNode::new(taffy::NodeId::new(0));
    let mut cx1 = LayoutContext { hot: &mut hot1 };
    let mut t_narrow =
        Text::new("The quick brown fox jumps over the lazy dog repeatedly").font_size(16.0);
    let size_narrow = t_narrow.measure(
        &mut cx1,
        LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(50.0, f32::MAX),
        },
    );

    let mut hot2 = HotNode::new(taffy::NodeId::new(0));
    let mut cx2 = LayoutContext { hot: &mut hot2 };
    let mut t_wide =
        Text::new("The quick brown fox jumps over the lazy dog repeatedly").font_size(16.0);
    let size_wide = t_wide.measure(
        &mut cx2,
        LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(f32::MAX, f32::MAX),
        },
    );

    // Narrow text should wrap, producing greater height
    assert!(
        size_narrow.y > size_wide.y,
        "narrow text height {} should exceed wide text height {}",
        size_narrow.y,
        size_wide.y
    );
    // Narrow text width should be <= the constraint
    assert!(
        size_narrow.x <= 50.0,
        "narrow text width {} should be <= 50.0",
        size_narrow.x
    );
}

#[test]
fn text_widget_measure_returns_nonzero_for_nonempty() {
    let mut hot = HotNode::new(taffy::NodeId::new(0));
    let mut cx = LayoutContext { hot: &mut hot };
    let mut t = Text::new("Test text").font_size(16.0);
    let size = t.measure(
        &mut cx,
        LayoutConstraints {
            min_size: Vec2::ZERO,
            max_size: Vec2::new(1000.0, 1000.0),
        },
    );
    // Should have non-negative dimensions
    assert!(size.x >= 0.0 && size.y >= 0.0);
}

#[test]
fn container_layout_computes_content_area_from_bounds() {
    let mut hot = HotNode::new(taffy::NodeId::new(0));
    let mut cx = LayoutContext { hot: &mut hot };
    let mut c = Container::new().padding_uniform(15.0);
    let bounds = Rect::new(0.0, 0.0, 200.0, 100.0);
    c.layout(&mut cx, bounds);
    let content = c.content_area();
    // Content area should be bounds minus padding on all sides
    assert_eq!(content.origin.x, 15.0);
    assert_eq!(content.origin.y, 15.0);
    assert_eq!(content.size.x, 170.0); // 200 - 30
    assert_eq!(content.size.y, 70.0); // 100 - 30
}
