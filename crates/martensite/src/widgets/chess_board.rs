//! `ChessBoard` — an interactive 8×8 chess board (the game/chess-UI
//! primitive).
//!
//! Squares are indexed `file + rank * 8` with `a1 = 0` (`rank 0` is
//! White's back rank, drawn at the bottom unless [`ChessBoard::flipped`]
//! is set). Pieces are painted as Unicode glyphs through the text
//! painter (letter glyphs are the fallback).
//!
//! Click a square holding a piece to select it, then click a
//! destination to move — the board applies the move (no legality
//! checking; the host owns the rules) and parks `(from, to)` in
//! [`ChessBoard::take_moved`]. Arrow keys move a focus square and
//! `Enter`/`Space` select or commit; `Escape` deselects. Use
//! [`ChessBoard::read_only`] for a display-only board, [`ChessBoard::undo`]
//! to pop the last move, and [`ChessBoard::fen`] to load a position.
//!
//! # Examples
//!
//! ```
//! use martensite::widgets::chess_board::{ChessBoard, Piece, Side};
//!
//! let mut b = ChessBoard::new();
//! assert_eq!(b.piece_at(ChessBoard::alg("e2").unwrap()), Some((Piece::Pawn, Side::White)));
//! b.move_piece(ChessBoard::alg("e2").unwrap(), ChessBoard::alg("e4").unwrap());
//! assert_eq!(b.last_move(), Some((12, 28)));
//! ```

use accesskit::Node as AccessKitNode;
use glam::Vec2;
use martensite_core::{
    EventContext, EventResponse, LayoutConstraints, LayoutContext, PaintContext, PointerButton,
    Rect, RenderMinimum, UnderflowPolicy, Widget, WidgetEvent,
};
use martensite_theme::TokenKey;

const SQ_PT: f32 = 44.0;
const COORD_PT: f32 = 16.0;

const LIGHT: [u8; 4] = [240, 217, 181, 255];
const DARK: [u8; 4] = [181, 136, 99, 255];
const EDGE: [u8; 4] = [90, 70, 50, 255];
const SELECT: [u8; 4] = [96, 165, 250, 110];
const LAST: [u8; 4] = [250, 204, 21, 70];
const FOCUS: [u8; 4] = [96, 165, 250, 255];
const WHITE_GLYPH: [u8; 4] = [250, 250, 250, 255];
const BLACK_GLYPH: [u8; 4] = [30, 30, 32, 255];
const COORD: [u8; 4] = [120, 100, 80, 255];

/// The kind of chess piece — see [`ChessBoard`].
///
/// ```
/// use martensite::widgets::chess_board::Piece;
///
/// assert_eq!(Piece::Knight.glyph(), '♘');
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piece {
    /// Pawn.
    Pawn,
    /// Knight.
    Knight,
    /// Bishop.
    Bishop,
    /// Rook.
    Rook,
    /// Queen.
    Queen,
    /// King.
    King,
}

impl Piece {
    /// White-side Unicode glyph.
    ///
    /// ```
    /// use martensite::widgets::chess_board::Piece;
    ///
    /// assert_eq!(Piece::Queen.glyph(), '♕');
    /// ```
    pub fn glyph(self) -> char {
        match self {
            Self::King => '♔',
            Self::Queen => '♕',
            Self::Rook => '♖',
            Self::Bishop => '♗',
            Self::Knight => '♘',
            Self::Pawn => '♙',
        }
    }

    /// ASCII letter fallback (`K`, `Q`, `R`, `B`, `N`, `P`).
    ///
    /// ```
    /// use martensite::widgets::chess_board::Piece;
    ///
    /// assert_eq!(Piece::Knight.letter(), 'N');
    /// ```
    pub fn letter(self) -> char {
        match self {
            Self::King => 'K',
            Self::Queen => 'Q',
            Self::Rook => 'R',
            Self::Bishop => 'B',
            Self::Knight => 'N',
            Self::Pawn => 'P',
        }
    }

    /// Parses a FEN piece letter (`p`/`n`/`b`/`r`/`q`/`k`, either case).
    fn from_fen(ch: char) -> Option<Self> {
        Some(match ch.to_ascii_lowercase() {
            'p' => Self::Pawn,
            'n' => Self::Knight,
            'b' => Self::Bishop,
            'r' => Self::Rook,
            'q' => Self::Queen,
            'k' => Self::King,
            _ => return None,
        })
    }
}

