// Example binary: unwrap keeps the demo focused on the widget being shown.
#![allow(clippy::unwrap_used)]

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ferrowl_ui::{
    AlternateScreen, Border,
    state::{MarkdownInputFieldState, MarkdownInputFieldStateBuilder, VimMode},
    traits::{HandleEvents, SetFocus},
    widgets::{MarkdownInputField, MarkdownInputFieldBuilder},
};
use ratatui::{Frame, layout::Margin};
use std::{io::Stdout, time::Duration};

const DOCUMENT: &str = "\
# Heading 1
## Heading 2
### Heading 3
#### Heading 4
##### Heading 5
###### Heading 6

- an unordered item
  - nested one level
1. an ordered item
- [ ] an open task
- [x] a done task

> a quote
>> a nested quote

---

```lua
local function greet(name)
    print('hello, ' .. name)
end
```

```json
{\"key\": \"value\", \"n\": 1}
```

```
a fence with no info string
```

Inline **bold**, *italic*, `inline code`, ~~strike~~, [a link](https://example.com) and
![an image](https://example.com/image.png).

This paragraph is deliberately much wider than the terminal so it wraps across several
display rows, exercising the hanging indent under its own left margin.

andthiswordisdeliberatelylongerthananyreasonablewidthsothewrapperhastobreakitmid-word
";

struct App {
    field: MarkdownInputField,
    state: MarkdownInputFieldState,
}

impl Default for App {
    fn default() -> Self {
        let mut state = MarkdownInputFieldStateBuilder::default().build().unwrap();
        state.set_content(DOCUMENT);
        state.set_focused(true);
        let field = MarkdownInputFieldBuilder::default()
            .border(Border::Full(Margin::new(1, 0)))
            .build()
            .unwrap();
        Self { field, state }
    }
}

fn ui(f: &mut Frame, app: &mut App) {
    f.render_stateful_widget(&app.field, f.area(), &mut app.state);
}

fn main() {
    let mut screen: AlternateScreen<Stdout> =
        AlternateScreen::new().expect("Failed to create alternate screen.");

    let mut app = App::default();

    loop {
        screen.draw(|f| ui(f, &mut app)).unwrap();

        if event::poll(Duration::from_millis(50)).unwrap()
            && let Event::Key(key) = event::read().unwrap()
            && key.kind == KeyEventKind::Press
        {
            // The demo shortcuts only intercept Normal mode: Insert and Visual must see
            // every key so `q`, `r` and `n` can still be typed into the document.
            match (app.state.vim_mode(), key.modifiers, key.code) {
                (VimMode::Normal, KeyModifiers::NONE, KeyCode::Char('q')) => break,
                (VimMode::Normal, KeyModifiers::NONE, KeyCode::Char('r')) => {
                    let ro = app.state.read_only();
                    app.state.set_read_only(!ro);
                }
                (VimMode::Normal, KeyModifiers::NONE, KeyCode::Char('n')) => {
                    let numbers = app.field.line_numbers();
                    app.field.set_line_numbers(!numbers);
                }
                (_, modifiers, code) => {
                    app.state.handle_events(modifiers, code);
                }
            }
        }
    }
}
