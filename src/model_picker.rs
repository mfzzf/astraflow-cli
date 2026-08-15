use crate::config::HarnessModelSettings;
use crate::modelverse::{AvailableModel, compact_price_summary, price_summary};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table, TableState, Tabs, Wrap},
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSlot {
    pub key: &'static str,
    pub label: &'static str,
    pub multiple: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerAction {
    Continue,
    Save(HarnessModelSettings),
    Submit(HarnessModelSettings),
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickerKey {
    Tab,
    BackTab,
    Left,
    Right,
    Up,
    Down,
    Enter,
    Escape,
    Backspace,
    CtrlC,
    Char(char),
}

#[derive(Debug)]
pub struct ModelPicker {
    models: Vec<AvailableModel>,
    slots: Vec<ModelSlot>,
    selected: HarnessModelSettings,
    cursor: BTreeMap<String, String>,
    active: usize,
    query: String,
}

impl ModelPicker {
    pub fn new(
        models: Vec<AvailableModel>,
        slots: Vec<ModelSlot>,
        mut defaults: HarnessModelSettings,
    ) -> Self {
        assert!(
            !models.is_empty(),
            "model picker requires at least one model"
        );
        assert!(!slots.is_empty(), "model picker requires at least one slot");
        let first = models[0].id.clone();
        defaults
            .cycle
            .retain(|id| models.iter().any(|model| model.id.eq_ignore_ascii_case(id)));
        let cursor: BTreeMap<String, String> = slots
            .iter()
            .map(|slot| {
                let model = if slot.multiple {
                    defaults.cycle.first()
                } else {
                    defaults.slots.get(slot.key)
                }
                .filter(|id| models.iter().any(|model| model.id.eq_ignore_ascii_case(id)))
                .cloned()
                .unwrap_or_else(|| first.clone());
                (slot.key.to_owned(), model)
            })
            .collect();
        for slot in slots.iter().filter(|slot| !slot.multiple) {
            defaults.slots.insert(
                slot.key.to_owned(),
                cursor
                    .get(slot.key)
                    .cloned()
                    .unwrap_or_else(|| first.clone()),
            );
        }
        Self {
            models,
            slots,
            selected: defaults,
            cursor,
            active: 0,
            query: String::new(),
        }
    }

    fn apply(&mut self, key: PickerKey) -> PickerAction {
        match key {
            PickerKey::Tab | PickerKey::Right => self.change_slot(1),
            PickerKey::BackTab | PickerKey::Left => self.change_slot(-1),
            PickerKey::Up => self.change_model(-1),
            PickerKey::Down => self.change_model(1),
            PickerKey::Char(' ') if self.current_slot().multiple => self.toggle_current_model(),
            PickerKey::Enter => return PickerAction::Submit(self.selected.clone()),
            PickerKey::Escape if !self.query.is_empty() => self.query.clear(),
            PickerKey::Escape | PickerKey::CtrlC => return PickerAction::Cancel,
            PickerKey::Char('D') => return PickerAction::Save(self.selected.clone()),
            PickerKey::Char('/') => self.query.clear(),
            PickerKey::Backspace => {
                self.query.pop();
                self.ensure_visible_selection();
            }
            PickerKey::Char(character) if !character.is_control() && !character.is_whitespace() => {
                self.query.push(character.to_ascii_lowercase());
                self.ensure_visible_selection();
            }
            _ => {}
        }
        PickerAction::Continue
    }

    pub fn run<F>(&mut self, mut save: F) -> anyhow::Result<HarnessModelSettings>
    where
        F: FnMut(HarnessModelSettings) -> anyhow::Result<()>,
    {
        ratatui::run(|terminal| {
            let mut notice = String::new();
            loop {
                terminal.draw(|frame| self.render(frame, &notice))?;
                notice.clear();
                let Event::Key(event) = event::read()? else {
                    continue;
                };
                if event.kind == KeyEventKind::Release {
                    continue;
                }
                let Some(key) = picker_key(event) else {
                    continue;
                };
                match self.apply(key) {
                    PickerAction::Continue => {}
                    PickerAction::Save(models) => {
                        save(models)?;
                        notice =
                            "✓ 已保存为 AstraFlow 默认组合 / Saved as AstraFlow defaults".into();
                    }
                    PickerAction::Submit(models) => return Ok(models),
                    PickerAction::Cancel => anyhow::bail!("model selection cancelled"),
                }
            }
        })
    }

    fn change_slot(&mut self, delta: isize) {
        let length = self.slots.len() as isize;
        self.active = (self.active as isize + delta).rem_euclid(length) as usize;
        self.query.clear();
    }

    fn change_model(&mut self, delta: isize) {
        let matches = self.matches();
        if matches.is_empty() {
            return;
        }
        let current = self.current_cursor();
        let position = matches
            .iter()
            .position(|index| self.models[*index].id.eq_ignore_ascii_case(current))
            .unwrap_or(0);
        let next = (position as isize + delta).rem_euclid(matches.len() as isize) as usize;
        let id = self.models[matches[next]].id.clone();
        self.cursor
            .insert(self.current_slot().key.to_owned(), id.clone());
        if !self.current_slot().multiple {
            self.selected
                .slots
                .insert(self.current_slot().key.to_owned(), id);
        }
    }

    fn ensure_visible_selection(&mut self) {
        let matches = self.matches();
        if matches.is_empty() {
            return;
        }
        if let Some(index) = matches
            .iter()
            .find(|index| self.models[**index].id.eq_ignore_ascii_case(&self.query))
        {
            let id = self.models[*index].id.clone();
            self.cursor
                .insert(self.current_slot().key.to_owned(), id.clone());
            if !self.current_slot().multiple {
                self.selected
                    .slots
                    .insert(self.current_slot().key.to_owned(), id);
            }
            return;
        }
        if !matches.iter().any(|index| {
            self.models[*index]
                .id
                .eq_ignore_ascii_case(self.current_cursor())
        }) {
            let id = self.models[matches[0]].id.clone();
            self.cursor
                .insert(self.current_slot().key.to_owned(), id.clone());
            if !self.current_slot().multiple {
                self.selected
                    .slots
                    .insert(self.current_slot().key.to_owned(), id);
            }
        }
    }

    fn toggle_current_model(&mut self) {
        let model = self.current_cursor().to_owned();
        if let Some(index) = self
            .selected
            .cycle
            .iter()
            .position(|item| item.eq_ignore_ascii_case(&model))
        {
            self.selected.cycle.remove(index);
        } else {
            self.selected.cycle.push(model);
        }
    }

    fn matches(&self) -> Vec<usize> {
        self.models
            .iter()
            .enumerate()
            .filter(|(_, model)| {
                self.query.is_empty() || model.id.to_ascii_lowercase().contains(&self.query)
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn current_slot(&self) -> ModelSlot {
        self.slots[self.active]
    }

    fn current_cursor(&self) -> &str {
        self.cursor
            .get(self.current_slot().key)
            .expect("every model slot has a cursor")
    }

    fn render(&self, frame: &mut Frame<'_>, notice: &str) {
        let area = frame.area();
        let outer = Block::new()
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
                    " Model Picker / 模型选择 ",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        let inner = outer.inner(area);
        frame.render_widget(outer, area);
        if inner.width < 30 || inner.height < 12 {
            frame.render_widget(
                Paragraph::new(
                    "Terminal is too small / 终端窗口太小\nPlease resize to at least 30 × 12",
                )
                .style(Style::default().fg(Color::Yellow))
                .block(
                    Block::new()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded),
                ),
                inner,
            );
            return;
        }

        let [tabs_area, search_area, models_area, detail_area, help_area] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(4),
            Constraint::Length(3),
        ])
        .areas(inner);

        let tab_titles = self
            .slots
            .iter()
            .map(|slot| Line::from(format!(" {} ", slot.label)))
            .collect::<Vec<_>>();
        let tabs = Tabs::new(tab_titles)
            .select(self.active)
            .block(
                Block::new()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(" Roles / 模型角色 "),
            )
            .style(Style::default().fg(Color::DarkGray))
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .divider(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        frame.render_widget(tabs, tabs_area);

        let search = if self.query.is_empty() {
            Line::from(vec![
                Span::styled("› ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    "直接输入模型名称 / Type to search",
                    Style::default().fg(Color::DarkGray),
                ),
            ])
        } else {
            Line::from(vec![
                Span::styled("› ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    self.query.as_str(),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("█", Style::default().fg(Color::Cyan)),
            ])
        };
        frame.render_widget(
            Paragraph::new(search).block(
                Block::new()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::DarkGray))
                    .title(" Search / 搜索 "),
            ),
            search_area,
        );

        let matches = self.matches();
        let title = format!(
            " Models / 模型  {} of {} ",
            matches.len(),
            self.models.len()
        );
        if matches.is_empty() {
            frame.render_widget(
                Paragraph::new("\n  No matching model / 没有匹配模型")
                    .style(Style::default().fg(Color::Yellow))
                    .block(
                        Block::new()
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded)
                            .title(title),
                    ),
                models_area,
            );
        } else {
            let selected_position = matches
                .iter()
                .position(|index| {
                    self.models[*index]
                        .id
                        .eq_ignore_ascii_case(self.current_cursor())
                })
                .unwrap_or(0);
            let rows = matches.iter().map(|index| {
                let model = &self.models[*index];
                let marker = if self.current_slot().multiple {
                    if self
                        .selected
                        .cycle
                        .iter()
                        .any(|id| id.eq_ignore_ascii_case(&model.id))
                    {
                        "● "
                    } else {
                        "○ "
                    }
                } else {
                    ""
                };
                Row::new([
                    Cell::from(format!("{marker}{}", model.id)),
                    Cell::from(compact_price_summary(model)),
                ])
            });
            let header = Row::new(["MODEL", "TOKEN PRICE"])
                .style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .bottom_margin(1);
            let table = Table::new(
                rows,
                [Constraint::Percentage(46), Constraint::Percentage(54)],
            )
            .header(header)
            .block(
                Block::new()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(title),
            )
            .column_spacing(2)
            .row_highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
            let mut state = TableState::default().with_selected(Some(selected_position));
            frame.render_stateful_widget(table, models_area, &mut state);
        }

        let detail = self
            .models
            .iter()
            .find(|model| model.id.eq_ignore_ascii_case(self.current_cursor()))
            .map(price_summary)
            .unwrap_or_else(|| "Price unavailable".to_owned());
        frame.render_widget(
            Paragraph::new(detail)
                .style(Style::default().fg(Color::Gray))
                .wrap(Wrap { trim: true })
                .block(
                    Block::new()
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(Color::DarkGray))
                        .title(" Selected price / 当前模型价格 "),
                ),
            detail_area,
        );

        let mut help = vec![Line::from(vec![
            key("Tab / ←→"),
            Span::raw(" role  "),
            key("↑↓"),
            Span::raw(" model  "),
            key("Space"),
            Span::raw(" pool  "),
            key("D"),
            Span::raw(" default  "),
            key("Enter"),
            Span::raw(" launch  "),
            key("Esc"),
            Span::raw(" cancel"),
        ])];
        if !notice.is_empty() {
            help.push(Line::from(Span::styled(
                notice,
                Style::default().fg(Color::Green),
            )));
        }
        frame.render_widget(
            Paragraph::new(help).block(
                Block::new()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(Color::DarkGray)),
            ),
            help_area,
        );
    }
}