/// Which side a piece belongs to — see [`ChessBoard`].
///
/// ```
/// use martensite::widgets::chess_board::Side;
///
/// assert_ne!(Side::White, Side::Black);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// White pieces (drawn at the bottom by default).
    White,
    /// Black pieces.
    Black,
}

/// A square's occupant.
type Cell = Option<(Piece, Side)>;

/// One undo record: `(from, to, captured)`.
type MoveRecord = (usize, usize, Cell);

/// An 8×8 chess board — see the module docs.
///
/// ```
/// use martensite::widgets::chess_board::ChessBoard;
///
/// assert_eq!(ChessBoard::new().piece_count(), 32);
/// ```
pub struct ChessBoard {
    /// Accessibility label.
    pub label: String,
    /// `rank * 8 + file`; `None` = empty square.
    cells: [Cell; 64],
    selected: Option<usize>,
    focus: usize,
    moved: Option<(usize, usize)>,
    history: Vec<MoveRecord>,
    flip: bool,
    readonly: bool,
    coords: bool,
    bounds: Rect,
    scale: f32,
    text_painter: Option<crate::text_paint::SharedTextPainter>,
}

impl std::fmt::Debug for ChessBoard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChessBoard")
            .field("pieces", &self.piece_count())
            .field("selected", &self.selected)
            .finish()
    }
}

impl Default for ChessBoard {
    fn default() -> Self {
        Self::new()
    }
}

impl ChessBoard {
    /// Board in the standard starting position.
    ///
    /// ```
    /// use martensite::widgets::chess_board::ChessBoard;
    ///
    /// assert_eq!(ChessBoard::new().piece_count(), 32);
    /// ```
    pub fn new() -> Self {
        let mut b = Self {
            label: "Chess board".to_string(),
            cells: [None; 64],
            selected: None,
            focus: 0,
            moved: None,
            history: Vec::new(),
            flip: false,
            readonly: false,
            coords: false,
            bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale: 1.0,
            text_painter: None,
        };
        b.reset();
        b
    }

    /// An empty board.
    ///
    /// ```
    /// use martensite::widgets::chess_board::ChessBoard;
    ///
    /// assert_eq!(ChessBoard::blank().piece_count(), 0);
    /// ```
    pub fn blank() -> Self {
        let mut b = Self::new();
        b.clear();
        b
    }

    /// Accessibility label.
    ///
    /// ```
    /// use martensite::widgets::chess_board::ChessBoard;
    ///
    /// assert_eq!(ChessBoard::new().label("Puzzle 42").label, "Puzzle 42");
    /// ```
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// Custom text painter.
    ///
    /// ```
    /// use martensite::widgets::chess_board::ChessBoard;
    ///
    /// let _ = ChessBoard::new(); // painter optional
    /// ```
    pub fn with_text_painter(mut self, p: crate::text_paint::SharedTextPainter) -> Self {
        self.text_painter = Some(p);
        self
    }

    /// Draws black at the bottom when `flip` is set.
    ///
    /// ```
    /// use martensite::widgets::chess_board::ChessBoard;
    ///
    /// assert!(ChessBoard::new().flipped(true).is_flipped());
    /// ```
    pub fn flipped(mut self, flip: bool) -> Self {
        self.flip = flip;
        self
    }

    /// Whether the board is drawn flipped.
    ///
    /// ```
    /// use martensite::widgets::chess_board::ChessBoard;
    ///
    /// assert!(!ChessBoard::new().is_flipped());
    /// ```
    pub fn is_flipped(&self) -> bool {
        self.flip
    }

    /// Display-only mode — clicks still select but never move.
    ///
    /// ```
    /// use martensite::widgets::chess_board::ChessBoard;
    ///
    /// assert!(ChessBoard::new().read_only(true).is_read_only());
    /// ```
    pub fn read_only(mut self, ro: bool) -> Self {
        self.readonly = ro;
        self
    }

    /// Whether moves are disabled.
    ///
    /// ```
    /// use martensite::widgets::chess_board::ChessBoard;
    ///
    /// assert!(!ChessBoard::new().is_read_only());
    /// ```
    pub fn is_read_only(&self) -> bool {
        self.readonly
    }

    /// Draws `a`–`h` / `1`–`8` labels in a margin when `on`.
    ///
    /// ```
    /// use martensite::widgets::chess_board::ChessBoard;
    ///
    /// assert!(ChessBoard::new().coordinates(true).has_coordinates());
    /// ```
    pub fn coordinates(mut self, on: bool) -> Self {
        self.coords = on;
        self
    }

