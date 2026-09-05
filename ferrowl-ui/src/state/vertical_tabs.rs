use crate::traits::ToLabel;

pub struct VerticalTabsState<T: ToLabel> {
    pub titles: Vec<T>,
    pub active: usize,
    /// The first visible row of the stacked titles, counted in character/padding
    /// rows across the whole tab list, not in tab indices.
    pub offset: usize,
}
