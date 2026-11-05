use ratatui::layout::Rect;
use serde::{Deserialize, Serialize};

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
    Setup,
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

/// Two editor panes, or two tiles: side by side (`Columns`) or stacked (`Rows`).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum SplitDirection {
    Columns,
    Rows,
}

/// A panel of the window (F39).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum Panel {
    Explorer,
    Editor,
    BottomTerminal,
    RightTerminal,
}

impl Panel {
    pub const ALL: [Self; 4] = [
        Self::Explorer,
        Self::Editor,
        Self::BottomTerminal,
        Self::RightTerminal,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Explorer => "Explorer",
            Self::Editor => "Editor",
            Self::BottomTerminal => "Bottom terminal",
            Self::RightTerminal => "Right terminal",
        }
    }
}

/// How the window is shared (F39): a binary tree with every panel once. A split gives the side
/// without the editor its length and the editor's side the rest, so hiding a panel gives its
/// room to what is beside it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub enum Tile {
    Panel(Panel),
    Split {
        direction: SplitDirection,
        first: Box<Tile>,
        second: Box<Tile>,
    },
}

/// What the editor's side of a split keeps: the rest goes to the other side.
const EDITOR_COLUMNS: u16 = 20;
const EDITOR_ROWS: u16 = 3;

/// A divider between two tiles: `path` leads to its split (false: first, true: second), `area`
/// is what the split shares, and `first` says its first side is the one with a length.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Divider {
    pub handle: Rect,
    pub area: Rect,
    pub direction: Option<SplitDirection>,
    pub path: Vec<bool>,
    pub first: bool,
}

/// The tree placed in a rectangle: the rectangle of each panel on screen and the dividers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tiling {
    pub panels: Vec<(Panel, Rect)>,
    pub dividers: Vec<Divider>,
}

impl Tiling {
    pub fn area(&self, panel: Panel) -> Rect {
        self.panels
            .iter()
            .find(|(placed, _)| *placed == panel)
            .map_or_else(Rect::default, |(_, area)| *area)
    }
}

impl Default for Tile {
    /// Explorer on the left, the right terminal on the right, the bottom one under the editor.
    fn default() -> Self {
        use Panel::{BottomTerminal, Editor, Explorer, RightTerminal};
        use SplitDirection::{Columns, Rows};
        let split = |direction, first, second| Self::split(direction, first, second);
        split(
            Columns,
            Self::Panel(Explorer),
            split(
                Columns,
                split(Rows, Self::Panel(Editor), Self::Panel(BottomTerminal)),
                Self::Panel(RightTerminal),
            ),
        )
    }
}

impl Tile {
    fn split(direction: SplitDirection, first: Self, second: Self) -> Self {
        Self::Split {
            direction,
            first: Box::new(first),
            second: Box::new(second),
        }
    }

    fn panels(&self, list: &mut Vec<Panel>) {
        match self {
            Self::Panel(panel) => list.push(*panel),
            Self::Split { first, second, .. } => {
                first.panels(list);
                second.panels(list);
            }
        }
    }

    /// Every panel once: what a saved layout must be to be used.
    pub fn is_complete(&self) -> bool {
        let mut list = Vec::new();
        self.panels(&mut list);
        list.len() == Panel::ALL.len() && Panel::ALL.iter().all(|panel| list.contains(panel))
    }

    fn has_editor(&self) -> bool {
        let mut list = Vec::new();
        self.panels(&mut list);
        list.contains(&Panel::Editor)
    }

    /// The length the tile asks for along `direction`, 0 when nothing of it shows.
    fn wanted(
        &self,
        direction: SplitDirection,
        length: &dyn Fn(Panel, SplitDirection) -> u16,
    ) -> u16 {
        match self {
            Self::Panel(Panel::Editor) => 1,
            Self::Panel(panel) => length(*panel, direction),
            Self::Split {
                direction: along,
                first,
                second,
            } => {
                let (first, second) = (
                    first.wanted(direction, length),
                    second.wanted(direction, length),
                );
                if *along != direction || first == 0 || second == 0 {
                    first.max(second)
                } else {
                    first.saturating_add(second).saturating_add(1)
                }
            }
        }
    }