    /// Whether the coordinate margin is drawn.
    ///
    /// ```
    /// use martensite::widgets::chess_board::ChessBoard;
    ///
    /// assert!(!ChessBoard::new().has_coordinates());
    /// ```
    pub fn has_coordinates(&self) -> bool {
        self.coords
    }

    /// Loads a FEN piece-placement field (the part before the first
    /// space), e.g. `"rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR"`.
    /// Invalid input clears the board.
    ///
    /// ```
    /// use martensite::widgets::chess_board::{ChessBoard, Piece, Side};
    ///
    /// let b = ChessBoard::blank().fen("8/8/8/8/8/8/4K3/4k3");
    /// assert_eq!(b.piece_count(), 2);
    /// ```
    pub fn fen(mut self, fen: &str) -> Self {
        self.clear();
        let placement = fen.split_whitespace().next().unwrap_or("");
        for (row_i, row) in placement.split('/').enumerate() {
            let rank = 7usize.wrapping_sub(row_i);
            if rank >= 8 {
                break;
            }
            let mut file = 0usize;
            for ch in row.chars() {
                if let Some(skip) = ch.to_digit(10) {
                    file += skip as usize;
                    continue;
                }
                if file >= 8 {
                    break;
                }
                if let Some(p) = Piece::from_fen(ch) {
                    let side = if ch.is_uppercase() {
                        Side::White
                    } else {
                        Side::Black
                    };
                    self.cells[rank * 8 + file] = Some((p, side));
                }
                file += 1;
            }
        }
        self
    }

    /// Square index from algebraic notation (`"a1"`–`"h8"`).
    ///
    /// ```
    /// use martensite::widgets::chess_board::ChessBoard;
    ///
    /// assert_eq!(ChessBoard::alg("e2"), Some(12));
    /// assert_eq!(ChessBoard::alg("z9"), None);
    /// ```
    pub fn alg(s: &str) -> Option<usize> {
        let b = s.as_bytes();
        if b.len() != 2 {
            return None;
        }
        let file = b[0].wrapping_sub(b'a');
        let rank = b[1].wrapping_sub(b'1');
        if file < 8 && rank < 8 {
            Some(rank as usize * 8 + file as usize)
        } else {
            None
        }
    }

    /// Algebraic name of square `sq` (`0` → `"a1"`).
    ///
    /// ```
    /// use martensite::widgets::chess_board::ChessBoard;
    ///
    /// assert_eq!(ChessBoard::name_of(28), "e4");
    /// ```
    pub fn name_of(sq: usize) -> String {
        let sq = sq.min(63);
        format!("{}{}", (b'a' + (sq % 8) as u8) as char, sq / 8 + 1)
    }

    /// Piece on square `sq`, if any.
    ///
    /// ```
    /// use martensite::widgets::chess_board::{ChessBoard, Piece, Side};
    ///
    /// assert_eq!(ChessBoard::new().piece_at(0), Some((Piece::Rook, Side::White)));
    /// ```
    pub fn piece_at(&self, sq: usize) -> Option<(Piece, Side)> {
        self.cells.get(sq).copied().flatten()
    }

    /// Occupied-square count.
    ///
    /// ```
    /// use martensite::widgets::chess_board::ChessBoard;
    ///
    /// assert_eq!(ChessBoard::new().piece_count(), 32);
    /// ```
    pub fn piece_count(&self) -> usize {
        self.cells.iter().flatten().count()
    }

    /// Places `piece` on square `sq` (`None` clears the square).
    ///
    /// ```
    /// use martensite::widgets::chess_board::{ChessBoard, Piece, Side};
    ///
    /// let mut b = ChessBoard::blank();
    /// b.set(ChessBoard::alg("d4").unwrap(), Piece::Queen, Side::White);
    /// assert_eq!(b.piece_count(), 1);
    /// ```
    pub fn set(&mut self, sq: usize, piece: Piece, side: Side) {
        if sq < 64 {
            self.cells[sq] = Some((piece, side));
        }
    }

    /// Clears every square and the move history.
    ///
    /// ```
    /// use martensite::widgets::chess_board::ChessBoard;
    ///
    /// let mut b = ChessBoard::new();
    /// b.clear();
    /// assert_eq!(b.piece_count(), 0);
    /// ```
    pub fn clear(&mut self) {
        self.cells = [None; 64];
        self.selected = None;
        self.moved = None;
        self.history.clear();
    }

