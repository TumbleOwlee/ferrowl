use crossterm::event::{KeyCode, KeyModifiers};
use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters};
use ratatui::widgets::{ScrollbarState, TableState as UiTableState};

use crate::EventResult;
use crate::traits::{HandleEvents, IsFocus, SetFocus};
use crate::widgets::{GetValue, TableEntry};

/// State of a [`Table`](crate::widgets::Table) over rows of type `V` with
/// `N` columns: row selection, plus vertical and horizontal scrolling.
///
/// Handles vim-style (`hjkl`, `g`/`G`, `0`/`$`) and arrow/Home/End
/// navigation.
#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters)]
#[getset(set = "pub")]
pub struct TableState<V, const N: usize>
where
    V: TableEntry<N>,
{
    // getset's struct-level `set = "pub"` would otherwise generate an inherent
    // `set_focused` shadowing the `SetFocus` trait impl below.
    #[getset(skip)]
    #[builder(default = "true")]
    focused: bool,
    #[getset(get_copy = "pub")]
    #[builder(setter(skip), default = "ScrollbarState::default()")]
    vertical_scroll: ScrollbarState,
    #[getset(get_copy = "pub")]
    #[builder(setter(skip), default = "0")]
    horizontal_scroll: u16,
    // Selection is coupled to the rows, so the setter is hand-written (see `set_values`)
    // to maintain the selection invariant; getset only generates the getter.
    #[getset(skip)]
    values: Vec<V>,
    #[getset(get_copy = "pub")]
    #[builder(setter(skip), default = "0")]
    visible_width: u16,
    #[getset(get_copy = "pub")]
    #[builder(setter(skip), default = "0")]
    total_width: u16,
    #[getset(get_copy = "pub")]
    #[builder(setter(skip), default = "UiTableState::default().with_selected(0)")]
    table_state: UiTableState,
}

impl<V, const N: usize> GetValue for TableState<V, N>
where
    V: TableEntry<N> + Clone + Default,
{
    type ValueType = V;

    fn get_value(&self) -> Self::ValueType {
        self.values
            .get(self.table_state.selected().unwrap_or(0))
            .map(|v| (*v).clone())
            .unwrap_or_default()
    }
}

impl<V, const N: usize> TableState<V, N>
where
    V: TableEntry<N>,
{
    pub fn focused(&self) -> bool {
        self.focused
    }
}

impl<V, const N: usize> SetFocus for TableState<V, N>
where
    V: TableEntry<N>,
{
    fn set_focused(&mut self, focus: bool) {
        self.focused = focus;
    }
}

impl<V, const N: usize> IsFocus for TableState<V, N>
where
    V: TableEntry<N>,
{
    fn is_focused(&self) -> bool {
        self.focused
    }
}

