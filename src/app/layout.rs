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

/// Two editor panes: side by side (`Columns`) or stacked (`Rows`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitDirection {
    Columns,
    Rows,
}

/// The editor split in two (F19): the active tab in one pane, `other` in the other one.
/// `active_first` says whether the active pane is the left/top one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Split {
    pub direction: SplitDirection,
    pub other: usize,
    pub active_first: bool,
}

/// Where a dragged tab is dropped in the editor: an edge splits, the centre joins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropZone {
    Left,
    Right,
    Top,
    Bottom,
    Center,
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
    pub dialog_next: Rect,
    pub dialog_skip: Rect,
    /// Both editor panes together; `editor` is the active pane and `split_editor` the other.
    pub editor_body: Rect,
    pub split_editor: Rect,
    pub search_input: Rect,
    pub search_results: Rect,
    pub search_match_items: Vec<(usize, Rect)>,
    pub tab_items: Vec<(usize, Rect, Rect)>,
}

pub(super) fn inner_size(area: Rect) -> (u16, u16) {
    (
        area.width.saturating_sub(2).max(1),
        area.height.saturating_sub(2).max(1),
    )
}