    /// Restores the standard starting position.
    ///
    /// ```
    /// use martensite::widgets::chess_board::ChessBoard;
    ///
    /// let mut b = ChessBoard::blank();
    /// b.reset();
    /// assert_eq!(b.piece_count(), 32);
    /// ```
    pub fn reset(&mut self) {
        self.clear();
        const BACK: [Piece; 8] = [
            Piece::Rook,
            Piece::Knight,
            Piece::Bishop,
            Piece::Queen,
            Piece::King,
            Piece::Bishop,
            Piece::Knight,
            Piece::Rook,
        ];
        for (f, p) in BACK.iter().enumerate() {
            self.cells[f] = Some((*p, Side::White));
            self.cells[8 + f] = Some((Piece::Pawn, Side::White));
            self.cells[48 + f] = Some((Piece::Pawn, Side::Black));
            self.cells[56 + f] = Some((*p, Side::Black));
        }
    }

    /// Applies a `from → to` move (capture allowed, no legality check)
    /// and records it for [`ChessBoard::undo`] and
    /// [`ChessBoard::last_move`].
    ///
    /// ```
    /// use martensite::widgets::chess_board::{ChessBoard, Piece, Side};
    ///
    /// let mut b = ChessBoard::new();
    /// b.move_piece(12, 28); // e2→e4
    /// assert_eq!(b.piece_at(28), Some((Piece::Pawn, Side::White)));
    /// assert_eq!(b.last_move(), Some((12, 28)));
    /// ```
    pub fn move_piece(&mut self, from: usize, to: usize) {
        if from >= 64 || to >= 64 || from == to {
            return;
        }
        if self.cells[from].is_none() {
            return;
        }
        let captured = self.cells[to];
        self.cells[to] = self.cells[from].take();
        self.history.push((from, to, captured));
        self.moved = Some((from, to));
        self.selected = None;
    }

    /// Reverts the last applied move (including any capture).
    ///
    /// ```
    /// use martensite::widgets::chess_board::ChessBoard;
    ///
    /// let mut b = ChessBoard::new();
    /// b.move_piece(12, 28);
    /// assert!(b.undo());
    /// assert_eq!(b.piece_count(), 32);
    /// ```
    pub fn undo(&mut self) -> bool {
        let Some((from, to, captured)) = self.history.pop() else {
            return false;
        };
        self.cells[from] = self.cells[to].take();
        self.cells[to] = captured;
        self.moved = self.history.last().map(|&(f, t, _)| (f, t));
        true
    }

    /// The most recently applied move.
    ///
    /// ```
    /// use martensite::widgets::chess_board::ChessBoard;
    ///
    /// assert_eq!(ChessBoard::new().last_move(), None);
    /// ```
    pub fn last_move(&self) -> Option<(usize, usize)> {
        self.moved
    }

    /// Currently selected square.
    ///
    /// ```
    /// use martensite::widgets::chess_board::ChessBoard;
    ///
    /// assert_eq!(ChessBoard::new().selected(), None);
    /// ```
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Selects square `sq` (clamped to the board).
    ///
    /// ```
    /// use martensite::widgets::chess_board::ChessBoard;
    ///
    /// let mut b = ChessBoard::new();
    /// b.select(12);
    /// assert_eq!(b.selected(), Some(12));
    /// ```
    pub fn select(&mut self, sq: usize) {
        if sq < 64 {
            self.selected = Some(sq);
            self.focus = sq;
        }
    }

    /// Drains the last move applied through a click or keyboard commit.
    ///
    /// ```
    /// use martensite::widgets::chess_board::ChessBoard;
    ///
    /// assert_eq!(ChessBoard::new().take_moved(), None);
    /// ```
    pub fn take_moved(&mut self) -> Option<(usize, usize)> {
        self.moved.take()
    }

    /// Board area inside the optional coordinate margin.
    fn board_rect(&self) -> Rect {
        let m = if self.coords {
            COORD_PT * self.scale
        } else {
            0.0
        };
        Rect::new(
            self.bounds.min_x() + m,
            self.bounds.min_y(),
            (self.bounds.width() - m).max(0.0),
            (self.bounds.height() - m).max(0.0),
        )
    }