    /// Places the tree in `area`; `length` says each panel's length in a direction, 0 for a
    /// hidden one.
    pub fn place(&self, area: Rect, length: &dyn Fn(Panel, SplitDirection) -> u16) -> Tiling {
        let mut tiling = Tiling::default();
        self.place_in(area, length, &mut Vec::new(), &mut tiling);
        tiling
    }

    fn place_in(
        &self,
        area: Rect,
        length: &dyn Fn(Panel, SplitDirection) -> u16,
        path: &mut Vec<bool>,
        tiling: &mut Tiling,
    ) {
        let Self::Split {
            direction,
            first,
            second,
        } = self
        else {
            if let Self::Panel(panel) = self
                && (*panel == Panel::Editor || self.wanted(SplitDirection::Columns, length) > 0)
            {
                tiling.panels.push((*panel, area));
            }
            return;
        };
        let direction = *direction;
        let (wanted_first, wanted_second) = (
            first.wanted(direction, length),
            second.wanted(direction, length),
        );
        let here = path.clone();
        let mut go = |tile: &Self, side: bool, part: Rect, tiling: &mut Tiling| {
            path.push(side);
            tile.place_in(part, length, path, tiling);
            path.pop();
        };
        if wanted_first == 0 || wanted_second == 0 {
            let (tile, side) = if wanted_first == 0 {
                (second, true)
            } else {
                (first, false)
            };
            go(tile, side, area, tiling);
            return;
        }
        let total = match direction {
            SplitDirection::Columns => area.width,
            SplitDirection::Rows => area.height,
        };
        let fixed_first = !first.has_editor();
        let (wanted, kept) = if fixed_first {
            (wanted_first, second.has_editor())
        } else {
            (wanted_second, true)
        };
        let keep = match (kept, direction) {
            (true, SplitDirection::Columns) => EDITOR_COLUMNS,
            (true, SplitDirection::Rows) => EDITOR_ROWS,
            (false, _) => 1,
        };
        let room = total.saturating_sub(1);
        let fixed = wanted.min(room.saturating_sub(keep));
        let before = if fixed_first {
            fixed
        } else {
            room.saturating_sub(fixed)
        };
        let (one, handle, two) = match direction {
            SplitDirection::Columns => (
                Rect::new(area.x, area.y, before, area.height),
                Rect::new(area.x + before, area.y, 1, area.height),
                Rect::new(area.x + before + 1, area.y, room - before, area.height),
            ),
            SplitDirection::Rows => (
                Rect::new(area.x, area.y, area.width, before),
                Rect::new(area.x, area.y + before, area.width, 1),
                Rect::new(area.x, area.y + before + 1, area.width, room - before),
            ),
        };
        tiling.dividers.push(Divider {
            handle,
            area,
            direction: Some(direction),
            path: here,
            first: fixed_first,
        });
        go(first, false, one, tiling);
        go(second, true, two, tiling);
    }

    fn at(&self, path: &[bool]) -> Option<&Self> {
        match (path.split_first(), self) {
            (None, tile) => Some(tile),
            (Some((side, rest)), Self::Split { first, second, .. }) => {
                if *side { second } else { first }.at(rest)
            }
            _ => None,
        }
    }

    /// The panels a divider gives `value` cells to and how many each gets: the panels on its
    /// fixed side, in a split along it only the one that touches the divider.
    pub fn resized(
        &self,
        divider: &Divider,
        value: u16,
        length: &dyn Fn(Panel, SplitDirection) -> u16,
    ) -> Vec<(Panel, u16)> {
        let mut sized = Vec::new();
        if let (Some(direction), Some(Self::Split { first, second, .. })) =
            (divider.direction, self.at(&divider.path))
        {
            let fixed = if divider.first { first } else { second };
            fixed.resize(direction, value, divider.first, length, &mut sized);
        }
        sized
    }

