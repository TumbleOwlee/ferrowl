use crossterm::event::{KeyCode, KeyModifiers};
use derive_builder::Builder;
use ratatui::layout::Rect;

use crate::EventResult;
use crate::state::{InputFieldState, SelectionState, SelectionStateBuilder};
use crate::traits::{HandleEvents, IsFocus, SetFocus, Suggestion, SuggestionProvider};
use crate::widgets::GetValue;

/// State of a [`SuggestInput`](crate::widgets::SuggestInput): a single-line
/// text field (delegated to an inner [`InputFieldState`]) plus a
/// [`SuggestionProvider`]-backed popup of completions.
///
/// Typing re-queries the provider; the popup opens when it returns matches
/// and closes when it doesn't. While the popup is open, Up/Down navigate it
/// and Enter/Tab accept the highlighted entry (re-querying and staying open
/// for [`Suggestion::partial`] entries); Esc closes it without touching the
/// text. While closed, those keys are left `Unhandled` for the caller.
#[derive(Builder, Debug, Clone)]
pub struct SuggestInputState<P>
where
    P: SuggestionProvider + Clone,
{
    #[builder(default = "InputFieldState::default()")]
    field: InputFieldState,
    provider: P,
    /// Suggestion labels, kept in a `SelectionState` so navigation/scroll
    /// logic can be reused for rendering.
    #[builder(
        default = "SelectionStateBuilder::default().values(Vec::new()).build().unwrap()",
        setter(skip)
    )]
    list: SelectionState<String>,
    /// Full suggestions backing `list`'s labels, in the same order.
    #[builder(default = "Vec::new()", setter(skip))]
    suggestions: Vec<Suggestion>,
    #[builder(default = "false", setter(skip))]
    open: bool,
    /// Area the field itself was last rendered into; the popup anchors
    /// below (or above) it. Set by [`SuggestInput`](crate::widgets::SuggestInput)'s
    /// `StatefulWidget::render`.
    #[builder(default = "None", setter(skip))]
    anchor: Option<Rect>,
}

impl<P> std::ops::Deref for SuggestInputState<P>
where
    P: SuggestionProvider + Clone,
{
    type Target = InputFieldState;

    fn deref(&self) -> &Self::Target {
        &self.field
    }
}

impl<P> std::ops::DerefMut for SuggestInputState<P>
where
    P: SuggestionProvider + Clone,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.field
    }
}

impl<P> SuggestInputState<P>
where
    P: SuggestionProvider + Clone,
{
    /// Mutable access to the inner text-field state, used by
    /// [`SuggestInput`](crate::widgets::SuggestInput) to render the field
    /// through `InputField`'s own `StatefulWidget` impl.
    pub(crate) fn field_mut(&mut self) -> &mut InputFieldState {
        &mut self.field
    }

    /// Whether the suggestion popup is currently open.
    pub fn suggestions_open(&self) -> bool {
        self.open
    }

    /// Labels of the current suggestions, as a `SelectionState` so a widget
    /// can reuse selection/scroll logic to render them.
    pub fn list(&self) -> &SelectionState<String> {
        &self.list
    }

    /// The full suggestions backing `list()`'s labels.
    pub fn suggestions(&self) -> &Vec<Suggestion> {
        &self.suggestions
    }

    /// Area the field was last rendered into, used by the popup to anchor
    /// itself; `None` until the field has been rendered at least once.
    pub fn anchor(&self) -> Option<Rect> {
        self.anchor
    }

    pub fn set_anchor(&mut self, anchor: Option<Rect>) {
        self.anchor = anchor;
    }

    /// Re-queries the provider with the current input text and opens the
    /// popup (list reset to the first entry) if it returns matches, or
    /// closes it otherwise. Called from `handle_events`, never from render.
    fn requery(&mut self) {
        let input = self.field.input().clone();
        let suggestions = self.provider.suggest(&input);
        if suggestions.is_empty() {
            self.close();
            return;
        }
        let labels = suggestions.iter().map(|s| s.label.clone()).collect();
        self.suggestions = suggestions;
        self.list = SelectionStateBuilder::default()
            .values(labels)
            .build()
            .unwrap();
        self.open = true;
    }

    /// Closes the popup without changing the text.
    fn close(&mut self) {
        self.open = false;
    }

    /// Accepts the highlighted suggestion: replaces the input with its
    /// value and moves the cursor to the end. `partial` suggestions
    /// re-query and may keep the popup open; others close it.
    fn accept_selected(&mut self) {
        let idx = self.list.selection();
        let Some(suggestion) = self.suggestions.get(idx).cloned() else {
            self.close();
            return;
        };
        self.field.set_input(suggestion.value);
        self.field.set_cursor(self.field.input().chars().count());
        if suggestion.partial {
            self.requery();
        } else {
            self.close();
        }
    }
}

impl<P> GetValue for SuggestInputState<P>
where
    P: SuggestionProvider + Clone,
{
    type ValueType = String;

    fn get_value(&self) -> Self::ValueType {
        self.field.get_value()
    }
}

impl<P> SetFocus for SuggestInputState<P>
where
    P: SuggestionProvider + Clone,
{
    fn set_focused(&mut self, focus: bool) {
        self.field.set_focused(focus);
        if !focus {
            self.close();
        }
    }
}