    /// Device-space rect of square `sq`.
    fn sq_rect(&self, sq: usize) -> Rect {
        let br = self.board_rect();
        let s = br.width().min(br.height()) / 8.0;
        let file = sq % 8;
        let rank = sq / 8;
        let (df, dr) = if self.flip {
            (7 - file, rank)
        } else {
            (file, 7 - rank)
        };
        Rect::new(br.min_x() + df as f32 * s, br.min_y() + dr as f32 * s, s, s)
    }

    /// Square under device point `p`, if any.
    fn square_at(&self, p: Vec2) -> Option<usize> {
        let br = self.board_rect();
        let s = br.width().min(br.height()) / 8.0;
        if s <= 0.0 {
            return None;
        }
        let df = ((p.x - br.min_x()) / s) as i32;
        let dr = ((p.y - br.min_y()) / s) as i32;
        if !(0..8).contains(&df) || !(0..8).contains(&dr) {
            return None;
        }
        let (file, rank) = if self.flip {
            (7 - df, dr)
        } else {
            (df, 7 - dr)
        };
        Some(rank as usize * 8 + file as usize)
    }

    /// Click semantics shared by pointer and Enter.
    fn tap(&mut self, sq: usize) {
        match self.selected {
            Some(from) if from != sq && !self.readonly => {
                if self.cells[sq].is_some()
                    && self.cells[from].is_some()
                    && self.cells[sq].map(|c| c.1) == self.cells[from].map(|c| c.1)
                {
                    // Friendly piece — reselect instead of capturing.
                    self.selected = Some(sq);
                } else {
                    self.move_piece(from, sq);
                }
            }
            Some(s) if s == sq => self.selected = None,
            _ => {
                if self.cells[sq].is_some() {
                    self.selected = Some(sq);
                }
            }
        }
        self.focus = sq;
    }
}

impl Widget for ChessBoard {
    fn measure(&mut self, cx: &mut LayoutContext, constraints: LayoutConstraints) -> Vec2 {
        let m = if self.coords { COORD_PT } else { 0.0 };
        let side = cx.pt(SQ_PT * 8.0 + m);
        Vec2::new(
            side.min(constraints.max_size.x.max(0.0)),
            side.min(constraints.max_size.y.max(0.0)),
        )
    }

    fn min_render(&self) -> RenderMinimum {
        RenderMinimum::new(Vec2::new(120.0, 120.0)).with_policy(UnderflowPolicy::Lint)
    }

    fn layout(&mut self, cx: &mut LayoutContext, bounds: Rect) {
        self.bounds = bounds;
        self.scale = cx.scale;
    }

    fn accessibility(&self, node: &mut AccessKitNode) {
        node.set_role(accesskit::Role::Grid);
        node.set_label(format!("{} — {} pieces", self.label, self.piece_count()));
        if self.readonly {
            node.set_disabled();
        }
    }

