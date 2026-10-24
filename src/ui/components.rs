use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
};

pub(crate) const ACTIVE: Color = Color::Cyan;
pub(crate) const TEXT: Color = Color::Reset;
pub(crate) const MUTED: Color = Color::DarkGray;
pub(crate) const BORDER: Color = Color::DarkGray;

pub(crate) fn panel_block(title: String, focused: bool) -> Block<'static> {
    Block::default()
        .title(Line::from(title))
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(if focused { ACTIVE } else { BORDER }))
        .style(Style::default().fg(TEXT))
}

pub(crate) fn render_handle(frame: &mut Frame, area: Rect, vertical: bool) {
    let symbol = if vertical { "│" } else { "─" };
    let lines = (0..area.width.max(area.height))
        .map(|_| Line::from(symbol))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(BORDER)),
        area,
    );
}

pub(crate) fn centered(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - height_percent) / 2),
        Constraint::Percentage(height_percent),
        Constraint::Percentage((100 - height_percent) / 2),
    ])
    .split(area);
    Layout::horizontal([
        Constraint::Percentage((100 - width_percent) / 2),
        Constraint::Percentage(width_percent),
        Constraint::Percentage((100 - width_percent) / 2),
    ])
    .split(vertical[1])[1]
}

pub(crate) fn scale_length(length: u16, progress: f32) -> u16 {
    ((length as f32 * progress.clamp(0.0, 1.0)).round() as u16).min(length)
}

pub(crate) fn reveal(area: Rect, progress: f32) -> Rect {
    Rect::new(
        area.x,
        area.y,
        scale_length(area.width, progress),
        area.height,
    )
}

pub(crate) fn scale_centered(area: Rect, progress: f32) -> Rect {
    let width = scale_length(area.width, progress);
    let height = scale_length(area.height, progress);
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animated_geometry_stays_inside_its_final_area() {
        let area = Rect::new(10, 8, 41, 19);
        let revealed = reveal(area, 0.5);
        let scaled = scale_centered(area, 0.5);

        assert_eq!(revealed, Rect::new(10, 8, 21, 19));
        assert_eq!(scaled, Rect::new(20, 12, 21, 10));
        assert!(scaled.x >= area.x && scaled.right() <= area.right());
        assert!(scaled.y >= area.y && scaled.bottom() <= area.bottom());
        assert_eq!(scale_length(30, 2.0), 30);
        assert_eq!(scale_length(30, -1.0), 0);
    }
}
