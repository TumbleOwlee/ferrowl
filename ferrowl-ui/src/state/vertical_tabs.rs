use crate::traits::ToLabel;

pub struct VerticalTabsState<T: ToLabel> {
    pub titles: Vec<T>,
    pub active: usize,
    pub offset: usize,
}
