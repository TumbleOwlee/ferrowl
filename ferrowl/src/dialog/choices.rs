//! Selection value-types shared by the modbus, monitor and ocpp setup dialogs: the dialog
//! mode, serial parity, auto-reconnect and numeric-serial choices, each rendered via
//! [`ToLabel`], plus their config-string mappings.

use ferrowl_ui::state::SelectionState;
use ferrowl_ui::traits::ToLabel;

/// Edit an existing instance, or create a new module (with an optional config path).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DialogMode {
    Edit,
    New,
}

/// Serial parity selection value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Parity {
    None,
    Odd,
    Even,
}

impl ToLabel for Parity {
    fn to_label(&self) -> String {
        match self {
            Parity::None => "None",
            Parity::Odd => "Odd",
            Parity::Even => "Even",
        }
        .to_string()
    }
}

impl Parity {
    /// Map to the `Endpoint`/`rtu::Config` representation (`None` = no parity).
    pub(crate) fn to_config(&self) -> Option<String> {
        match self {
            Parity::None => None,
            Parity::Odd => Some("odd".to_string()),
            Parity::Even => Some("even".to_string()),
        }
    }

    pub(crate) fn from_config(value: Option<&str>) -> Parity {
        match value.map(str::to_ascii_lowercase).as_deref() {
            Some("odd") => Parity::Odd,
            Some("even") => Parity::Even,
            _ => Parity::None,
        }
    }

    pub(crate) fn index(&self) -> usize {
        match self {
            Parity::None => 0,
            Parity::Odd => 1,
            Parity::Even => 2,
        }
    }
}

/// Auto-reconnect toggle: client redial (OC-R-048, OC-R-107) or server bind retry
/// (OC-R-083, OC-R-108–109), reused verbatim for the monitor's serial-open retry (MB-R-141).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReconnectChoice {
    On,
    Off,
}

impl ToLabel for ReconnectChoice {
    fn to_label(&self) -> String {
        match self {
            ReconnectChoice::On => "On",
            ReconnectChoice::Off => "Off",
        }
        .to_string()
    }
}

/// A numeric serial choice (data/stop bits) rendered as a selection label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct U8Choice(pub u8);

impl ToLabel for U8Choice {
    fn to_label(&self) -> String {
        self.0.to_string()
    }
}

/// Select the entry matching `current` (if present) in a numeric choice selection.
pub(crate) fn select_u8(state: &mut SelectionState<U8Choice>, current: Option<u8>) {
    if let Some(value) = current
        && let Some(index) = state.values().iter().position(|c| c.0 == value)
    {
        state.set_selection(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_labels() {
        assert_eq!(Parity::Even.to_label(), "Even");
        assert_eq!(ReconnectChoice::On.to_label(), "On");
        assert_eq!(U8Choice(8).to_label(), "8");
    }

    #[test]
    fn ut_parity_config_round_trip_and_index() {
        assert_eq!(Parity::None.to_config(), None);
        assert_eq!(Parity::Odd.to_config().as_deref(), Some("odd"));
        assert_eq!(Parity::Even.to_config().as_deref(), Some("even"));
        // from_config is case-insensitive; anything unrecognized (or absent) is None parity.
        assert_eq!(Parity::from_config(Some("ODD")), Parity::Odd);
        assert_eq!(Parity::from_config(Some("even")), Parity::Even);
        assert_eq!(Parity::from_config(Some("weird")), Parity::None);
        assert_eq!(Parity::from_config(None), Parity::None);
        assert_eq!(Parity::None.index(), 0);
        assert_eq!(Parity::Odd.index(), 1);
        assert_eq!(Parity::Even.index(), 2);
    }
}