impl<P> IsFocus for SuggestInputState<P>
where
    P: SuggestionProvider + Clone,
{
    fn is_focused(&self) -> bool {
        self.field.is_focused()
    }
}

impl<P> HandleEvents for SuggestInputState<P>
where
    P: SuggestionProvider + Clone,
{
    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        if self.open {
            match (modifiers, code) {
                (KeyModifiers::NONE, KeyCode::Up) => {
                    self.list.move_up();
                    return EventResult::Consumed;
                }
                (KeyModifiers::NONE, KeyCode::Down) => {
                    self.list.move_down();
                    return EventResult::Consumed;
                }
                (KeyModifiers::NONE, KeyCode::Enter) | (KeyModifiers::NONE, KeyCode::Tab) => {
                    self.accept_selected();
                    return EventResult::Consumed;
                }
                (KeyModifiers::NONE, KeyCode::Esc) => {
                    self.close();
                    return EventResult::Consumed;
                }
                _ => {}
            }
        } else if matches!(
            (modifiers, code),
            (KeyModifiers::NONE, KeyCode::Up)
                | (KeyModifiers::NONE, KeyCode::Down)
                | (KeyModifiers::NONE, KeyCode::Enter)
                | (KeyModifiers::NONE, KeyCode::Tab)
                | (KeyModifiers::NONE, KeyCode::Esc)
        ) {
            return EventResult::Unhandled(modifiers, code);
        }

        let before = self.field.input().clone();
        let result = self.field.handle_events(modifiers, code);
        if &before != self.field.input() {
            self.requery();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    struct TestProvider;

    impl SuggestionProvider for TestProvider {
        fn suggest(&self, input: &str) -> Vec<Suggestion> {
            match input {
                "a" => vec![
                    Suggestion {
                        value: "apple".to_string(),
                        label: "apple".to_string(),
                        partial: false,
                    },
                    Suggestion {
                        value: "apricot".to_string(),
                        label: "apricot".to_string(),
                        partial: false,
                    },
                ],
                "d" => vec![Suggestion {
                    value: "dir/".to_string(),
                    label: "dir/".to_string(),
                    partial: true,
                }],
                "dir/" => vec![Suggestion {
                    value: "dir/file".to_string(),
                    label: "file".to_string(),
                    partial: false,
                }],
                _ => vec![],
            }
        }
    }

    fn state() -> SuggestInputState<TestProvider> {
        SuggestInputStateBuilder::default()
            .provider(TestProvider)
            .build()
            .unwrap()
    }

    fn type_str(s: &mut SuggestInputState<TestProvider>, text: &str) {
        for c in text.chars() {
            s.handle_events(KeyModifiers::NONE, KeyCode::Char(c));
        }
    }

    #[test]
    fn typing_opens_popup_on_match() {
        let mut s = state();
        type_str(&mut s, "a");
        assert!(s.suggestions_open());
        assert_eq!(s.list().values().len(), 2);
    }

    #[test]
    fn typing_keeps_popup_closed_without_match() {
        let mut s = state();
        type_str(&mut s, "z");
        assert!(!s.suggestions_open());
    }

    #[test]
    fn up_down_wrap_around() {
        let mut s = state();
        type_str(&mut s, "a");
        assert_eq!(s.list().selection(), 0);
        s.handle_events(KeyModifiers::NONE, KeyCode::Up); // wraps to last
        assert_eq!(s.list().selection(), 1);
        s.handle_events(KeyModifiers::NONE, KeyCode::Down); // wraps to first
        assert_eq!(s.list().selection(), 0);
    }

    #[test]
    fn enter_accepts_selected_and_moves_cursor_to_end() {
        let mut s = state();
        type_str(&mut s, "a");
        s.handle_events(KeyModifiers::NONE, KeyCode::Down); // select "apricot"
        let r = s.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert!(matches!(r, EventResult::Consumed));
        assert_eq!(s.input(), "apricot");
        assert_eq!(s.cursor(), 7);
        assert!(!s.suggestions_open());
    }

    #[test]
    fn accepting_partial_suggestion_requeries_and_stays_open() {
        let mut s = state();
        type_str(&mut s, "d");
        assert!(s.suggestions_open());
        s.handle_events(KeyModifiers::NONE, KeyCode::Enter);
        assert_eq!(s.input(), "dir/");
        assert!(s.suggestions_open());
        assert_eq!(s.list().values(), &vec!["file".to_string()]);
    }

    #[test]
    fn esc_closes_popup_only_and_is_consumed_while_open() {
        let mut s = state();
        type_str(&mut s, "a");
        let r = s.handle_events(KeyModifiers::NONE, KeyCode::Esc);
        assert!(matches!(r, EventResult::Consumed));
        assert!(!s.suggestions_open());
        assert_eq!(s.input(), "a");
    }

    #[test]
    fn navigation_keys_unhandled_while_closed() {
        let mut s = state();
        for code in [
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Enter,
            KeyCode::Tab,
            KeyCode::Esc,
        ] {
            let r = s.handle_events(KeyModifiers::NONE, code);
            assert!(matches!(r, EventResult::Unhandled(..)), "{code:?}");
        }
    }

    #[test]
    fn set_focused_false_closes_popup() {
        let mut s = state();
        type_str(&mut s, "a");
        assert!(s.suggestions_open());
        s.set_focused(false);
        assert!(!s.suggestions_open());
        assert!(!s.is_focused());
    }
}
