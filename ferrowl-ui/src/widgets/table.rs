use std::marker::PhantomData;

use derive_builder::Builder;
use getset::{CopyGetters, Getters, Setters, WithSetters};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint as LConstraint, Layout, Margin, Rect},
    text::{Line, Text},
    widgets::{
        Cell, HighlightSpacing, Row, Scrollbar, ScrollbarOrientation, StatefulWidget,
        Table as UiTable, Widget,
    },
};

use crate::{
    state::{TableState, TableStateBuilder},
    style::TableStyle,
    Border,
    widgets::Title,
};

/// Minimum and maximum width of a table column, in characters.
pub struct Width {
    pub min: usize,
    pub max: usize,
}

/// Static description of a table's `N` columns: header labels and widths.
pub trait Header<const N: usize> {
    fn header() -> [String; N];
    fn widths() -> [Width; N];
}

/// A row that can be displayed in an `N`-column [`Table`].
pub trait TableEntry<const N: usize> {
    /// The cell text for each column.
    fn values(&self) -> [String; N];
    /// The row height in lines.
    fn height(&self) -> u16;
}

/// A scrollable table of [`TableEntry`] rows with an `N`-column [`Header`],
/// rendered from a [`TableState`](crate::state::TableState). Configure
/// border, title, margins, and [`TableStyle`] via [`TableBuilder`].
#[derive(Builder, Debug, Clone, Getters, Setters, CopyGetters, WithSetters)]
#[getset(set = "pub")]
pub struct Table<V, H, const N: usize>
where
    V: TableEntry<N>,
    H: Header<N>,
{
    #[getset(get = "pub")]
    #[builder(default = "None")]
    title: Option<Title>,
    #[getset(get = "pub")]
    #[builder(default = "Border::None")]
    border: Border,
    #[getset(get = "pub")]
    #[builder(default = "TableStyle::default()")]
    style: TableStyle,
    #[getset(get = "pub")]
    #[builder(default = "Margin::default()")]
    margin: Margin,
    #[getset(get = "pub")]
    #[builder(default = "Margin::default()")]
    row_margin: Margin,
    #[getset(get = "pub")]
    #[builder(default = "[true; N]")]
    split_by_whitespace: [bool; N],
    #[builder(setter(skip))]
    #[builder(default = "PhantomData")]
    marker: PhantomData<V>,
    #[builder(setter(skip))]
    #[builder(default = "PhantomData")]
    header: PhantomData<H>,
}

impl<V, H, const N: usize> Widget for Table<V, H, N>
where
    V: TableEntry<N> + Clone,
    H: Header<N>,
{
    fn render(self, area: Rect, buf: &mut Buffer) {
        Widget::render(&self, area, buf);
    }
}

impl<V, H, const N: usize> Widget for &Table<V, H, N>
where
    V: TableEntry<N> + Clone,
    H: Header<N>,
{
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut state = TableStateBuilder::default().build().unwrap();
        StatefulWidget::render(self, area, buf, &mut state);
    }
}

impl<V, H, const N: usize> StatefulWidget for Table<V, H, N>
where
    V: TableEntry<N>,
    H: Header<N>,
{
    type State = TableState<V, N>;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        StatefulWidget::render(&self, area, buf, state);
    }
}

