//! Comprehensive integration and unit test suite for `martensite_devtools::event_ledger`.
//!
//! Tests the preallocated zero-allocation event ledger ring buffer, hit paths,
//! event records, dispositions, rejections, filtering, and diagnostic formatting.

use martensite_core::WidgetId;
use martensite_devtools::event_ledger::{
    is_debug_events_enabled, set_debug_events_enabled, Disposition, EventFilter, EventKind,
    EventLedger, EventRecord, HitPath, HitRejection, Point, DEFAULT_LEDGER_CAPACITY,
    HIT_PATH_CAPACITY,
};

// ============================================================================
// 1. Ring Buffer Wraparound and FIFO Eviction Tests
// ============================================================================

#[test]
fn test_ledger_default_state() {
    let ledger = EventLedger::new();
    assert_eq!(ledger.capacity(), DEFAULT_LEDGER_CAPACITY);
    assert_eq!(ledger.capacity(), 1024);
    assert_eq!(ledger.len(), 0);
    assert!(ledger.is_empty());
    assert!(ledger.oldest().is_none());
    assert!(ledger.newest().is_none());
    assert!(ledger.get(0).is_none());

    // Default trait
    let default_ledger: EventLedger = Default::default();
    assert_eq!(default_ledger.capacity(), DEFAULT_LEDGER_CAPACITY);
    assert!(default_ledger.is_empty());
}

#[test]
fn test_ledger_with_capacity_clamping() {
    // Capacity 0 must clamp to 1 to avoid zero-division or zero-size buffer
    let ledger_zero = EventLedger::with_capacity(0);
    assert_eq!(ledger_zero.capacity(), 1);
    assert!(ledger_zero.is_empty());

    let ledger_custom = EventLedger::with_capacity(16);
    assert_eq!(ledger_custom.capacity(), 16);
    assert!(ledger_custom.is_empty());
}

#[test]
fn test_ledger_push_under_capacity() {
    let mut ledger = EventLedger::with_capacity(5);
    let target = WidgetId::from_parts(10, 1);

    for i in 1..=3 {
        let record = EventRecord::new(i, 100, EventKind::Key, Disposition::Handled(target));
        ledger.push(record);
        assert_eq!(ledger.len(), i as usize);
        assert!(!ledger.is_empty());
    }

    assert_eq!(ledger.len(), 3);
    assert_eq!(ledger.oldest().unwrap().seq, 1);
    assert_eq!(ledger.newest().unwrap().seq, 3);
    assert_eq!(ledger.get(0).unwrap().seq, 1);
    assert_eq!(ledger.get(1).unwrap().seq, 2);
    assert_eq!(ledger.get(2).unwrap().seq, 3);
    assert!(ledger.get(3).is_none());
}

#[test]
fn test_ledger_exact_capacity() {
    let mut ledger = EventLedger::with_capacity(4);
    for i in 1..=4 {
        ledger.push(EventRecord::new(
            i,
            1,
            EventKind::Pointer,
            Disposition::Ignored,
        ));
    }

    assert_eq!(ledger.len(), 4);
    assert_eq!(ledger.capacity(), 4);
    assert_eq!(ledger.oldest().unwrap().seq, 1);
    assert_eq!(ledger.newest().unwrap().seq, 4);

    let seqs: Vec<u64> = ledger.iter().map(|r| r.seq).collect();
    assert_eq!(seqs, vec![1, 2, 3, 4]);
}

#[test]
fn test_ledger_wraparound_fifo_eviction() {
    let mut ledger = EventLedger::with_capacity(4);

    // Push 10 items into capacity-4 buffer (wraps multiple times)
    for i in 1..=10 {
        ledger.push(EventRecord::new(
            i,
            1,
            EventKind::Pointer,
            Disposition::Ignored,
        ));
        let expected_len = (i as usize).min(4);
        assert_eq!(ledger.len(), expected_len);
        assert_eq!(ledger.newest().unwrap().seq, i);

        let expected_oldest = if i <= 4 { 1 } else { i - 3 };
        assert_eq!(ledger.oldest().unwrap().seq, expected_oldest);
    }

    // Now capacity is 4, elements 7, 8, 9, 10 remain
    assert_eq!(ledger.len(), 4);
    assert_eq!(ledger.get(0).unwrap().seq, 7);
    assert_eq!(ledger.get(1).unwrap().seq, 8);
    assert_eq!(ledger.get(2).unwrap().seq, 9);
    assert_eq!(ledger.get(3).unwrap().seq, 10);
    assert!(ledger.get(4).is_none());

    let seqs: Vec<u64> = ledger.iter().map(|r| r.seq).collect();
    assert_eq!(seqs, vec![7, 8, 9, 10]);

    // Push one more to verify next wrap
    ledger.push(EventRecord::new(
        11,
        1,
        EventKind::Pointer,
        Disposition::Ignored,
    ));
    assert_eq!(ledger.len(), 4);
    assert_eq!(ledger.oldest().unwrap().seq, 8);
    assert_eq!(ledger.newest().unwrap().seq, 11);
    let seqs_after: Vec<u64> = ledger.iter().map(|r| r.seq).collect();
    assert_eq!(seqs_after, vec![8, 9, 10, 11]);
}