impl<V, const N: usize> TableState<V, N>
where
    V: TableEntry<N>,
{
    pub fn move_down(&mut self) {
        if self.values.is_empty() {
            self.table_state.select(None);
            self.vertical_scroll = self.vertical_scroll.position(0);
        } else {
            let i = self
                .table_state
                .selected()
                .map(|i| std::cmp::min(i + 1, std::cmp::max(self.values.len(), 1) - 1))
                .unwrap_or(0);
            self.table_state.select(Some(i));
            self.vertical_scroll = self.vertical_scroll.position(i);
        }
    }

    pub fn move_up(&mut self) {
        if self.values.is_empty() {
            self.table_state.select(None);
            self.vertical_scroll = self.vertical_scroll.position(0);
        } else {
            let i = self
                .table_state
                .selected()
                .map(|i| std::cmp::max(i, 1) - 1)
                .unwrap_or(0);
            self.table_state.select(Some(i));
            self.vertical_scroll = self.vertical_scroll.position(i);
        }
    }

    pub fn move_to_bottom(&mut self) {
        if self.values.is_empty() {
            self.table_state.select(None);
            self.vertical_scroll = self.vertical_scroll.position(0);
        } else {
            self.table_state.select(Some(self.values.len() - 1));
            self.vertical_scroll = self.vertical_scroll.position(self.values.len() - 1);
        }
    }

    pub fn move_to_top(&mut self) {
        if self.values.is_empty() {
            self.table_state.select(None);
            self.vertical_scroll = self.vertical_scroll.position(0);
        } else {
            self.table_state.select(Some(0));
            self.vertical_scroll = self.vertical_scroll.position(0);
        }
    }

    pub fn values(&self) -> &Vec<V> {
        &self.values
    }

    /// Replace the rows, maintaining the selection invariant: an empty list has no
    /// selection (`None`); a non-empty list always has a valid `Some(idx)` (clamped
    /// to the last row, defaulting to the first when previously unselected).
    pub fn set_values(&mut self, values: Vec<V>) {
        self.values = values;
        if self.values.is_empty() {
            self.table_state.select(None);
            self.vertical_scroll = self.vertical_scroll.position(0);
        } else {
            let i = std::cmp::min(
                self.table_state.selected().unwrap_or(0),
                self.values.len() - 1,
            );
            self.table_state.select(Some(i));
            self.vertical_scroll = self.vertical_scroll.position(i);
        }
    }

    /// Select a row by index directly, without depending on `values` being
    /// populated. Callers pass an index known to be valid for the rows that will be
    /// rendered; [`set_values`](Self::set_values) clamps as a safety net.
    pub fn select_index(&mut self, idx: usize) {
        if self.values.is_empty() {
            self.table_state.select(None);
            self.vertical_scroll = self.vertical_scroll.position(0);
            return;
        }
        let idx = idx.min(self.values.len() - 1);
        self.table_state.select(Some(idx));
        self.vertical_scroll = self.vertical_scroll.position(idx);
    }

    pub fn move_right(&mut self) {
        self.horizontal_scroll = std::cmp::min(
            std::cmp::max(self.total_width, self.visible_width) - self.visible_width,
            self.horizontal_scroll + 3,
        );
    }

    pub fn move_to_right(&mut self) {
        self.horizontal_scroll =
            std::cmp::max(self.total_width, self.visible_width) - self.visible_width;
    }

    pub fn move_left(&mut self) {
        self.horizontal_scroll = std::cmp::max(3, self.horizontal_scroll) - 3;
    }

    pub fn move_to_left(&mut self) {
        self.horizontal_scroll = 0;
    }
}

