//! Modal dialogs: register editing and module setup.

mod edit;
mod setup;

pub use edit::{EditInputDialog, EditSelectionDialog, EditedRegister, SubDialogs, parse_raw_value};
pub use setup::{SetupDialog, SetupValues};