#[test]
fn test_ledger_single_capacity_eviction() {
    let mut ledger = EventLedger::with_capacity(1);
    assert_eq!(ledger.capacity(), 1);

    ledger.push(EventRecord::new(1, 1, EventKind::Key, Disposition::Ignored));
    assert_eq!(ledger.len(), 1);
    assert_eq!(ledger.oldest().unwrap().seq, 1);
    assert_eq!(ledger.newest().unwrap().seq, 1);

    ledger.push(EventRecord::new(2, 1, EventKind::Key, Disposition::Ignored));
    assert_eq!(ledger.len(), 1);
    assert_eq!(ledger.oldest().unwrap().seq, 2);
    assert_eq!(ledger.newest().unwrap().seq, 2);
    assert_eq!(ledger.get(0).unwrap().seq, 2);
    assert!(ledger.get(1).is_none());
}

#[test]
fn test_ledger_auto_sequence_number_assignment() {
    let mut ledger = EventLedger::with_capacity(5);

    // Records with seq = 0 should be automatically assigned monotonic sequences 1, 2, 3...
    ledger.push(EventRecord::new(
        0,
        10,
        EventKind::Pointer,
        Disposition::Ignored,
    ));
    ledger.push(EventRecord::new(
        0,
        10,
        EventKind::Key,
        Disposition::Ignored,
    ));
    ledger.push(EventRecord::new(
        0,
        10,
        EventKind::Scroll,
        Disposition::Ignored,
    ));

    assert_eq!(ledger.get(0).unwrap().seq, 1);
    assert_eq!(ledger.get(1).unwrap().seq, 2);
    assert_eq!(ledger.get(2).unwrap().seq, 3);

    // Explicit higher sequence sets high water mark
    ledger.push(EventRecord::new(
        100,
        10,
        EventKind::Ime,
        Disposition::Ignored,
    ));
    assert_eq!(ledger.get(3).unwrap().seq, 100);

    // Subsequent zero sequence continues from new counter
    ledger.push(EventRecord::new(
        0,
        10,
        EventKind::Focus,
        Disposition::Ignored,
    ));
    assert_eq!(ledger.get(4).unwrap().seq, 101);
}

#[test]
fn test_ledger_clear() {
    let mut ledger = EventLedger::with_capacity(4);
    for i in 1..=4 {
        ledger.push(EventRecord::new(
            i,
            1,
            EventKind::Pointer,
            Disposition::Ignored,
        ));
    }
    assert_eq!(ledger.len(), 4);

    ledger.clear();
    assert_eq!(ledger.len(), 0);
    assert!(ledger.is_empty());
    assert_eq!(ledger.capacity(), 4);
    assert!(ledger.oldest().is_none());
    assert!(ledger.newest().is_none());
    assert!(ledger.get(0).is_none());

    // Can push again after clear
    ledger.push(EventRecord::new(
        42,
        1,
        EventKind::Pointer,
        Disposition::Ignored,
    ));
    assert_eq!(ledger.len(), 1);
    assert_eq!(ledger.oldest().unwrap().seq, 42);
    assert_eq!(ledger.newest().unwrap().seq, 42);
}

#[test]
fn test_ledger_drain() {
    let mut ledger = EventLedger::with_capacity(3);
    for i in 1..=5 {
        ledger.push(EventRecord::new(
            i,
            1,
            EventKind::Pointer,
            Disposition::Ignored,
        ));
    }
    // Capacity 3: records 3, 4, 5
    assert_eq!(ledger.len(), 3);

    {
        let mut drain = ledger.drain();
        assert_eq!(drain.len(), 3);
        let (lower, upper) = drain.size_hint();
        assert_eq!(lower, 3);
        assert_eq!(upper, Some(3));

        assert_eq!(drain.next().map(|r| r.seq), Some(3));
        assert_eq!(drain.len(), 2);
        assert_eq!(drain.next().map(|r| r.seq), Some(4));
        assert_eq!(drain.next().map(|r| r.seq), Some(5));
        assert_eq!(drain.next(), None);
        assert_eq!(drain.len(), 0);
    }

    // Ledger is empty after drain completes
    assert_eq!(ledger.len(), 0);
    assert!(ledger.is_empty());
    assert_eq!(ledger.capacity(), 3);

    // Can push again with no issue
    ledger.push(EventRecord::new(
        100,
        2,
        EventKind::Key,
        Disposition::Ignored,
    ));
    assert_eq!(ledger.len(), 1);
    assert_eq!(ledger.oldest().unwrap().seq, 100);
}