impl<V, const N: usize> HandleEvents for TableState<V, N>
where
    V: TableEntry<N>,
{
    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        match (modifiers, code) {
            (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
                self.move_down();
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
                self.move_up();
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char('h')) | (KeyModifiers::NONE, KeyCode::Left) => {
                self.move_left();
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char('l')) | (KeyModifiers::NONE, KeyCode::Right) => {
                self.move_right();
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::End) => {
                self.move_to_right();
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Home) => {
                self.move_to_left();
                EventResult::Consumed
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char('G')) => {
                self.move_to_bottom();
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char('g')) => {
                self.move_to_top();
                EventResult::Consumed
            }
            (KeyModifiers::NONE, KeyCode::Char('0')) => {
                self.move_to_left();
                EventResult::Consumed
            }
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char('$')) => {
                self.move_to_right();
                EventResult::Consumed
            }
            _ => EventResult::Unhandled(modifiers, code),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate as ferrowl_ui;
    use ferrowl_ui_derive::TableEntry;

    #[derive(Clone, Default, Debug, PartialEq, TableEntry)]
    struct Row {
        #[column(name = "Value", min = 1, max = 100)]
        value: String,
    }

    fn table(n: usize) -> TableState<Row, 1> {
        let values = (0..n)
            .map(|i| Row {
                value: i.to_string(),
            })
            .collect::<Vec<_>>();
        TableStateBuilder::default().values(values).build().unwrap()
    }

    fn selected(s: &TableState<Row, 1>) -> Option<usize> {
        s.table_state().selected()
    }

    #[test]
    /// UI-R-013 — a table starts with its first row selected.
    fn starts_with_first_row_selected() {
        assert_eq!(selected(&table(3)), Some(0));
    }

    #[test]
    /// UI-R-013 — j/Down moves the row selection down and clamps at the last row (no wrap).
    fn move_down_advances_and_clamps_at_bottom() {
        let mut s = table(3);
        s.move_down();
        assert_eq!(selected(&s), Some(1));
        s.move_down();
        assert_eq!(selected(&s), Some(2));
        s.move_down(); // already at last row, stays
        assert_eq!(selected(&s), Some(2));
    }

    #[test]
    /// UI-R-013 — k/Up moves the row selection up and clamps at the first row.
    fn move_up_retreats_and_clamps_at_top() {
        let mut s = table(3);
        s.move_to_bottom();
        assert_eq!(selected(&s), Some(2));
        s.move_up();
        assert_eq!(selected(&s), Some(1));
        s.move_up();
        assert_eq!(selected(&s), Some(0));
        s.move_up(); // already at top, stays
        assert_eq!(selected(&s), Some(0));
    }

    #[test]
    /// UI-R-013 — g jumps to the first row and G to the last.
    fn move_to_top_and_bottom_jump() {
        let mut s = table(5);
        s.move_to_bottom();
        assert_eq!(selected(&s), Some(4));
        s.move_to_top();
        assert_eq!(selected(&s), Some(0));
    }

    #[test]
    /// UI-R-013 — table navigation is a safe no-op on an empty table.
    fn empty_table_navigation_selects_none_without_panic() {
        let mut s = table(0);
        s.move_down();
        assert_eq!(selected(&s), None);
        s.move_up();
        assert_eq!(selected(&s), None);
        s.move_to_bottom();
        assert_eq!(selected(&s), None);
        s.move_to_top();
        assert_eq!(selected(&s), None);
    }

    #[test]
    /// UI-R-013 — navigation on a single-row table stays put.
    fn single_row_navigation_stays_put() {
        let mut s = table(1);
        s.move_down();
        assert_eq!(selected(&s), Some(0));
        s.move_up();
        assert_eq!(selected(&s), Some(0));
    }

    #[test]
    /// UI-R-013 — column/horizontal movement clamps at the left edge.
    fn horizontal_scroll_does_not_underflow_at_left_edge() {
        let mut s = table(3);
        s.move_left();
        assert_eq!(s.horizontal_scroll(), 0);
        // With zero measured widths there is nothing to scroll into.
        s.move_right();
        assert_eq!(s.horizontal_scroll(), 0);
    }

    #[test]
    /// UI-R-013 — vim keys dispatch table row/column navigation.
    fn handle_events_dispatches_vim_keys() {
        let mut s = table(4);
        s.handle_events(KeyModifiers::NONE, KeyCode::Char('j'));
        assert_eq!(selected(&s), Some(1));
        s.handle_events(KeyModifiers::NONE, KeyCode::Char('G'));
        assert_eq!(selected(&s), Some(3));
        s.handle_events(KeyModifiers::NONE, KeyCode::Char('k'));
        assert_eq!(selected(&s), Some(2));
        s.handle_events(KeyModifiers::NONE, KeyCode::Char('g'));
        assert_eq!(selected(&s), Some(0));
    }

    #[test]
    /// UI-R-013 — replacing the rows selects the first row when nothing was selected.
    fn set_values_selects_first_row_when_previously_unselected() {
        let mut s = table(0);
        s.move_down(); // empty -> selection becomes None
        assert_eq!(selected(&s), None);
        s.set_values(vec![Row { value: "a".into() }, Row { value: "b".into() }]);
        assert_eq!(selected(&s), Some(0));
    }

    #[test]
    /// UI-R-013 — the selection clamps into range when the row list shrinks.
    fn set_values_clamps_selection_when_list_shrinks() {
        let mut s = table(5);
        s.move_to_bottom();
        assert_eq!(selected(&s), Some(4));
        s.set_values(vec![
            Row { value: "a".into() },
            Row { value: "b".into() },
            Row { value: "c".into() },
        ]);
        assert_eq!(selected(&s), Some(2));
    }

    #[test]
    /// UI-R-013 — emptying the rows clears the selection.
    fn set_values_clears_selection_when_emptied() {
        let mut s = table(3);
        s.set_values(Vec::new());
        assert_eq!(selected(&s), None);
    }

    #[test]
    /// UI-R-013 — replacing the rows keeps an existing in-range selection.
    fn set_values_keeps_existing_in_range_selection() {
        let mut s = table(5);
        s.move_down();
        s.move_down();
        assert_eq!(selected(&s), Some(2));
        s.set_values(
            (0..5)
                .map(|i| Row {
                    value: format!("x{i}"),
                })
                .collect(),
        );
        assert_eq!(selected(&s), Some(2));
    }

    #[test]
    /// UI-R-013 — a row can be selected directly by index.
    fn select_index_sets_selection_directly() {
        let mut s = table(4);
        s.select_index(3);
        assert_eq!(selected(&s), Some(3));
    }

    #[test]
    /// UI-R-047 — an unhandled table key returns Unhandled so it propagates to the enclosing layer.
    fn handle_events_returns_unhandled_for_unknown_key() {
        let mut s = table(2);
        let r = s.handle_events(KeyModifiers::NONE, KeyCode::Char('z'));
        assert!(matches!(r, EventResult::Unhandled(..)));
    }
}