    fn resize(
        &self,
        direction: SplitDirection,
        value: u16,
        toward_end: bool,
        length: &dyn Fn(Panel, SplitDirection) -> u16,
        sized: &mut Vec<(Panel, u16)>,
    ) {
        match self {
            Self::Panel(panel) => sized.push((*panel, value)),
            Self::Split {
                direction: along,
                first,
                second,
            } if *along != direction => {
                first.resize(direction, value, toward_end, length, sized);
                second.resize(direction, value, toward_end, length, sized);
            }
            Self::Split { first, second, .. } => {
                let (near, far) = if toward_end {
                    (second, first)
                } else {
                    (first, second)
                };
                match far.wanted(direction, length) {
                    0 => near.resize(direction, value, toward_end, length, sized),
                    far => near.resize(
                        direction,
                        value.saturating_sub(far + 1).max(1),
                        toward_end,
                        length,
                        sized,
                    ),
                }
            }
        }
    }

    /// The tree without `panel`: its sibling takes the place of their split.
    fn without(self, panel: Panel) -> Option<Self> {
        match self {
            Self::Panel(this) => (this != panel).then_some(self),
            Self::Split {
                direction,
                first,
                second,
            } => match (first.without(panel), second.without(panel)) {
                (Some(first), Some(second)) => Some(Self::split(direction, first, second)),
                (first, second) => first.or(second),
            },
        }
    }

    /// `tile` with `panel` on the `zone` side of it.
    fn beside(self, panel: Panel, zone: DropZone) -> Self {
        use SplitDirection::{Columns, Rows};
        let new = Self::Panel(panel);
        match zone {
            DropZone::Left => Self::split(Columns, new, self),
            DropZone::Right => Self::split(Columns, self, new),
            DropZone::Top => Self::split(Rows, new, self),
            DropZone::Bottom => Self::split(Rows, self, new),
            DropZone::Center | DropZone::Pane(_) => self,
        }
    }

    fn map_panel(self, panel: Panel, change: &mut Option<impl FnOnce(Self) -> Self>) -> Self {
        match self {
            Self::Panel(this) if this == panel => match change.take() {
                Some(change) => change(self),
                None => self,
            },
            Self::Panel(_) => self,
            Self::Split {
                direction,
                first,
                second,
            } => {
                let first = first.map_panel(panel, change);
                Self::split(direction, first, second.map_panel(panel, change))
            }
        }
    }

    fn swapped(self, one: Panel, other: Panel) -> Self {
        match self {
            Self::Panel(panel) if panel == one => Self::Panel(other),
            Self::Panel(panel) if panel == other => Self::Panel(one),
            Self::Panel(_) => self,
            Self::Split {
                direction,
                first,
                second,
            } => Self::split(
                direction,
                first.swapped(one, other),
                second.swapped(one, other),
            ),
        }
    }

    /// `panel` dropped on `zone` of `target`, or of the whole window without a target: on a
    /// side it shares the target's place, on the centre the two change places.
    pub fn moved(self, panel: Panel, target: Option<Panel>, zone: DropZone) -> Self {
        if target == Some(panel) {
            return self;
        }
        if let (Some(target), DropZone::Center) = (target, zone) {
            return self.swapped(panel, target);
        }
        let Some(rest) = self.clone().without(panel) else {
            return self;
        };
        match target {
            Some(target) => {
                let mut change = Some(|tile: Self| tile.beside(panel, zone));
                rest.map_panel(target, &mut change)
            }
            None => rest.beside(panel, zone),
        }
    }
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

/// A panel held by its title (F39): where it was pressed and where it would land, on a panel
/// or, without one, on an edge of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelDrag {
    pub panel: Panel,
    pub from: (u16, u16),
    pub zone: Option<(Option<Panel>, DropZone)>,
}