#[test]
fn test_ledger_drain_partial_drop() {
    let mut ledger = EventLedger::with_capacity(4);
    for i in 1..=4 {
        ledger.push(EventRecord::new(
            i,
            1,
            EventKind::Pointer,
            Disposition::Ignored,
        ));
    }

    {
        let mut drain = ledger.drain();
        let first = drain.next();
        assert_eq!(first.map(|r| r.seq), Some(1));
        // Drop drain early without exhausting
    }

    // Ledger should still be reset by Drop
    assert_eq!(ledger.len(), 0);
    assert!(ledger.is_empty());
}

#[test]
fn test_ledger_iter_and_double_ended() {
    let mut ledger = EventLedger::with_capacity(4);
    for i in 1..=6 {
        ledger.push(EventRecord::new(
            i,
            1,
            EventKind::Pointer,
            Disposition::Ignored,
        ));
    }
    // Stored: [3, 4, 5, 6]
    let mut iter = ledger.iter();
    assert_eq!(iter.len(), 4);

    // Front: 3
    assert_eq!(iter.next().map(|r| r.seq), Some(3));
    assert_eq!(iter.len(), 3);

    // Back: 6
    assert_eq!(iter.next_back().map(|r| r.seq), Some(6));
    assert_eq!(iter.len(), 2);

    // Front: 4
    assert_eq!(iter.next().map(|r| r.seq), Some(4));
    assert_eq!(iter.len(), 1);

    // Back: 5
    assert_eq!(iter.next_back().map(|r| r.seq), Some(5));
    assert_eq!(iter.len(), 0);

    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_back(), None);

    // IntoIterator for &EventLedger
    let seqs_from_ref: Vec<u64> = (&ledger).into_iter().map(|r| r.seq).collect();
    assert_eq!(seqs_from_ref, vec![3, 4, 5, 6]);
}

// ============================================================================
// 2. EventRecord and EventKind Tests
// ============================================================================

#[test]
fn test_event_kind_variants_and_display() {
    let kinds = [
        (EventKind::Pointer, "Pointer"),
        (EventKind::Key, "Key"),
        (EventKind::Scroll, "Scroll"),
        (EventKind::Ime, "Ime"),
        (EventKind::Focus, "Focus"),
        (EventKind::Dnd, "Dnd"),
    ];

    for (kind, expected_str) in kinds {
        assert_eq!(format!("{kind}"), expected_str);
    }

    assert_eq!(EventKind::default(), EventKind::Pointer);
}

#[test]
fn test_event_record_builder_and_fields() {
    let target = WidgetId::from_parts(10, 1);
    let ancestor = WidgetId::from_parts(5, 1);
    let from_focus = WidgetId::from_parts(2, 1);
    let to_focus = WidgetId::from_parts(3, 1);

    let mut path = HitPath::new();
    path.push(ancestor);
    path.push(target);

    let record = EventRecord::new(10, 20, EventKind::Pointer, Disposition::Handled(target))
        .with_position(Point::new(150.5, 300.0))
        .with_hit_path(path)
        .with_hit_rejection(HitRejection::OutsideBounds)
        .with_focus(Some(from_focus), Some(to_focus))
        .with_timestamp(1_234_567_890);

    assert_eq!(record.seq, 10);
    assert_eq!(record.frame, 20);
    assert_eq!(record.kind, EventKind::Pointer);
    assert_eq!(record.position, Some(Point::new(150.5, 300.0)));
    assert_eq!(record.hit_path.len(), 2);
    assert_eq!(record.hit_path.last(), Some(target));
    assert_eq!(record.hit_rejection, Some(HitRejection::OutsideBounds));
    assert_eq!(record.disposition, Disposition::Handled(target));
    assert_eq!(record.focus_from, Some(from_focus));
    assert_eq!(record.focus_to, Some(to_focus));
    assert_eq!(record.timestamp, 1_234_567_890);
}

#[test]
fn test_point_conversions_and_display() {
    let p = Point::new(10.5, 20.0);
    assert_eq!(p.x, 10.5);
    assert_eq!(p.y, 20.0);
    assert_eq!(format!("{p}"), "10.5,20");

    let p_int = Point::new(412.0, 301.0);
    assert_eq!(format!("{p_int}"), "412,301");

    // From glam::Vec2
    let gv = glam::Vec2::new(5.0, 7.5);
    let p_from_gv: Point = gv.into();
    assert_eq!(p_from_gv, Point::new(5.0, 7.5));

    // Into glam::Vec2
    let gv_back: glam::Vec2 = p_from_gv.into();
    assert_eq!(gv_back.x, 5.0);
    assert_eq!(gv_back.y, 7.5);

    // From tuple
    let p_tuple: Point = (12.0, 34.0).into();
    assert_eq!(p_tuple, Point::new(12.0, 34.0));

    // From array
    let p_array: Point = [56.0, 78.0].into();
    assert_eq!(p_array, Point::new(56.0, 78.0));
}

