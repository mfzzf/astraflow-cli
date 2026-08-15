use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateChoice {
    UpdateNow,
    RemindLater,
    SkipVersion,
}

const CHOICES: [UpdateChoice; 3] = [
    UpdateChoice::UpdateNow,
    UpdateChoice::RemindLater,
    UpdateChoice::SkipVersion,
];

pub fn prompt(current: &str, latest: &str) -> Result<UpdateChoice> {
    let mut selected = 0;
    ratatui::run(|terminal| {
        loop {
            terminal.draw(|frame| render(frame, current, latest, selected))?;
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind == KeyEventKind::Release {
                continue;
            }
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                return Ok(UpdateChoice::RemindLater);
            }
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    selected = (selected + CHOICES.len() - 1) % CHOICES.len()
                }
                KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                    selected = (selected + 1) % CHOICES.len()
                }
                KeyCode::Enter => return Ok(CHOICES[selected]),
                KeyCode::Esc => return Ok(UpdateChoice::RemindLater),
                _ => {}
            }
        }
    })
}

fn render(frame: &mut Frame<'_>, current: &str, latest: &str, selected: usize) {
    let area = centered_rect(frame.area(), 72, 13);
    frame.render_widget(Clear, area);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Line::from(vec![
            Span::styled(
                " AstraFlow ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " Update available / 发现新版本 ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [summary_area, choices_area, help_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(2),
    ])
    .areas(inner);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("A new release is ready  ", Style::default().fg(Color::Gray)),
                Span::styled(current, Style::default().fg(Color::DarkGray)),
                Span::styled("  →  ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    latest,
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from("Choose when AstraFlow should install it. / 请选择更新方式。"),
        ])
        .wrap(Wrap { trim: true }),
        summary_area,
    );
    let items = [
        "Update now          Install and exit / 立即更新",
        "Remind me later     Ask again in 24h / 稍后提醒",
        "Skip this version   Resume on next release / 跳过此版本",
    ]
    .into_iter()
    .map(ListItem::new);
    let list = List::new(items).highlight_symbol("  ▶ ").highlight_style(
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    let mut state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(list, choices_area, &mut state);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("↑↓", Style::default().fg(Color::Cyan)),
            Span::raw(" choose   "),
            Span::styled("Enter", Style::default().fg(Color::Cyan)),
            Span::raw(" confirm   "),
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" later"),
        ]))
        .style(Style::default().fg(Color::DarkGray)),
        help_area,
    );
}

fn centered_rect(area: Rect, preferred_width: u16, preferred_height: u16) -> Rect {
    let width = preferred_width.min(area.width.saturating_sub(2)).max(1);
    let height = preferred_height.min(area.height.saturating_sub(2)).max(1);
    let [vertical] = Layout::vertical([Constraint::Length(height)])
        .flex(ratatui::layout::Flex::Center)
        .areas(area);
    let [centered] = Layout::horizontal([Constraint::Length(width)])
        .flex(ratatui::layout::Flex::Center)
        .areas(vertical);
    centered
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn update_dialog_renders_versions_and_all_choices() {
        let backend = TestBackend::new(90, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render(frame, "0.3.0", "0.3.1", 0))
            .unwrap();
        let rendered = terminal.backend().to_string();
        for expected in [
            "Update available",
            "0.3.0",
            "0.3.1",
            "Update now",
            "Remind me later",
            "Skip this version",
        ] {
            assert!(
                rendered.contains(expected),
                "missing {expected}:\n{rendered}"
            );
        }
    }
}