#[derive(Default)]
pub struct Regions {
    pub body: Rect,
    pub explorer: Rect,
    /// The panels and dividers as placed (F39).
    pub tiling: Tiling,
    pub tabs: Rect,
    pub editor: Rect,
    pub bottom_terminal: Rect,
    pub right_terminal: Rect,
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
    /// The rows of the preview beside the editor's menu (Q73) that a click acts on.
    pub preview_rows: Vec<(super::PreviewRow, Rect)>,
    /// The Properties dialog: a click outside it closes it.
    pub properties: Rect,
    /// The hover and Peek boxes of Code Intelligence (F31): the wheel scrolls them, the mouse
    /// over them keeps them open.
    pub hover: Rect,
    pub peek: Rect,
    /// The tabs of the terminals: panel, tab, the tab and its `x` (empty when it has none).
    pub terminal_tabs: Vec<(Focus, usize, Rect, Rect)>,
    /// The `+` of the bottom terminal.
    pub terminal_add: Rect,
    /// The keys Help lets change (F29), and its Reset button.
    pub help_keys: Vec<(super::Action, Rect)>,
    pub help_reset: Rect,
    /// Help's tabs, the Dependencies rows and the projects to choose from (F32, F33).
    pub help_tabs: Vec<(super::HelpTab, Rect)>,
    pub dependency_rows: Vec<(usize, Rect)>,
    pub choice_rows: Vec<(usize, Rect)>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use Panel::{BottomTerminal, Editor, Explorer, RightTerminal};

    const AREA: Rect = Rect::new(0, 1, 120, 30);

    fn sizes(panel: Panel, direction: SplitDirection) -> u16 {
        match (panel, direction) {
            (Explorer, SplitDirection::Columns) => 30,
            (Explorer, SplitDirection::Rows) => 8,
            (BottomTerminal, SplitDirection::Columns) => 40,
            (BottomTerminal, SplitDirection::Rows) => 12,
            (RightTerminal, SplitDirection::Columns) => 42,
            (RightTerminal, SplitDirection::Rows) => 12,
            (Editor, _) => 0,
        }
    }

    #[test]
    fn the_first_layout_is_the_one_the_window_always_had() {
        let tiling = Tile::default().place(AREA, &sizes);
        assert_eq!(tiling.area(Explorer), Rect::new(0, 1, 30, 30));
        assert_eq!(tiling.area(RightTerminal), Rect::new(78, 1, 42, 30));
        assert_eq!(tiling.area(Editor), Rect::new(31, 1, 46, 17));
        assert_eq!(tiling.area(BottomTerminal), Rect::new(31, 19, 46, 12));
        let handles = tiling
            .dividers
            .iter()
            .map(|divider| divider.handle)
            .collect::<Vec<_>>();
        assert_eq!(
            handles,
            [
                Rect::new(30, 1, 1, 30),
                Rect::new(77, 1, 1, 30),
                Rect::new(31, 18, 46, 1)
            ]
        );
    }

    #[test]
    fn a_hidden_panel_leaves_its_room_and_no_divider() {
        let none = |panel: Panel, direction| match panel {
            BottomTerminal | RightTerminal => 0,
            _ => sizes(panel, direction),
        };
        let tiling = Tile::default().place(AREA, &none);
        assert_eq!(tiling.area(Editor), Rect::new(31, 1, 89, 30));
        assert_eq!(tiling.area(RightTerminal), Rect::default());
        assert_eq!(tiling.dividers.len(), 1);
    }