fn picker_key(event: KeyEvent) -> Option<PickerKey> {
    if event.modifiers.contains(KeyModifiers::CONTROL) && event.code == KeyCode::Char('c') {
        return Some(PickerKey::CtrlC);
    }
    Some(match event.code {
        KeyCode::Tab => PickerKey::Tab,
        KeyCode::BackTab => PickerKey::BackTab,
        KeyCode::Left => PickerKey::Left,
        KeyCode::Right => PickerKey::Right,
        KeyCode::Up => PickerKey::Up,
        KeyCode::Down => PickerKey::Down,
        KeyCode::Enter => PickerKey::Enter,
        KeyCode::Esc => PickerKey::Escape,
        KeyCode::Backspace => PickerKey::Backspace,
        KeyCode::Char(character) => PickerKey::Char(character),
        _ => return None,
    })
}

fn key(label: &'static str) -> Span<'static> {
    Span::styled(
        label,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str) -> AvailableModel {
        AvailableModel {
            id: id.into(),
            created: 0,
            pricing: Vec::new(),
        }
    }

    #[test]
    fn tabs_and_arrows_change_independent_slot_selections() {
        let mut picker = ModelPicker::new(
            vec![model("deepseek-v4-flash-0731"), model("glm-5.2")],
            vec![
                ModelSlot {
                    key: "default",
                    label: "Default",
                    multiple: false,
                },
                ModelSlot {
                    key: "small",
                    label: "Small",
                    multiple: false,
                },
            ],
            HarnessModelSettings::default(),
        );
        picker.apply(PickerKey::Down);
        picker.apply(PickerKey::Tab);
        assert!(
            matches!(picker.apply(PickerKey::Enter), PickerAction::Submit(models) if models.slots["default"] == "glm-5.2" && models.slots["small"] == "deepseek-v4-flash-0731")
        );
    }

    #[test]
    fn typing_filters_and_uppercase_d_saves() {
        let mut picker = ModelPicker::new(
            vec![
                model("deepseek-v4-flash-0731"),
                model("glm-5.2-sg"),
                model("glm-5.2"),
            ],
            vec![ModelSlot {
                key: "default",
                label: "Default",
                multiple: false,
            }],
            HarnessModelSettings::default(),
        );
        for key in "glm-5.2".chars().map(PickerKey::Char) {
            picker.apply(key);
        }
        assert!(
            matches!(picker.apply(PickerKey::Char('D')), PickerAction::Save(models) if models.slots["default"] == "glm-5.2")
        );
    }

    #[test]
    fn cycle_pool_supports_space_to_toggle_multiple_models() {
        let mut picker = ModelPicker::new(
            vec![model("deepseek-v4-flash-0731"), model("glm-5.2")],
            vec![ModelSlot {
                key: "cycle",
                label: "Cycle Pool",
                multiple: true,
            }],
            HarnessModelSettings::default(),
        );
        picker.apply(PickerKey::Char(' '));
        picker.apply(PickerKey::Down);
        picker.apply(PickerKey::Char(' '));
        assert!(
            matches!(picker.apply(PickerKey::Enter), PickerAction::Submit(models) if models.cycle == ["deepseek-v4-flash-0731", "glm-5.2"])
        );
    }

    #[test]
    fn ratatui_layout_renders_search_models_prices_and_help() {
        use ratatui::{Terminal, backend::TestBackend};

        let picker = ModelPicker::new(
            vec![model("deepseek-v4-flash-0731"), model("glm-5.2")],
            vec![ModelSlot {
                key: "default",
                label: "Default",
                multiple: false,
            }],
            HarnessModelSettings::default(),
        );
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| picker.render(frame, "Saved as AstraFlow defaults"))
            .unwrap();
        let rendered = terminal.backend().to_string();
        for expected in [
            "AstraFlow",
            "Model Picker",
            "Search",
            "deepseek-v4-flash-0731",
            "TOKEN PRICE",
            "Saved as AstraFlow defaults",
        ] {
            assert!(
                rendered.contains(expected),
                "missing {expected}:\n{rendered}"
            );
        }
    }
}