// ============================================================================
// 3. HitPath Inline Capacity Bounds and Traversal
// ============================================================================

#[test]
fn test_hit_path_empty() {
    let path = HitPath::new();
    assert_eq!(path.len(), 0);
    assert!(path.is_empty());
    assert!(!path.is_truncated());
    assert_eq!(path.first(), None);
    assert_eq!(path.last(), None);
    assert_eq!(path.get(0), None);
    assert_eq!(path.to_vec(), Vec::<WidgetId>::new());
    assert_eq!(format!("{path}"), "");

    let default_path: HitPath = Default::default();
    assert!(default_path.is_empty());
}

#[test]
fn test_hit_path_within_capacity() {
    let mut path = HitPath::new();
    let id1 = WidgetId::from_parts(1, 1);
    let id2 = WidgetId::from_parts(2, 1);
    let id3 = WidgetId::from_parts(3, 1);

    path.push(id1);
    path.push(id2);
    path.push(id3);

    assert_eq!(path.len(), 3);
    assert!(!path.is_empty());
    assert!(!path.is_truncated());
    assert_eq!(path.first(), Some(id1));
    assert_eq!(path.last(), Some(id3));
    assert_eq!(path.get(0), Some(id1));
    assert_eq!(path.get(1), Some(id2));
    assert_eq!(path.get(2), Some(id3));
    assert_eq!(path.get(3), None);

    assert!(path.contains(id1));
    assert!(path.contains(id2));
    assert!(path.contains(id3));
    assert!(!path.contains(WidgetId::from_parts(99, 1)));

    // Index operator
    assert_eq!(path[0], id1);
    assert_eq!(path[1], id2);
    assert_eq!(path[2], id3);

    // Display
    assert_eq!(format!("{path}"), "1:1/2:1/3:1");
}

#[test]
fn test_hit_path_capacity_boundary_and_truncation() {
    assert_eq!(HIT_PATH_CAPACITY, 16);
    let mut path = HitPath::new();

    // Fill to exact capacity (16 entries)
    for i in 0..16 {
        path.push(WidgetId::from_parts(i as u32 + 1, 1));
    }

    assert_eq!(path.len(), 16);
    assert!(!path.is_truncated());
    assert_eq!(path.first(), Some(WidgetId::from_parts(1, 1)));
    assert_eq!(path.last(), Some(WidgetId::from_parts(16, 1)));

    // Push 17th, 18th, 19th items: should be ignored, sets truncated = true
    path.push(WidgetId::from_parts(17, 1));
    path.push(WidgetId::from_parts(18, 1));
    path.push(WidgetId::from_parts(19, 1));

    assert_eq!(path.len(), 16);
    assert!(path.is_truncated());
    assert_eq!(path.first(), Some(WidgetId::from_parts(1, 1)));
    assert_eq!(path.last(), Some(WidgetId::from_parts(16, 1)));
    assert!(!path.contains(WidgetId::from_parts(17, 1)));

    // Display formatting shows truncated marker
    let display_str = format!("{path}");
    assert!(display_str.ends_with("/...truncated"));
    assert!(display_str.starts_with("1:1/2:1/"));
}

#[test]
#[should_panic(expected = "index out of bounds")]
fn test_hit_path_index_out_of_bounds() {
    let mut path = HitPath::new();
    path.push(WidgetId::from_parts(1, 1));
    let _ = path[1];
}

#[test]
fn test_hit_path_conversions() {
    let id1 = WidgetId::from_parts(1, 1);
    let id2 = WidgetId::from_parts(2, 1);
    let id3 = WidgetId::from_parts(3, 1);

    // From slice
    let slice = [id1, id2];
    let path_slice = HitPath::from(&slice[..]);
    assert_eq!(path_slice.len(), 2);
    assert_eq!(path_slice[0], id1);
    assert_eq!(path_slice[1], id2);

    // From array
    let path_arr = HitPath::from([id1, id2, id3]);
    assert_eq!(path_arr.len(), 3);
    assert_eq!(path_arr[2], id3);

    // FromIterator
    let path_iter: HitPath = vec![id1, id2].into_iter().collect();
    assert_eq!(path_iter.len(), 2);
    assert_eq!(path_iter.to_vec(), vec![id1, id2]);
}

#[test]
fn test_hit_path_iter_double_ended() {
    let id1 = WidgetId::from_parts(1, 1);
    let id2 = WidgetId::from_parts(2, 1);
    let id3 = WidgetId::from_parts(3, 1);

    let path = HitPath::from([id1, id2, id3]);
    let mut iter = path.iter();

    assert_eq!(iter.len(), 3);
    let (min, max) = iter.size_hint();
    assert_eq!(min, 3);
    assert_eq!(max, Some(3));

    assert_eq!(iter.next(), Some(id1));
    assert_eq!(iter.len(), 2);
    assert_eq!(iter.next_back(), Some(id3));
    assert_eq!(iter.len(), 1);
    assert_eq!(iter.next(), Some(id2));
    assert_eq!(iter.len(), 0);
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_back(), None);
}

