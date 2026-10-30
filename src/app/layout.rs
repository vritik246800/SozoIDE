use ratatui::layout::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Explorer,
    Editor,
    BottomTerminal,
    RightTerminal,
    Database,
    Git,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlay {
    None,
    Help,
    About,
    Update,
    Tutorial,
    WhatsNew,
}

/// Which notes the notes dialog shows (F18): all of them, only what is new or only what was
/// fixed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NotesFilter {
    #[default]
    All,
    New,
    Fixed,
}

impl NotesFilter {
    pub const ALL: [Self; 3] = [Self::All, Self::New, Self::Fixed];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::New => "New",
            Self::Fixed => "Fixed",
        }
    }

    /// The next filter, or the previous one, round the ends (Tab and BackTab).
    pub fn step(self, forward: bool) -> Self {
        let index = Self::ALL.iter().position(|filter| *filter == self);
        let index = index.unwrap_or_default() + if forward { 1 } else { 2 };
        Self::ALL[index % Self::ALL.len()]
    }
}

/// Two editor panes: side by side (`Columns`) or stacked (`Rows`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitDirection {
    Columns,
    Rows,
}

/// The editor split in two (F19): each pane has its own tabs (`OpenFile::pane`, 0 is the
/// left/top one). The active tab is shown in its pane and `other` in the other pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Split {
    pub direction: SplitDirection,
    pub other: usize,
}

/// Where a dragged tab is dropped in the editor: an edge splits it, the centre keeps one
/// pane and, once split, a pane takes the tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropZone {
    Left,
    Right,
    Top,
    Bottom,
    Center,
    Pane(usize),
}

/// A pane's tab bar: the first tab drawn and the tab it last brought into view, so that it
/// follows a new tab but the wheel can move it freely.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabBar {
    pub scroll: usize,
    pub shown: Option<usize>,
}

impl DropZone {
    /// The outer quarter of each side splits; the rest of the editor joins.
    pub fn at(area: Rect, point: (u16, u16)) -> Self {
        let x = f32::from(point.0.saturating_sub(area.x)) / f32::from(area.width.max(1));
        let y = f32::from(point.1.saturating_sub(area.y)) / f32::from(area.height.max(1));
        if x < 0.25 {
            Self::Left
        } else if x >= 0.75 {
            Self::Right
        } else if y < 0.25 {
            Self::Top
        } else if y >= 0.75 {
            Self::Bottom
        } else {
            Self::Center
        }
    }
}

/// A tab held with the mouse: the tab shown before it was pressed and the zone under the
/// pointer once it leaves the tab bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TabDrag {
    pub tab: usize,
    pub previous: Option<usize>,
    pub zone: Option<DropZone>,
}

/// Text chosen with the mouse in a terminal (Q57): the cells (row, column inside its border)
/// where the drag started and where it is, in the tab on screen of the panel `panel` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalSelection {
    pub panel: Focus,
    pub anchor: (u16, u16),
    pub end: (u16, u16),
}

impl TerminalSelection {
    /// The first and the last cell, in reading order.
    pub fn bounds(&self) -> ((u16, u16), (u16, u16)) {
        (self.anchor.min(self.end), self.anchor.max(self.end))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DragTarget {
    Explorer,
    RightTerminal,
    BottomTerminal,
}

#[derive(Default)]
pub struct Regions {
    pub body: Rect,
    pub explorer: Rect,
    pub explorer_handle: Rect,
    pub tabs: Rect,
    pub editor: Rect,
    pub bottom_terminal: Rect,
    pub bottom_handle: Rect,
    pub right_terminal: Rect,
    pub right_handle: Rect,
    pub header_help: Rect,
    pub header_about: Rect,
    pub update_install: Rect,
    pub update_skip: Rect,
    pub about_repository: Rect,
    pub about_update: Rect,
    /// About's Release notes, and the All, New and Fixed of the notes dialog (F18).
    pub about_notes: Rect,
    pub notes_filters: Vec<(NotesFilter, Rect)>,
    pub dialog_next: Rect,
    pub dialog_skip: Rect,
    /// Both editor panes together; `editor` is the text area of the active pane.
    pub editor_body: Rect,
    /// Each pane with its tab bar, its tab bar and its text area (the second one only when
    /// split).
    pub pane_areas: [Rect; 2],
    pub tab_bars: [Rect; 2],
    pub pane_bodies: [Rect; 2],
    /// The `‹` and `›` of a tab bar with hidden tabs: pane, right side, area.
    pub tab_arrows: Vec<(usize, bool, Rect)>,
    pub search_input: Rect,
    pub search_results: Rect,
    pub search_match_items: Vec<(usize, Rect)>,
    /// The `◂` and `▸` of the search counter.
    pub search_previous: Rect,
    pub search_next: Rect,
    pub tab_items: Vec<(usize, Rect, Rect)>,
    pub entry_menu_items: Vec<(super::EntryAction, Rect)>,
    /// The tabs of the terminals: panel, tab, the tab and its `x` (empty when it has none).
    pub terminal_tabs: Vec<(Focus, usize, Rect, Rect)>,
    /// The `+` of the bottom terminal.
    pub terminal_add: Rect,
    /// The keys Help lets change (F29), and its Reset button.
    pub help_keys: Vec<(super::Action, Rect)>,
    pub help_reset: Rect,
}

/// A video's tab inside its border: the picture, then the progress row and the button row
/// (F23); the view draws them and the mouse is matched against them.
pub fn video_areas(area: Rect) -> [Rect; 3] {
    let inner = Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    let controls = inner.height.min(2);
    let picture = Rect::new(inner.x, inner.y, inner.width, inner.height - controls);
    let row = |offset: u16| {
        let y = picture.bottom() + offset;
        Rect::new(inner.x, y, inner.width, u16::from(offset < controls))
    };
    [picture, row(0), row(1)]
}

pub(super) fn inner_size(area: Rect) -> (u16, u16) {
    (
        area.width.saturating_sub(2).max(1),
        area.height.saturating_sub(2).max(1),
    )
}
