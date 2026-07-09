use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters, WithSetters};

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::text::Text;
use ratatui::widgets::Widget;
use ratatui::widgets::{Paragraph, StatefulWidget};
use std::marker::PhantomData;

use crate::Border;
use crate::state::InputFieldState;
use crate::style::InputFieldStyle;
use crate::traits::Margins;
use crate::widgets::Title;

pub enum ValidateResult {
    Success,
    None,
    Error(String),
}

/// Validates raw input text for an [`InputField`].
///
/// Implemented for `String` (always valid) and all numeric primitives
/// (valid if the text parses as that type); invalid input is rendered with
/// the field's error style.
pub trait Validate {
    /// Returns `Err` with a message if `input` is not a valid value.
    fn validate(input: &str) -> ValidateResult;

    /// Whether `c` may be typed into the field. Char-level only: cannot
    /// guarantee overall validity (e.g. "1.2.3" for a float); `validate`
    /// still styles invalid text. Default allows every character.
    fn allowed_char(_c: char) -> bool {
        true
    }
}

impl Validate for String {
    fn validate(_input: &str) -> ValidateResult {
        ValidateResult::None
    }
}

// `allowed_char` is char-level filtering only, so "inf"/"NaN"/leading '+' for
// ints become untypeable even though `parse` accepts them — deliberate.
macro_rules! generate_validate {
    ($v:ty, $allowed:expr) => {
        impl Validate for $v {
            fn validate(input: &str) -> ValidateResult {
                let result = input.parse::<$v>();
                match result {
                    Ok(_) => ValidateResult::None,
                    Err(e) => ValidateResult::Error(format!("{}", e)),
                }
            }

            fn allowed_char(c: char) -> bool {
                let f: fn(char) -> bool = $allowed;
                f(c)
            }
        }
    };
}

generate_validate!(usize, |c: char| c.is_ascii_digit());
generate_validate!(u8, |c: char| c.is_ascii_digit());
generate_validate!(u16, |c: char| c.is_ascii_digit());
generate_validate!(u32, |c: char| c.is_ascii_digit());
generate_validate!(u64, |c: char| c.is_ascii_digit());
generate_validate!(u128, |c: char| c.is_ascii_digit());
generate_validate!(i8, |c: char| c.is_ascii_digit() || c == '-');
generate_validate!(i16, |c: char| c.is_ascii_digit() || c == '-');
generate_validate!(i32, |c: char| c.is_ascii_digit() || c == '-');
generate_validate!(i64, |c: char| c.is_ascii_digit() || c == '-');
generate_validate!(i128, |c: char| c.is_ascii_digit() || c == '-');
generate_validate!(f32, |c: char| c.is_ascii_digit()
    || matches!(c, '.' | '-' | '+' | 'e' | 'E'));
generate_validate!(f64, |c: char| c.is_ascii_digit()
    || matches!(c, '.' | '-' | '+' | 'e' | 'E'));

/// A single-line text input rendered from an
/// [`InputFieldState`](crate::state::InputFieldState), typed by the
/// [`Validate`] impl of `ValueType` so invalid text is styled as an error.
/// Configure border, title, margins, and [`InputFieldStyle`] via
/// [`InputFieldBuilder`].
#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters, WithSetters)]
#[getset(set = "pub")]
pub struct InputField<ValueType>
where
    ValueType: Validate,
{
    #[getset(get = "pub")]
    #[builder(default = "Border::None")]
    border: Border,
    #[getset(get = "pub")]
    #[builder(default = "InputFieldStyle::default()")]
    style: InputFieldStyle,
    #[getset(get = "pub")]
    #[builder(default = "None")]
    title: Option<Title>,
    #[getset(get = "pub")]
    #[builder(default = "Margin::default()")]
    margin: Margin,
    #[getset(get = "pub")]
    #[builder(default = "false")]
    multiline: bool,
    #[builder(setter(skip))]
    #[builder(default = "PhantomData")]
    marker: PhantomData<ValueType>,
}

impl<ValueType> Margins for InputField<ValueType>
where
    ValueType: Validate,
{
    fn margins(&self) -> Margin {
        let horizontal = if let Border::Full(margin) = &self.border {
            4 + margin.horizontal * 2
        } else {
            0
        } + 2 * self.margin.horizontal
            + 1;
        let vertical = if let Border::Full(margin) = &self.border {
            2 + margin.vertical * 2
        } else if self.title.is_some() {
            1
        } else {
            0
        } + self.margin.vertical;
        Margin {
            horizontal,
            vertical,
        }
    }
}

impl<ValueType> Widget for InputField<ValueType>
where
    ValueType: Validate,
{
    fn render(self, area: Rect, buf: &mut Buffer) {
        Widget::render(&self, area, buf);
    }
}

impl<ValueType> Widget for &InputField<ValueType>
where
    ValueType: Validate,
{
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut state = InputFieldState::default();
        StatefulWidget::render(self, area, buf, &mut state);
    }
}