// ============================================================================
// 4. Filtering Operations Tests
// ============================================================================

#[test]
fn test_filter_by_kind() {
    let mut ledger = EventLedger::new();
    ledger.push(EventRecord::new(
        1,
        1,
        EventKind::Pointer,
        Disposition::Ignored,
    ));
    ledger.push(EventRecord::new(2, 1, EventKind::Key, Disposition::Ignored));
    ledger.push(EventRecord::new(
        3,
        1,
        EventKind::Pointer,
        Disposition::Ignored,
    ));
    ledger.push(EventRecord::new(
        4,
        1,
        EventKind::Scroll,
        Disposition::Ignored,
    ));
    ledger.push(EventRecord::new(
        5,
        1,
        EventKind::Pointer,
        Disposition::Ignored,
    ));

    let ptr_records: Vec<u64> = ledger
        .filter_by_kind(EventKind::Pointer)
        .map(|r| r.seq)
        .collect();
    assert_eq!(ptr_records, vec![1, 3, 5]);

    let key_records: Vec<u64> = ledger
        .filter_by_kind(EventKind::Key)
        .map(|r| r.seq)
        .collect();
    assert_eq!(key_records, vec![2]);

    let dnd_records: Vec<u64> = ledger
        .filter_by_kind(EventKind::Dnd)
        .map(|r| r.seq)
        .collect();
    assert!(dnd_records.is_empty());
}

#[test]
fn test_filter_by_frame() {
    let mut ledger = EventLedger::new();
    ledger.push(EventRecord::new(
        1,
        100,
        EventKind::Pointer,
        Disposition::Ignored,
    ));
    ledger.push(EventRecord::new(
        2,
        100,
        EventKind::Key,
        Disposition::Ignored,
    ));
    ledger.push(EventRecord::new(
        3,
        101,
        EventKind::Scroll,
        Disposition::Ignored,
    ));
    ledger.push(EventRecord::new(
        4,
        102,
        EventKind::Ime,
        Disposition::Ignored,
    ));

    let frame_100: Vec<u64> = ledger.filter_by_frame(100).map(|r| r.seq).collect();
    assert_eq!(frame_100, vec![1, 2]);

    let frame_101: Vec<u64> = ledger.filter_by_frame(101).map(|r| r.seq).collect();
    assert_eq!(frame_101, vec![3]);

    let frame_999: Vec<u64> = ledger.filter_by_frame(999).map(|r| r.seq).collect();
    assert!(frame_999.is_empty());
}

#[test]
fn test_filter_by_widget_and_mentions_widget() {
    let target = WidgetId::from_parts(10, 1);
    let occluder = WidgetId::from_parts(20, 1);
    let captured_by = WidgetId::from_parts(30, 1);
    let focus_old = WidgetId::from_parts(40, 1);
    let focus_new = WidgetId::from_parts(50, 1);
    let unrelated = WidgetId::from_parts(99, 1);

    let mut ledger = EventLedger::new();

    // 1. Mentioned via hit path
    let mut path = HitPath::new();
    path.push(target);
    ledger
        .push(EventRecord::new(1, 1, EventKind::Pointer, Disposition::Ignored).with_hit_path(path));

    // 2. Mentioned via disposition Handled
    ledger.push(EventRecord::new(
        2,
        1,
        EventKind::Pointer,
        Disposition::Handled(target),
    ));

    // 3. Mentioned via disposition BubbledTo
    ledger.push(EventRecord::new(
        3,
        1,
        EventKind::Key,
        Disposition::BubbledTo(target),
    ));

    // 4. Mentioned via disposition Captured
    ledger.push(EventRecord::new(
        4,
        1,
        EventKind::Pointer,
        Disposition::Captured(target),
    ));

    // 5. Mentioned via hit rejection OccludedBy
    ledger.push(
        EventRecord::new(5, 1, EventKind::Pointer, Disposition::Ignored)
            .with_hit_rejection(HitRejection::OccludedBy(occluder)),
    );

    // 6. Mentioned via hit rejection CapturedByOther
    ledger.push(
        EventRecord::new(6, 1, EventKind::Pointer, Disposition::Ignored)
            .with_hit_rejection(HitRejection::CapturedByOther(captured_by)),
    );

    // 7. Mentioned via focus_from
    ledger.push(
        EventRecord::new(7, 1, EventKind::Focus, Disposition::Ignored)
            .with_focus(Some(focus_old), None),
    );

    // 8. Mentioned via focus_to
    ledger.push(
        EventRecord::new(8, 1, EventKind::Focus, Disposition::Ignored)
            .with_focus(None, Some(focus_new)),
    );

    // Check filter_by_widget for target (seq 1, 2, 3, 4)
    let target_seqs: Vec<u64> = ledger.filter_by_widget(target).map(|r| r.seq).collect();
    assert_eq!(target_seqs, vec![1, 2, 3, 4]);

    // Check occluder (seq 5)
    let occluder_seqs: Vec<u64> = ledger.filter_by_widget(occluder).map(|r| r.seq).collect();
    assert_eq!(occluder_seqs, vec![5]);

    // Check captured_by (seq 6)
    let captured_seqs: Vec<u64> = ledger
        .filter_by_widget(captured_by)
        .map(|r| r.seq)
        .collect();
    assert_eq!(captured_seqs, vec![6]);

    // Check focus_old (seq 7)
    let focus_old_seqs: Vec<u64> = ledger.filter_by_widget(focus_old).map(|r| r.seq).collect();
    assert_eq!(focus_old_seqs, vec![7]);

    // Check focus_new (seq 8)
    let focus_new_seqs: Vec<u64> = ledger.filter_by_widget(focus_new).map(|r| r.seq).collect();
    assert_eq!(focus_new_seqs, vec![8]);

    // Check unrelated (none)
    let unrelated_seqs: Vec<u64> = ledger.filter_by_widget(unrelated).map(|r| r.seq).collect();
    assert!(unrelated_seqs.is_empty());
}