impl<V, H, const N: usize> StatefulWidget for &Table<V, H, N>
where
    V: TableEntry<N>,
    H: Header<N>,
{
    type State = TableState<V, N>;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let area = Layout::vertical([
            LConstraint::Length(self.margin.vertical),
            LConstraint::Min(2),
            LConstraint::Length(self.margin.vertical),
        ])
        .split(area)[1];

        let area = Layout::horizontal([
            LConstraint::Length(self.margin.horizontal),
            LConstraint::Min(1),
            LConstraint::Length(self.margin.horizontal),
        ])
        .split(area)[1];

        // Create block if border is required
        let border_style = if state.focused() {
            self.style.border
        } else {
            self.style.general
        };
        let area = crate::widgets::render_border(
            area,
            buf,
            &self.border,
            self.title.as_ref(),
            border_style,
        );

        let header = H::header();
        let header = header
            .iter()
            .map(|c| Cell::from(c.as_str()))
            .collect::<Row>()
            .style(self.style.header)
            .height(1);

        // Count chars per column for maximal width
        let column_title_len: [usize; N] = H::header()
            .iter()
            .map(|h| h.chars().count())
            .collect::<Vec<usize>>()
            .try_into()
            .unwrap();
        let column_max_widths = state
            .values()
            .iter()
            .fold(column_title_len, |mut init, item| {
                let values = item.values();
                for i in 0..N {
                    init[i] = std::cmp::max(init[i], values[i].chars().count());
                }
                init
            });

        // Get real column width respecting input constraints
        let column_widths: Vec<u16> = H::widths()
            .iter()
            .zip(column_max_widths.iter())
            .map(|(w, max_w)| {
                let Width { max, min } = w;
                std::cmp::max(*min as u16, std::cmp::min(*max as u16, *max_w as u16))
            })
            .collect();

        let column_spacings = 2 * (N as u16) + (N as u16 - 1);
        let table_width = column_widths.iter().fold(column_spacings, |acc, w| acc + w) + 3;

        let selected_style = if state.focused() {
            &self.style.focused
        } else {
            &self.style.unfocused_selected
        };
        let bar_style = selected_style;
        let mut bar_height = 0;

        state.set_total_width(std::cmp::max(table_width, area.width));

        let rows = state.values().iter().enumerate().map(|(i, item)| {
            let color = self.style.rows.get(i % 2).unwrap();
            let spacing =
                itertools::repeat_n('\n', self.row_margin.vertical as usize).collect::<String>();
            let mut max_line_cnt = 0;
            let row = item
                .values()
                .iter()
                .zip(&column_widths)
                .enumerate()
                .map(|(col, (content, width))| {
                    let mut line_cnt = 0;
                    let mut line = String::with_capacity(*width as usize);
                    let mut output = String::with_capacity(
                        content.chars().count() + (content.chars().count() / *width as usize) + 1,
                    );
                    if self.split_by_whitespace[col] {
                        for s in content.split_whitespace() {
                            if line.chars().count() + s.chars().count() < *width as usize {
                                if !line.is_empty() {
                                    line += " ";
                                }
                                line += s;
                            } else {
                                if !line.is_empty() {
                                    if output.is_empty() {
                                        output += &line.to_string();
                                    } else {
                                        output += &format!("\n{line}");
                                    }
                                    line_cnt += 1;
                                }
                                line.clear();
                                let mut s = s.to_owned();
                                while s.chars().count() > *width as usize {
                                    output += &format!(
                                        "{}{}",
                                        if output.is_empty() { "" } else { "\n" },
                                        s.chars().take(*width as usize).collect::<String>()
                                    );
                                    s = s.chars().skip(*width as usize).collect::<String>();
                                    line_cnt += 1;
                                }
                                line += &s;
                            }
                        }
                        if !line.is_empty() {
                            let mut s = line;
                            while s.chars().count() > *width as usize {
                                output += &format!(
                                    "{}{}",
                                    if output.is_empty() { "" } else { "\n" },
                                    s.chars().take(*width as usize).collect::<String>()
                                );
                                s = s.chars().skip(*width as usize).collect::<String>();
                                line_cnt += 1;
                            }
                            output +=
                                &format!("{}{}", if output.is_empty() { "" } else { "\n" }, s);
                            line_cnt += 1;
                        }
                    } else {
                        let mut s = content.clone();
                        while s.chars().count() > *width as usize {
                            output += &format!(
                                "{}{}",
                                if output.is_empty() { "" } else { "\n" },
                                s.chars().take(*width as usize).collect::<String>()
                            );
                            s = s.chars().skip(*width as usize).collect::<String>();
                            line_cnt += 1;
                        }
                        output += &format!("{}{}", if output.is_empty() { "" } else { "\n" }, s);
                        line_cnt += 1;
                    }
                    max_line_cnt = std::cmp::max(line_cnt, max_line_cnt);
                    Cell::from(Text::from(format!("{spacing}{output}{spacing}")))
                })
                .collect::<Row>()
                .style(*color)
                .height(self.row_margin.vertical * 2 + max_line_cnt);
            if state.table_state().selected() == Some(i) {
                bar_height = max_line_cnt;
            }
            row
        });

        let constraints = H::widths()
            .iter()
            .zip(column_widths.iter())
            .map(|(w, c)| {
                if w.max > *c as usize {
                    LConstraint::Length(*c + 2)
                } else {
                    LConstraint::Length(w.max as u16 + 2)
                }
            })
            .collect::<Vec<_>>();

        let bar = " █ ";
        let t = UiTable::new(rows, constraints)
            .header(header)
            .row_highlight_style(*selected_style)
            .highlight_symbol({
                Text::from({
                    let spacing = itertools::repeat_n("".into(), self.row_margin.vertical as usize);
                    spacing
                        .clone()
                        .chain(itertools::repeat_n(bar.into(), bar_height as usize))
                        .chain(spacing)
                        .collect::<Vec<Line>>()
                })
                .style(*bar_style)
            })
            .column_spacing(1)
            .highlight_spacing(HighlightSpacing::Always);

        state.set_visible_width(area.width);
        if state.total_width() <= area.width {
            StatefulWidget::render(t, area, buf, &mut state.table_state());
        } else {
            let rect = Rect {
                x: 0,
                y: 0,
                width: state.total_width(),
                height: area.height,
            };

            let mut buffer = Buffer::empty(rect);
            ratatui::widgets::StatefulWidget::render(
                t,
                rect,
                &mut buffer,
                &mut state.table_state(),
            );
            let offset = std::cmp::min(state.total_width() - area.width, state.horizontal_scroll());

            for (x, y) in itertools::iproduct!(offset..(offset + area.width), 0..(rect.height)) {
                buf[(x - offset + area.x, y + area.y)].clone_from(&buffer[(x, y)]);
            }
        }
        ratatui::widgets::StatefulWidget::render(
            Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None),
            area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            buf,
            &mut state.vertical_scroll(),
        );
    }
}
