use crate::traits::ToLabel;

pub struct ScrollingTabsState<T: ToLabel> {
    pub titles: Vec<T>,
    pub selected: usize,
}
