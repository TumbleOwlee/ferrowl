use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters, WithSetters};

use ratatui::buffer::Buffer;
use ratatui::layout::{Margin, Rect};
use ratatui::text::Text;
use ratatui::widgets::{Clear, StatefulWidget, Widget};

use crate::Border;
use crate::state::SuggestInputState;
use crate::style::SuggestInputStyle;
use crate::traits::{Margins, SuggestionProvider};
use crate::widgets::{InputField, InputFieldBuilder, Validate};

/// A single-line text input (an inner [`InputField`]) with a
/// [`SuggestionProvider`]-backed completion popup, rendered through
/// [`SuggestInputState`].
///
/// `StatefulWidget::render` only draws the field itself; call
/// [`render_overlay`](SuggestInput::render_overlay) separately, after every
/// sibling widget in the frame has been rendered, to draw the popup on top
/// (see that method's doc comment for why).
#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters, WithSetters)]
#[getset(set = "pub")]
pub struct SuggestInput<ValueType, P>
where
    ValueType: Validate,
    P: SuggestionProvider + Clone,
{
    #[getset(get = "pub")]
    #[builder(
        default = "InputFieldBuilder::default().build().expect(\"InputFieldBuilder fields all default\")"
    )]
    input_field: InputField<ValueType>,
    #[getset(get = "pub")]
    #[builder(default = "SuggestInputStyle::default()")]
    popup_style: SuggestInputStyle,
    /// Maximum number of suggestion rows shown at once before the popup
    /// scrolls (via the inner `SelectionState`'s own windowing).
    #[getset(get_copy = "pub")]
    #[builder(default = "8")]
    max_rows: u16,
    #[builder(setter(skip))]
    #[builder(default = "std::marker::PhantomData")]
    marker: std::marker::PhantomData<P>,
}

impl<ValueType, P> Margins for SuggestInput<ValueType, P>
where
    ValueType: Validate,
    P: SuggestionProvider + Clone,
{
    fn margins(&self) -> Margin {
        self.input_field.margins()
    }
}

impl<ValueType, P> StatefulWidget for SuggestInput<ValueType, P>
where
    ValueType: Validate,
    P: SuggestionProvider + Clone,
{
    type State = SuggestInputState<P>;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        StatefulWidget::render(&self, area, buf, state);
    }
}

impl<ValueType, P> StatefulWidget for &SuggestInput<ValueType, P>
where
    ValueType: Validate,
    P: SuggestionProvider + Clone,
{
    type State = SuggestInputState<P>;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        StatefulWidget::render(&self.input_field, area, buf, state.field_mut());
        state.set_anchor(Some(area));
    }
}

impl<ValueType, P> SuggestInput<ValueType, P>
where
    ValueType: Validate,
    P: SuggestionProvider + Clone,
{
    /// Draws the suggestion popup over `bounds`, if `state`'s popup is open.
    ///
    /// Must be called *after* every sibling widget for the current frame has
    /// been rendered: ratatui's `Buffer` uses a painter's-algorithm model
    /// (later renders overwrite earlier ones), so a popup drawn before
    /// sibling widgets would immediately be painted over by them. Pass the
    /// frame/screen area (or whatever region the popup should be clamped
    /// into) as `bounds`.
    ///
    /// Never panics: bails out on a closed popup, a missing anchor
    /// (the field hasn't been rendered yet), an empty suggestion list, or a
    /// `bounds`/anchor combination too small to fit anything.
    pub fn render_overlay(&self, bounds: Rect, buf: &mut Buffer, state: &mut SuggestInputState<P>) {
        if !state.suggestions_open() || bounds.width == 0 || bounds.height == 0 {
            return;
        }
        let Some(anchor) = state.anchor() else {
            return;
        };
        let row_count = state.list().values().len();
        if row_count == 0 {
            return;
        }

        let rows = std::cmp::min(row_count as u16, self.max_rows);
        let height = std::cmp::min(rows + 2, bounds.height);
        if height < 3 {
            // Not enough room for a border plus a single row.
            return;
        }
        let width = std::cmp::min(std::cmp::max(anchor.width, 1), bounds.width);

        let max_x = bounds.x + bounds.width - width;
        let x = anchor.x.clamp(bounds.x, max_x);

        let below_y = anchor.y + anchor.height;
        let fits_below = below_y + height <= bounds.y + bounds.height;
        let fits_above = anchor.y >= bounds.y + height;
        let y = if fits_below {
            below_y
        } else if fits_above {
            anchor.y - height
        } else {
            // Neither direction has room; clamp into the bottom of bounds
            // rather than refuse to draw at all.
            bounds.y + bounds.height - height
        };

        let popup = Rect {
            x,
            y,
            width,
            height,
        }
        .intersection(bounds);
        if popup.width < 2 || popup.height < 3 {
            return;
        }

        Clear.render(popup, buf);
        buf.set_style(popup, self.popup_style.general);
        let inner = crate::widgets::render_border(
            popup,
            buf,
            &Border::Full(Margin::new(0, 0)),
            None,
            self.popup_style.border,
        );

        let labels = state.list().values().clone();
        let selected = state.list().selection();
        let visible = std::cmp::min(inner.height as usize, labels.len());
        // Scroll the window so the selected row stays visible.
        let offset = (selected + 1).saturating_sub(visible);
        for (i, label) in labels.into_iter().skip(offset).take(visible).enumerate() {
            let style = if i + offset == selected {
                self.popup_style.selected
            } else {
                self.popup_style.general
            };
            let row = Rect {
                x: inner.x,
                y: inner.y + i as u16,
                width: inner.width,
                height: 1,
            };
            Text::from(label).style(style).render(row, buf);
        }
    }
}