impl<ValueType> StatefulWidget for InputField<ValueType>
where
    ValueType: Validate,
{
    type State = InputFieldState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        StatefulWidget::render(&self, area, buf, state);
    }
}

impl<ValueType> StatefulWidget for &InputField<ValueType>
where
    ValueType: Validate,
{
    type State = InputFieldState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        buf.set_style(area, self.style.general);

        let mut height = if let Border::Full(margin) = &self.border {
            2 + margin.vertical * 2
        } else {
            0
        };
        if self.multiline {
            height += std::cmp::max(1, area.height);
        } else {
            height += 1;
        }

        let area = Layout::vertical([
            Constraint::Length(self.margin.vertical),
            Constraint::Length(height),
            Constraint::Length(self.margin.vertical),
        ])
        .split(area)[1];

        let area = Layout::horizontal([
            Constraint::Length(self.margin.horizontal),
            Constraint::Min(1),
            Constraint::Length(self.margin.horizontal),
        ])
        .split(area)[1];

        let valid = ValueType::validate(state.input());

        // Create block if border is required
        let border_style = if state.focused() && !state.disabled() {
            match valid {
                ValidateResult::Success | ValidateResult::None => self.style.focused,
                ValidateResult::Error(_) => self.style.error,
            }
        } else {
            match valid {
                ValidateResult::Success | ValidateResult::None => self.style.border,
                ValidateResult::Error(_) => self.style.error,
            }
        };
        let area = crate::widgets::render_border(
            area,
            buf,
            &self.border,
            self.title.as_ref(),
            border_style,
        );

        let input = state.input();
        let mut text = if input.is_empty() {
            state
                .autofill()
                .clone()
                .or(state.placeholder().clone())
                .unwrap_or("Enter value..".to_string())
        } else {
            input.clone()
        };

        let mut x_start = 0;
        let cursor = state.cursor();
        let text_len = text.chars().count();

        // Calculate range of text to display
        if area.width > 0 && area.height > 0 && (area.width as usize) <= text_len {
            let total_len = area.width * area.height - 1;
            let width = (total_len / 2) as usize;
            // Display width characters left of cursor
            x_start = std::cmp::max(state.cursor(), width) - width;
            // Display width characters right of cursor
            let mut x_end = std::cmp::min(cursor + width, text_len);
            // Add more characters to the left, if right of cursor are not enough
            if (x_end - cursor) < (total_len as usize - width) {
                let remaining = (total_len as usize - width) - (x_end - cursor);
                x_start = std::cmp::max(x_start, remaining) - remaining;
            }
            // Add more characters to the right, if left of cursor are not enough
            if (cursor - x_start) < width {
                let remaining = width - (cursor - x_start);
                x_end = std::cmp::min(text_len, x_end + remaining);
            }
            // Get displayable text area
            text = text.chars().enumerate().fold(
                String::with_capacity(x_end - x_start),
                |mut s, (i, c)| {
                    if i >= x_start && i < x_end {
                        s.push(c);
                    }
                    s
                },
            );
        }

        let text_style = if state.input().is_empty() {
            self.style.placeholder
        } else {
            match valid {
                ValidateResult::Success => self.style.success,
                ValidateResult::None => self.style.general,
                ValidateResult::Error(_) => self.style.error,
            }
        };

        let mut text_area = area;
        let (len, remain) = text
            .chars()
            .fold((0, String::new()), |(mut len, mut line), c| {
                line.push(c);
                len += 1;
                if len >= area.width as usize {
                    let input = Paragraph::new(Text::from(line).style(text_style));
                    input.render(text_area, buf);
                    text_area.y += 1;
                    (0, String::new())
                } else {
                    (len, line)
                }
            });
        if len > 0 {
            let input = Paragraph::new(Text::from(remain).style(text_style));
            input.render(text_area, buf);
        }

        if !state.disabled() {
            // Display cursor
            if state.focused() {
                let pos = (cursor - x_start) as u16;
                let pos_x = pos % area.width;
                let pos_y = pos / area.width;
                buf[(area.x + pos_x, area.y + pos_y)].set_style(self.style.cursor);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u32_allowed_char_accepts_digits_rejects_others() {
        assert!(u32::allowed_char('0'));
        assert!(u32::allowed_char('9'));
        assert!(!u32::allowed_char('a'));
        assert!(!u32::allowed_char('-'));
    }

    #[test]
    fn i32_allowed_char_accepts_minus() {
        assert!(i32::allowed_char('-'));
        assert!(i32::allowed_char('5'));
        assert!(!i32::allowed_char('a'));
    }

    #[test]
    fn f64_allowed_char_accepts_float_chars() {
        assert!(f64::allowed_char('.'));
        assert!(f64::allowed_char('e'));
        assert!(f64::allowed_char('+'));
        assert!(!f64::allowed_char('a'));
    }

    #[test]
    fn string_allowed_char_accepts_anything() {
        assert!(String::allowed_char('a'));
        assert!(String::allowed_char('!'));
    }
}
