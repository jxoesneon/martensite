use super::*;

#[derive(Clone)]
enum PanelRow {
    /// Sort mode id: 0 = pack order, 1 = a→z, 2 = z→a.
    Sort(u8),
    /// `None` clears the category; `Some` selects it.
    Category(Option<String>),
}

/// The filter popover content — opened as an **overlay entry** so it
/// paints above everything (hero glyph included), gets outside-press
/// and `Escape` dismissal from the layer, and owns its own rows.
pub(super) struct FilterPanel {
    /// Category choices captured at open (the popover is short-lived;
    /// a pack change closes and rebuilds it).
    cats: Vec<String>,
    signals: ViewerSignals,
    bounds: Rect,
    scale: f32,
    rows: Vec<(Rect, PanelRow)>,
    hover_row: Option<usize>,
}

impl FilterPanel {
    const ROW: f32 = 20.0;
    const HEAD: f32 = 16.0;
    const W: f32 = 190.0;

    pub(super) fn new(cats: Vec<String>, signals: ViewerSignals) -> Self {
        Self {
            cats,
            signals,
            bounds: Rect::default(),
            scale: 1.0,
            rows: Vec::new(),
            hover_row: None,
        }
    }

    /// Logical-size content box: sort header + 3 options, category
    /// header + "all" + each category, padded.
    fn content_pt(&self) -> (f32, f32) {
        let rows = 3 + 1 + self.cats.len();
        (Self::W, 2.0 * Self::HEAD + rows as f32 * Self::ROW + 12.0)
    }

    fn hit_row(&self, p: Vec2) -> Option<usize> {
        self.rows.iter().position(|(r, _)| r.contains(p))
    }
}

impl Widget for FilterPanel {
    fn measure(&mut self, cx: &mut LayoutContext, _c: LayoutConstraints) -> Vec2 {
        let (w, h) = self.content_pt();
        Vec2::new(w * cx.scale, h * cx.scale)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale.max(0.01);
        let s = self.scale;
        let pt = |v: f32| v * s;
        let row_h = pt(Self::ROW);
        let head_h = pt(Self::HEAD);
        self.rows.clear();
        let mut cy = bounds.origin.y + pt(6.0) + head_h;
        for mode in 0..3u8 {
            self.rows.push((
                Rect::new(bounds.origin.x, cy, bounds.size.x, row_h),
                PanelRow::Sort(mode),
            ));
            cy += row_h;
        }
        cy += head_h;
        self.rows.push((
            Rect::new(bounds.origin.x, cy, bounds.size.x, row_h),
            PanelRow::Category(None),
        ));
        cy += row_h;
        for c in self.cats.clone() {
            self.rows.push((
                Rect::new(bounds.origin.x, cy, bounds.size.x, row_h),
                PanelRow::Category(Some(c)),
            ));
            cy += row_h;
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerMoved { position } => {
                let row = self.hit_row(*position);
                if row != self.hover_row {
                    self.hover_row = row;
                    EventResponse::RequestRepaint
                } else {
                    EventResponse::Handled
                }
            }
            WidgetEvent::PointerPressed { position, .. } => {
                if let Some(row_i) = self.hit_row(*position) {
                    match &self.rows[row_i].1 {
                        PanelRow::Sort(mode) => {
                            let s = ["", "az", "za"][usize::from(*mode)];
                            self.signals.sort.set(s.to_string());
                        }
                        PanelRow::Category(cat) => {
                            self.signals.category.set(cat.clone().unwrap_or_default());
                        }
                    }
                }
                // Any press inside the popup is consumed — the layer
                // only dismisses on presses outside the bounds.
                EventResponse::Handled
            }
            _ => EventResponse::Ignored,
        }
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::ListBox);
        node.set_label("filter options — sort and category");
    }

    fn paint(&self, cx: &mut PaintContext) {
        let r = self.bounds;
        if r.width() <= 0.0 {
            return;
        }
        let s = self.scale;
        let pt = |v: f32| v * s;
        let k = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        fn text(cx: &mut PaintContext, x: f32, y: f32, t: &str, size_px: f32, color: [u8; 4]) {
            let origin = kurbo::Point::new(f64::from(x), f64::from(y));
            if let Some(p) = cx.text_painter {
                p.paint_shaped_text(cx.list, origin, t, size_px, color);
            } else {
                cx.list.push_text(origin, t.to_string(), size_px, color);
            }
        }
        let ink = cx.color(TokenKey::TextColor, [226, 230, 240, 255]);
        let dim = cx.color(TokenKey::TextMutedColor, [122, 130, 150, 255]);
        let accent = cx.color(TokenKey::AccentColor, [96, 165, 250, 255]);
        let raised = cx.color(TokenKey::RaisedColor, [22, 26, 35, 255]);
        let border = cx.color(TokenKey::BorderColor, [43, 48, 63, 255]);

        cx.list
            .push_fill_shape(k(r), &Shape::rounded(pt(10.0)), raised);
        cx.list
            .push_stroke_shape(k(r), &Shape::rounded(pt(10.0)), pt(1.0), border);
        let head_h = pt(Self::HEAD);
        text(
            cx,
            r.origin.x + pt(10.0),
            r.origin.y + pt(6.0),
            "sort",
            10.0 * s,
            dim,
        );
        let sort_labels = ["pack order", "name a → z", "name z → a"];
        let cur_sort = match self.signals.sort.get().as_str() {
            "az" => 1,
            "za" => 2,
            _ => 0,
        };
        let cur_cat = self.signals.category.get();
        for (i, (rr, row)) in self.rows.iter().enumerate() {
            if self.hover_row == Some(i) {
                cx.list.push_fill_shape(
                    k(*rr),
                    &Shape::rounded(pt(6.0)),
                    [accent[0], accent[1], accent[2], 22],
                );
            }
            let (label, selected) = match row {
                PanelRow::Sort(mode) => (
                    sort_labels[usize::from(*mode)].to_string(),
                    usize::from(*mode) == cur_sort,
                ),
                PanelRow::Category(cat) => match cat {
                    None => ("all categories".to_string(), cur_cat.is_empty()),
                    Some(c) => (c.clone(), cur_cat == *c),
                },
            };
            // Section header sits in the gap above the first
            // category row.
            if i == 3 {
                text(
                    cx,
                    r.origin.x + pt(10.0),
                    rr.origin.y - head_h + pt(3.0),
                    "category",
                    10.0 * s,
                    dim,
                );
            }
            let mark = if selected { "● " } else { "  " };
            text(
                cx,
                r.origin.x + pt(10.0),
                rr.origin.y + pt(4.5),
                &format!("{mark}{label}"),
                11.0 * s,
                if selected { accent } else { ink },
            );
        }
    }

    fn debug_name(&self) -> &'static str {
        "FilterPanel"
    }
}