#[test]
fn test_query_and_event_filter() {
    let target = WidgetId::from_parts(10, 1);
    let other = WidgetId::from_parts(20, 1);

    let mut ledger = EventLedger::new();
    ledger.push(EventRecord::new(
        1,
        100,
        EventKind::Pointer,
        Disposition::Handled(target),
    ));
    ledger.push(EventRecord::new(
        2,
        100,
        EventKind::Pointer,
        Disposition::Handled(other),
    ));
    ledger.push(EventRecord::new(
        3,
        101,
        EventKind::Key,
        Disposition::Handled(target),
    ));
    ledger.push(EventRecord::new(
        4,
        101,
        EventKind::Pointer,
        Disposition::Handled(target),
    ));

    // Custom query predicate
    let query_res: Vec<u64> = ledger
        .query(|r| r.frame == 101 && r.kind == EventKind::Pointer)
        .map(|r| r.seq)
        .collect();
    assert_eq!(query_res, vec![4]);

    // Empty EventFilter matches all records
    let filter_all = EventFilter::new();
    assert_eq!(ledger.filter(filter_all).count(), 4);

    // Filter by kind + frame
    let filter_kind_frame = EventFilter::new()
        .with_kind(EventKind::Pointer)
        .with_frame(100);
    let res: Vec<u64> = ledger.filter(filter_kind_frame).map(|r| r.seq).collect();
    assert_eq!(res, vec![1, 2]);

    // Filter by widget + kind
    let filter_widget_kind = EventFilter::new()
        .with_widget(target)
        .with_kind(EventKind::Pointer);
    let res: Vec<u64> = ledger.filter(filter_widget_kind).map(|r| r.seq).collect();
    assert_eq!(res, vec![1, 4]);

    // Filter by widget + kind + frame
    let filter_all_three = EventFilter::new()
        .with_widget(target)
        .with_kind(EventKind::Pointer)
        .with_frame(100);
    let res: Vec<u64> = ledger.filter(filter_all_three).map(|r| r.seq).collect();
    assert_eq!(res, vec![1]);

    // Non-matching filter
    let filter_none = EventFilter::new().with_widget(other).with_frame(101);
    assert_eq!(ledger.filter(filter_none).count(), 0);
}

// ============================================================================
// 5. HitRejection Variants Tests
// ============================================================================

#[test]
fn test_hit_rejection_variants_and_occluding_widget() {
    let occluder = WidgetId::from_parts(42, 2);
    let capturer = WidgetId::from_parts(84, 3);

    let rejections = [
        (HitRejection::OutsideBounds, None, "OutsideBounds"),
        (
            HitRejection::OccludedBy(occluder),
            Some(occluder),
            "OccludedBy(42:2)",
        ),
        (HitRejection::HitTestDisabled, None, "HitTestDisabled"),
        (HitRejection::UnderModal, None, "UnderModal"),
        (
            HitRejection::CapturedByOther(capturer),
            Some(capturer),
            "CapturedByOther(84:3)",
        ),
    ];

    for (rejection, expected_occluding, expected_display) in rejections {
        assert_eq!(rejection.occluding_widget(), expected_occluding);
        assert_eq!(format!("{rejection}"), expected_display);
    }
}

// ============================================================================
// 6. Disposition Variants Tests
// ============================================================================

#[test]
fn test_disposition_variants_and_target_widget() {
    let target = WidgetId::from_parts(15, 1);

    let dispositions = [
        (Disposition::Handled(target), Some(target), "Handled(15:1)"),
        (Disposition::Ignored, None, "Ignored"),
        (
            Disposition::BubbledTo(target),
            Some(target),
            "BubbledTo(15:1)",
        ),
        (
            Disposition::Captured(target),
            Some(target),
            "Captured(15:1)",
        ),
    ];

    for (disp, expected_target, expected_display) in dispositions {
        assert_eq!(disp.target_widget(), expected_target);
        assert_eq!(format!("{disp}"), expected_display);
    }

    assert_eq!(Disposition::default(), Disposition::Ignored);
}