    fn event(&mut self, cx: &mut EventContext) -> EventResponse {
        match cx.event {
            WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position,
                ..
            } => {
                let Some(sq) = self.square_at(*position) else {
                    return EventResponse::Ignored;
                };
                self.tap(sq);
                EventResponse::RequestRepaint
            }
            WidgetEvent::KeyPressed { key, .. } => match key.as_str() {
                "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" => {
                    let file = self.focus % 8;
                    let rank = self.focus / 8;
                    let (df, dr) = if self.flip {
                        (7 - file, rank)
                    } else {
                        (file, 7 - rank)
                    };
                    let (nd, nr) = match key.as_str() {
                        "ArrowLeft" => (df.saturating_sub(1), dr),
                        "ArrowRight" => ((df + 1).min(7), dr),
                        "ArrowUp" => (df, dr.saturating_sub(1)),
                        _ => (df, nr_up(dr)),
                    };
                    let (nf, nrank) = if self.flip {
                        (7 - nd, nr)
                    } else {
                        (nd, 7 - nr)
                    };
                    self.focus = nrank * 8 + nf;
                    EventResponse::RequestRepaint
                }
                "Enter" | " " => {
                    self.tap(self.focus);
                    EventResponse::RequestRepaint
                }
                "Escape" => {
                    if self.selected.take().is_some() {
                        EventResponse::RequestRepaint
                    } else {
                        EventResponse::Ignored
                    }
                }
                _ => EventResponse::Ignored,
            },
            _ => EventResponse::Ignored,
        }
    }

    fn paint(&self, cx: &mut PaintContext) {
        let krect = |r: Rect| {
            kurbo::Rect::new(
                f64::from(r.min_x()),
                f64::from(r.min_y()),
                f64::from(r.max_x()),
                f64::from(r.max_y()),
            )
        };
        let painter = crate::text_paint::resolve_painter(&self.text_painter, cx.text_painter);
        let br = self.board_rect();
        let s = br.width().min(br.height()) / 8.0;

        for sq in 0..64 {
            let r = self.sq_rect(sq);
            let light = (sq % 8 + sq / 8) % 2 == 0;
            cx.list
                .push_fill_rect(krect(r), if light { LIGHT } else { DARK });
            if self.last_move().map(|(f, t)| f == sq || t == sq) == Some(true) {
                cx.list.push_fill_rect(krect(r), LAST);
            }
            if self.selected == Some(sq) {
                cx.list
                    .push_fill_rect(krect(r), cx.color(TokenKey::AccentColor, SELECT));
            }
            if let Some((piece, side)) = self.cells[sq] {
                let color = match side {
                    Side::White => WHITE_GLYPH,
                    Side::Black => BLACK_GLYPH,
                };
                let size = s * 0.72;
                let text = if painter.is_some() {
                    piece.glyph().to_string()
                } else {
                    piece.letter().to_string()
                };
                crate::text_paint::paint_label_clipped(
                    painter,
                    cx.list,
                    krect(r),
                    kurbo::Point::new(
                        f64::from(r.min_x() + s * 0.5 - size * 0.3),
                        f64::from(r.min_y() + s * 0.5 - size * 0.62),
                    ),
                    &text,
                    size,
                    color,
                );
            }
            if self.focus == sq {
                cx.list.push_stroke_shape(
                    krect(r),
                    &martensite_core::shape::Shape::RECT,
                    1.0 * self.scale,
                    FOCUS,
                );
            }
        }
        cx.list.push_stroke_shape(
            krect(br),
            &martensite_core::shape::Shape::RECT,
            self.scale,
            cx.color(TokenKey::BorderColor, EDGE),
        );

        // Coordinate margin.
        if self.coords {
            let m = COORD_PT * self.scale;
            let size = 9.0 * self.scale;
            for i in 0..8 {
                // File letters along the bottom edge.
                let file = if self.flip { 7 - i } else { i };
                let ch = ((b'a' + file as u8) as char).to_string();
                let x = br.min_x() + i as f32 * s + s * 0.5 - size * 0.3;
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(f64::from(x), f64::from(br.max_y() + m * 0.2)),
                    &ch,
                    size,
                    cx.color(TokenKey::TextMutedColor, COORD),
                );
                // Rank numbers in the left margin.
                let rank = if self.flip { i } else { 7 - i };
                let ch = (rank + 1).to_string();
                let y = br.min_y() + i as f32 * s + s * 0.5 - size * 0.62;
                crate::text_paint::paint_label(
                    painter,
                    cx.list,
                    kurbo::Point::new(f64::from(self.bounds.min_x() + m * 0.25), f64::from(y)),
                    &ch,
                    size,
                    cx.color(TokenKey::TextMutedColor, COORD),
                );
            }
        }
    }
}

/// Saturating up-step for the display row.
fn nr_up(dr: usize) -> usize {
    (dr + 1).min(7)
}

#[cfg(test)]
mod tests {
    use super::*;
    use martensite_core::{HotNode, PaintList};

    fn laid_out(b: &mut ChessBoard, w: f32, h: f32) {
        let mut hot = HotNode::default();
        let mut cx = LayoutContext {
            hot: &mut hot,
            scale: 1.0,
        };
        b.measure(
            &mut cx,
            LayoutConstraints {
                min_size: Vec2::ZERO,
                max_size: Vec2::new(w, h),
            },
        );
        b.layout(&mut cx, Rect::new(0.0, 0.0, w, h));
    }

    fn ev(b: &mut ChessBoard, e: &WidgetEvent) {
        b.event(&mut EventContext {
            event: e,
            bounds: b.bounds,
            scale: 1.0,
        });
    }

    #[test]
    fn starting_position() {
        let b = ChessBoard::new();
        assert_eq!(b.piece_count(), 32);
        assert_eq!(
            b.piece_at(ChessBoard::alg("e1").unwrap()),
            Some((Piece::King, Side::White))
        );
        assert_eq!(
            b.piece_at(ChessBoard::alg("d8").unwrap()),
            Some((Piece::Queen, Side::Black))
        );
        assert_eq!(b.piece_at(ChessBoard::alg("e4").unwrap()), None);
    }

