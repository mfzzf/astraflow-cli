use crate::config::HarnessModelSettings;
use crate::modelverse::{AvailableModel, price_summary};
use console::{Key, Term, style};
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

    pub fn apply(&mut self, key: Key) -> PickerAction {
        match key {
            Key::Tab | Key::ArrowRight => self.change_slot(1),
            Key::BackTab | Key::ArrowLeft => self.change_slot(-1),
            Key::ArrowUp => self.change_model(-1),
            Key::ArrowDown => self.change_model(1),
            Key::Char(' ') if self.current_slot().multiple => self.toggle_current_model(),
            Key::Enter => return PickerAction::Submit(self.selected.clone()),
            Key::Escape if !self.query.is_empty() => self.query.clear(),
            Key::Escape | Key::CtrlC => return PickerAction::Cancel,
            Key::Char('D') => return PickerAction::Save(self.selected.clone()),
            Key::Char('/') => self.query.clear(),
            Key::Backspace => {
                self.query.pop();
                self.ensure_visible_selection();
            }
            Key::Char(character) if !character.is_control() && !character.is_whitespace() => {
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
        let term = Term::stderr();
        let mut rendered_lines = 0;
        let mut notice = String::new();
        loop {
            if rendered_lines > 0 {
                term.clear_last_lines(rendered_lines)?;
            }
            let lines = self.render(&notice, term.size().1 as usize);
            rendered_lines = lines.len();
            for line in lines {
                term.write_line(&line)?;
            }
            notice.clear();
            match self.apply(term.read_key()?) {
                PickerAction::Continue => {}
                PickerAction::Save(models) => {
                    save(models)?;
                    notice = "✓ 已保存为 AstraFlow 默认组合 / Saved as AstraFlow defaults".into();
                }
                PickerAction::Submit(models) => {
                    term.clear_last_lines(rendered_lines)?;
                    return Ok(models);
                }
                PickerAction::Cancel => {
                    term.clear_last_lines(rendered_lines)?;
                    anyhow::bail!("model selection cancelled");
                }
            }
        }
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

    fn render(&self, notice: &str, width: usize) -> Vec<String> {
        let width = width.max(60);
        let tabs = self
            .slots
            .iter()
            .enumerate()
            .map(|(index, slot)| {
                if index == self.active {
                    style(format!("[{}]", slot.label)).cyan().bold().to_string()
                } else {
                    format!(" {} ", slot.label)
                }
            })
            .collect::<Vec<_>>()
            .join("  ");
        let mut lines = vec![
            style("AstraFlow Model Picker / 模型选择")
                .bold()
                .to_string(),
            tabs,
            format!(
                "Search / 搜索: {}",
                if self.query.is_empty() {
                    "(type to search)"
                } else {
                    &self.query
                }
            ),
        ];
        let matches = self.matches();
        if matches.is_empty() {
            lines.push(
                style("  No matching model / 没有匹配模型")
                    .yellow()
                    .to_string(),
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
            let max_rows = 10;
            let start = selected_position.saturating_sub(max_rows / 2);
            for index in matches.iter().skip(start).take(max_rows) {
                let model = &self.models[*index];
                let marker = if model.id.eq_ignore_ascii_case(self.current_cursor()) {
                    ">"
                } else {
                    " "
                };
                let checked = if self.current_slot().multiple {
                    if self
                        .selected
                        .cycle
                        .iter()
                        .any(|id| id.eq_ignore_ascii_case(&model.id))
                    {
                        "[x] "
                    } else {
                        "[ ] "
                    }
                } else {
                    ""
                };
                let line = format!(
                    "{marker} {checked}{}  —  {}",
                    model.id,
                    price_summary(model)
                );
                lines.push(truncate_line(&line, width));
            }
            if matches.len() > max_rows {
                lines.push(format!("  … {} models / 个模型", matches.len()));
            }
        }
        lines.push(
            "Tab/Shift+Tab or ←/→ slot · ↑/↓ model · Space toggle pool · type search · D save defaults · Enter launch · Esc cancel"
                .into(),
        );
        lines.push(if notice.is_empty() {
            " ".into()
        } else {
            notice.into()
        });
        lines
    }
}

fn truncate_line(value: &str, width: usize) -> String {
    let count = value.chars().count();
    if count <= width {
        return value.to_owned();
    }
    value
        .chars()
        .take(width.saturating_sub(1))
        .collect::<String>()
        + "…"
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
        picker.apply(Key::ArrowDown);
        picker.apply(Key::Tab);
        assert!(
            matches!(picker.apply(Key::Enter), PickerAction::Submit(models) if models.slots["default"] == "glm-5.2" && models.slots["small"] == "deepseek-v4-flash-0731")
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
        for key in "glm-5.2".chars().map(Key::Char) {
            picker.apply(key);
        }
        assert!(
            matches!(picker.apply(Key::Char('D')), PickerAction::Save(models) if models.slots["default"] == "glm-5.2")
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
        picker.apply(Key::Char(' '));
        picker.apply(Key::ArrowDown);
        picker.apply(Key::Char(' '));
        assert!(
            matches!(picker.apply(Key::Enter), PickerAction::Submit(models) if models.cycle == ["deepseek-v4-flash-0731", "glm-5.2"])
        );
    }
}