// ============================================================================
// 7. Diagnostic Formatting and Debug Toggle Tests
// ============================================================================

#[test]
fn test_diagnostic_formatting_spec_examples() {
    let button_id = WidgetId::from_parts(42, 1);

    // ptr@ 412,301 → hit[42:1] handled
    let mut record = EventRecord::new(1, 100, EventKind::Pointer, Disposition::Handled(button_id))
        .with_position(Point::new(412.0, 301.0));
    record.hit_path.push(button_id);

    assert_eq!(
        record.format_diagnostic(),
        "ptr@ 412,301 → hit[42:1] handled"
    );
    assert_eq!(format!("{record}"), "ptr@ 412,301 → hit[42:1] handled");
}

#[test]
fn test_diagnostic_formatting_all_event_kinds() {
    let target = WidgetId::from_parts(10, 1);

    // Pointer without position
    let rec_ptr = EventRecord::new(1, 1, EventKind::Pointer, Disposition::Ignored);
    assert_eq!(rec_ptr.format_diagnostic(), "ptr → ignored");

    // Pointer with fractional position
    let rec_ptr_pos = EventRecord::new(1, 1, EventKind::Pointer, Disposition::Ignored)
        .with_position(Point::new(12.34, 56.78));
    assert_eq!(rec_ptr_pos.format_diagnostic(), "ptr@ 12.3,56.8 → ignored");

    // Key
    let rec_key = EventRecord::new(2, 1, EventKind::Key, Disposition::Handled(target));
    assert_eq!(rec_key.format_diagnostic(), "key → handled(10:1)");

    // Scroll with position
    let rec_scroll = EventRecord::new(3, 1, EventKind::Scroll, Disposition::Ignored)
        .with_position(Point::new(100.0, 200.0));
    assert_eq!(rec_scroll.format_diagnostic(), "scroll@ 100,200 → ignored");

    // Scroll without position
    let rec_scroll_no_pos = EventRecord::new(3, 1, EventKind::Scroll, Disposition::Ignored);
    assert_eq!(rec_scroll_no_pos.format_diagnostic(), "scroll → ignored");

    // Ime
    let rec_ime = EventRecord::new(4, 1, EventKind::Ime, Disposition::Ignored);
    assert_eq!(rec_ime.format_diagnostic(), "ime → ignored");

    // Focus
    let rec_focus = EventRecord::new(5, 1, EventKind::Focus, Disposition::Ignored);
    assert_eq!(rec_focus.format_diagnostic(), "focus → ignored");

    // Dnd with position
    let rec_dnd = EventRecord::new(6, 1, EventKind::Dnd, Disposition::Ignored)
        .with_position(Point::new(50.0, 75.0));
    assert_eq!(rec_dnd.format_diagnostic(), "dnd@ 50,75 → ignored");

    // Dnd without position
    let rec_dnd_no_pos = EventRecord::new(6, 1, EventKind::Dnd, Disposition::Ignored);
    assert_eq!(rec_dnd_no_pos.format_diagnostic(), "dnd → ignored");
}

#[test]
fn test_diagnostic_formatting_focus_transitions() {
    let from_id = WidgetId::from_parts(1, 1);
    let to_id = WidgetId::from_parts(2, 1);

    // Both from and to
    let rec_both = EventRecord::new(1, 1, EventKind::Focus, Disposition::Ignored)
        .with_focus(Some(from_id), Some(to_id));
    assert_eq!(rec_both.format_diagnostic(), "focus → [1:1 → 2:1] ignored");

    // Only from
    let rec_from = EventRecord::new(2, 1, EventKind::Focus, Disposition::Ignored)
        .with_focus(Some(from_id), None);
    assert_eq!(rec_from.format_diagnostic(), "focus → [1:1 → -] ignored");

    // Only to
    let rec_to = EventRecord::new(3, 1, EventKind::Focus, Disposition::Ignored)
        .with_focus(None, Some(to_id));
    assert_eq!(rec_to.format_diagnostic(), "focus → [- → 2:1] ignored");
}