    #[test]
    fn fen_loads() {
        let b = ChessBoard::blank().fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR");
        assert_eq!(b.piece_count(), 32);
        assert_eq!(b.piece_at(0), Some((Piece::Rook, Side::White)));
        assert_eq!(b.piece_at(63), Some((Piece::Rook, Side::Black)));
    }

    #[test]
    fn click_moves() {
        let mut b = ChessBoard::new();
        laid_out(&mut b, 352.0, 352.0);
        let press = |b: &mut ChessBoard, sq: usize| {
            let r = b.sq_rect(sq);
            ev(
                b,
                &WidgetEvent::PointerPressed {
                    button: PointerButton::Primary,
                    position: Vec2::new(
                        (r.min_x() + r.max_x()) / 2.0,
                        (r.min_y() + r.max_y()) / 2.0,
                    ),
                    count: 1,
                },
            );
        };
        let e2 = ChessBoard::alg("e2").unwrap();
        let e4 = ChessBoard::alg("e4").unwrap();
        press(&mut b, e2);
        assert_eq!(b.selected(), Some(e2));
        press(&mut b, e4);
        assert_eq!(b.piece_at(e4), Some((Piece::Pawn, Side::White)));
        assert_eq!(b.take_moved(), Some((e2, e4)));
        assert_eq!(b.take_moved(), None);
    }

    #[test]
    fn undo_restores_capture() {
        let mut b = ChessBoard::blank();
        let d4 = ChessBoard::alg("d4").unwrap();
        let e5 = ChessBoard::alg("e5").unwrap();
        b.set(d4, Piece::Queen, Side::White);
        b.set(e5, Piece::Pawn, Side::Black);
        b.move_piece(d4, e5);
        assert_eq!(b.piece_count(), 1); // the pawn was captured
        assert!(b.undo());
        assert_eq!(b.piece_at(d4), Some((Piece::Queen, Side::White)));
        assert_eq!(b.piece_at(e5), Some((Piece::Pawn, Side::Black)));
        assert!(!b.undo());
    }

    #[test]
    fn read_only_blocks_moves() {
        let mut b = ChessBoard::new().read_only(true);
        laid_out(&mut b, 352.0, 352.0);
        let r = b.sq_rect(ChessBoard::alg("e2").unwrap());
        ev(
            &mut b,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new((r.min_x() + r.max_x()) / 2.0, (r.min_y() + r.max_y()) / 2.0),
                count: 1,
            },
        );
        assert_eq!(b.selected(), Some(12));
        let r4 = b.sq_rect(ChessBoard::alg("e4").unwrap());
        ev(
            &mut b,
            &WidgetEvent::PointerPressed {
                button: PointerButton::Primary,
                position: Vec2::new(
                    (r4.min_x() + r4.max_x()) / 2.0,
                    (r4.min_y() + r4.max_y()) / 2.0,
                ),
                count: 1,
            },
        );
        assert_eq!(b.piece_at(ChessBoard::alg("e4").unwrap()), None);
        assert_eq!(b.take_moved(), None);
    }

    #[test]
    fn arrows_move_focus() {
        let mut b = ChessBoard::new();
        laid_out(&mut b, 352.0, 352.0);
        b.focus = 12; // e2
        ev(
            &mut b,
            &WidgetEvent::KeyPressed {
                key: "ArrowUp".to_string(),
                repeat: false,
            },
        );
        assert_eq!(b.focus, 20); // e3
        ev(
            &mut b,
            &WidgetEvent::KeyPressed {
                key: "ArrowDown".to_string(),
                repeat: false,
            },
        );
        assert_eq!(b.focus, 12); // back to e2
        ev(
            &mut b,
            &WidgetEvent::KeyPressed {
                key: "Enter".to_string(),
                repeat: false,
            },
        );
        assert_eq!(b.selected(), Some(12)); // e2 holds a pawn
    }

    #[test]
    fn paints_without_painter() {
        let mut b = ChessBoard::new();
        laid_out(&mut b, 352.0, 352.0);
        let theme = martensite_theme::Theme::new("test");
        let mut list = PaintList::new();
        let mut cx = PaintContext {
            list: &mut list,
            bounds: b.bounds,
            scale: 1.0,
            theme: &theme,
            text_painter: None,
        };
        b.paint(&mut cx);
    }
}
