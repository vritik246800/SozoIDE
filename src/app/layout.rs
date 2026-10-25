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