#[test]
fn test_diagnostic_formatting_hit_path_and_rejections() {
    let root = WidgetId::from_parts(1, 1);
    let middle = WidgetId::from_parts(2, 1);
    let occluder = WidgetId::from_parts(99, 1);

    let mut path = HitPath::new();
    path.push(root);
    path.push(middle);

    // OutsideBounds
    let rec_outside = EventRecord::new(1, 1, EventKind::Pointer, Disposition::Ignored)
        .with_hit_path(path)
        .with_hit_rejection(HitRejection::OutsideBounds);
    assert_eq!(
        rec_outside.format_diagnostic(),
        "ptr → hit[1:1/2:1] rejected:OutsideBounds ignored"
    );

    // OccludedBy
    let rec_occluded = EventRecord::new(2, 1, EventKind::Pointer, Disposition::Ignored)
        .with_hit_path(path)
        .with_hit_rejection(HitRejection::OccludedBy(occluder));
    assert_eq!(
        rec_occluded.format_diagnostic(),
        "ptr → hit[1:1/2:1] rejected:OccludedBy(99:1) ignored"
    );

    // HitTestDisabled
    let rec_disabled = EventRecord::new(3, 1, EventKind::Pointer, Disposition::Ignored)
        .with_hit_rejection(HitRejection::HitTestDisabled);
    assert_eq!(
        rec_disabled.format_diagnostic(),
        "ptr → rejected:HitTestDisabled ignored"
    );

    // UnderModal
    let rec_modal = EventRecord::new(4, 1, EventKind::Pointer, Disposition::Ignored)
        .with_hit_rejection(HitRejection::UnderModal);
    assert_eq!(
        rec_modal.format_diagnostic(),
        "ptr → rejected:UnderModal ignored"
    );

    // CapturedByOther
    let rec_cap_other = EventRecord::new(5, 1, EventKind::Pointer, Disposition::Ignored)
        .with_hit_rejection(HitRejection::CapturedByOther(occluder));
    assert_eq!(
        rec_cap_other.format_diagnostic(),
        "ptr → rejected:CapturedByOther(99:1) ignored"
    );
}

#[test]
fn test_diagnostic_formatting_dispositions() {
    let leaf = WidgetId::from_parts(20, 1);
    let ancestor = WidgetId::from_parts(10, 1);
    let other = WidgetId::from_parts(30, 1);

    let mut path = HitPath::new();
    path.push(ancestor);
    path.push(leaf);

    // Handled by leaf of hit path -> "handled"
    let rec_handled_leaf =
        EventRecord::new(1, 1, EventKind::Pointer, Disposition::Handled(leaf)).with_hit_path(path);
    assert_eq!(
        rec_handled_leaf.format_diagnostic(),
        "ptr → hit[10:1/20:1] handled"
    );

    // Handled by non-leaf -> "handled(10:1)"
    let rec_handled_ancestor =
        EventRecord::new(2, 1, EventKind::Pointer, Disposition::Handled(ancestor))
            .with_hit_path(path);
    assert_eq!(
        rec_handled_ancestor.format_diagnostic(),
        "ptr → hit[10:1/20:1] handled(10:1)"
    );

    // BubbledTo -> "bubbled_to(10:1)"
    let rec_bubbled = EventRecord::new(3, 1, EventKind::Key, Disposition::BubbledTo(ancestor))
        .with_hit_path(path);
    assert_eq!(
        rec_bubbled.format_diagnostic(),
        "key → hit[10:1/20:1] bubbled_to(10:1)"
    );

    // Captured -> "captured(30:1)"
    let rec_captured = EventRecord::new(4, 1, EventKind::Pointer, Disposition::Captured(other))
        .with_hit_path(path);
    assert_eq!(
        rec_captured.format_diagnostic(),
        "ptr → hit[10:1/20:1] captured(30:1)"
    );
}

#[test]
fn test_diagnostic_formatting_with_custom_name_resolver() {
    let btn_id = WidgetId::from_parts(42, 1);
    let panel_id = WidgetId::from_parts(10, 1);
    let unknown_id = WidgetId::from_parts(99, 1);

    let mut path = HitPath::new();
    path.push(panel_id);
    path.push(btn_id);

    let record = EventRecord::new(1, 1, EventKind::Pointer, Disposition::Handled(btn_id))
        .with_position(Point::new(412.0, 301.0))
        .with_hit_path(path)
        .with_focus(Some(unknown_id), Some(btn_id));

    let formatted = record.format_diagnostic_with(|id| {
        if id == btn_id {
            Some("App/SubmitButton".to_string())
        } else if id == panel_id {
            Some("App/FormPanel".to_string())
        } else {
            None // Falls back to slot:gen
        }
    });

    assert_eq!(
        formatted,
        "ptr@ 412,301 → [99:1 → App/SubmitButton] hit[App/FormPanel/App/SubmitButton] handled"
    );
}

#[test]
fn test_debug_events_toggle_and_log() {
    // Save original state
    let original = is_debug_events_enabled();

    set_debug_events_enabled(true);
    assert!(is_debug_events_enabled());

    let record = EventRecord::new(1, 1, EventKind::Key, Disposition::Ignored);
    // Should execute without panic (emits to stderr)
    record.log_if_debug_enabled();

    set_debug_events_enabled(false);
    assert!(!is_debug_events_enabled());
    record.log_if_debug_enabled();

    // Restore original state
    set_debug_events_enabled(original);
}
