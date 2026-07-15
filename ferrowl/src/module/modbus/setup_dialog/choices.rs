//! Selection value-types for the Modbus setup dialog: the transport, parity, reconnect and
//! numeric-serial choices, each rendered via [`ToLabel`], plus their config-string mappings.
//! Separated from the dialog widget/state logic in the parent module.

use ferrowl_ui::traits::ToLabel;

use crate::config::Role;

/// Edit an existing instance, or create a new module (with an optional config path).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogMode {
    Edit,
    New,
}

/// Transport selection value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transport {
    Tcp,
    Rtu,
}

impl ToLabel for Transport {
    fn to_label(&self) -> String {
        match self {
            Transport::Tcp => "TCP",
            Transport::Rtu => "RTU",
        }
        .to_string()
    }
}

impl ToLabel for Role {
    fn to_label(&self) -> String {
        format!("{self}")
    }
}

/// Serial parity selection value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parity {
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
    pub(super) fn to_config(&self) -> Option<String> {
        match self {
            Parity::None => None,
            Parity::Odd => Some("odd".to_string()),
            Parity::Even => Some("even".to_string()),
        }
    }

    pub(super) fn from_config(value: Option<&str>) -> Parity {
        match value.map(|s| s.to_ascii_lowercase()).as_deref() {
            Some("odd") => Parity::Odd,
            Some("even") => Parity::Even,
            _ => Parity::None,
        }
    }

    pub(super) fn index(&self) -> usize {
        match self {
            Parity::None => 0,
            Parity::Odd => 1,
            Parity::Even => 2,
        }
    }
}

/// Client-only auto-reconnect toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconnectChoice {
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
pub struct U8Choice(pub u8);

impl ToLabel for U8Choice {
    fn to_label(&self) -> String {
        self.0.to_string()
    }
}