    #[test]
    fn panels_land_where_they_are_dropped_and_the_rest_adjusts() {
        // On a side of a panel: it shares its place.
        let tile = Tile::default().moved(Explorer, Some(RightTerminal), DropZone::Top);
        assert!(tile.is_complete());
        let tiling = tile.place(AREA, &sizes);
        assert_eq!(tiling.area(Explorer), Rect::new(78, 1, 42, 8));
        assert_eq!(tiling.area(RightTerminal), Rect::new(78, 10, 42, 21));
        assert_eq!(
            tiling.area(Editor).x,
            0,
            "the editor takes the room it left"
        );

        // On the edge of the window: the whole edge.
        let tile = Tile::default().moved(BottomTerminal, None, DropZone::Top);
        let tiling = tile.place(AREA, &sizes);
        assert_eq!(tiling.area(BottomTerminal), Rect::new(0, 1, 120, 12));
        assert_eq!(tiling.area(Explorer), Rect::new(0, 14, 30, 17));

        // The editor moves too: it keeps being what fills the window.
        let tile = Tile::default().moved(Editor, Some(Explorer), DropZone::Left);
        assert!(tile.is_complete());
        let tiling = tile.place(AREA, &sizes);
        assert_eq!(tiling.area(Editor).x, 0);
        assert!(tiling.area(Editor).width >= 20);
        assert!(tiling.area(Explorer).x > tiling.area(Editor).x);

        // On the centre: they change places.
        let tile = Tile::default().moved(Explorer, Some(Editor), DropZone::Center);
        let tiling = tile.place(AREA, &sizes);
        assert_eq!(tiling.area(Editor).x, 0);
        assert_eq!(tiling.area(Explorer).height, 8);
        assert!(tile.is_complete());

        // On itself: nothing.
        assert_eq!(
            Tile::default().moved(Explorer, Some(Explorer), DropZone::Left),
            Tile::default()
        );
    }

    #[test]
    fn no_side_takes_the_editor_off_the_window() {
        let tiling = Tile::default().place(Rect::new(0, 0, 40, 10), &sizes);
        assert!(tiling.area(Editor).width >= 1);
        assert!(tiling.panels.iter().all(|(_, area)| area.right() <= 40));
        let tiny = Tile::default().place(Rect::new(0, 0, 3, 2), &sizes);
        assert!(tiny.panels.iter().all(|(_, area)| area.right() <= 3));
    }

    #[test]
    fn a_divider_gives_its_length_to_the_panel_that_touches_it() {
        let tile = Tile::default();
        let tiling = tile.place(AREA, &sizes);
        assert_eq!(
            tile.resized(&tiling.dividers[0], 25, &sizes),
            [(Explorer, 25)]
        );
        assert_eq!(
            tile.resized(&tiling.dividers[1], 50, &sizes),
            [(RightTerminal, 50)]
        );
        assert_eq!(
            tile.resized(&tiling.dividers[2], 9, &sizes),
            [(BottomTerminal, 9)]
        );
        // Two panels side by side on the fixed side: the one at the divider takes the change.
        let tile = Tile::default().moved(RightTerminal, Some(Explorer), DropZone::Right);
        let tiling = tile.place(AREA, &sizes);
        let divider = tiling
            .dividers
            .iter()
            .find(|divider| divider.path.is_empty())
            .unwrap();
        assert_eq!(divider.handle.x, 73);
        assert_eq!(tile.resized(divider, 80, &sizes), [(RightTerminal, 49)]);
        // Stacked on the fixed side: both.
        let tile = Tile::default().moved(RightTerminal, Some(Explorer), DropZone::Bottom);
        let tiling = tile.place(AREA, &sizes);
        let divider = tiling
            .dividers
            .iter()
            .find(|divider| divider.path.is_empty())
            .unwrap();
        assert_eq!(
            tile.resized(divider, 33, &sizes),
            [(Explorer, 33), (RightTerminal, 33)]
        );
    }

    #[test]
    fn the_layout_goes_through_a_settings_file() {
        #[derive(Deserialize, Serialize)]
        struct File {
            tiles: Tile,
        }
        let tiles = Tile::default().moved(Explorer, Some(Editor), DropZone::Bottom);
        let text = toml::to_string(&File {
            tiles: tiles.clone(),
        })
        .unwrap();
        let read: File = toml::from_str(&text).unwrap();
        assert_eq!(read.tiles, tiles);
        assert!(read.tiles.is_complete());
        assert!(!Tile::Panel(Editor).is_complete());
    }
}
