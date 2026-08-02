//! Helpers genuinely shared by both client and server, on both transports.

use crate::SerialError;

use rust_modbus::{DataBits, Parity, SerialConfig, StopBits};

/// Build a serial port configuration from the optional serial parameters, validating each.
///
/// An unset parameter is left at the Modbus serial-line default the protocol library
/// declares (8 data bits, even parity, one stop bit — MB-R-072); only the fields the
/// device config states are overridden. The port path is not part of the configuration:
/// it is passed alongside it to `open_serial`.
pub(crate) fn serial_config_from(
    baud_rate: u32,
    data_bits: Option<u8>,
    stop_bits: Option<u8>,
    parity: Option<&str>,
) -> Result<SerialConfig, SerialError> {
    let mut config = SerialConfig {
        baud_rate,
        ..SerialConfig::default()
    };
    if let Some(v) = data_bits {
        config.data_bits = match v {
            5 => DataBits::Five,
            6 => DataBits::Six,
            7 => DataBits::Seven,
            8 => DataBits::Eight,
            _ => {
                return Err(SerialError::Configuration(
                    "Invalid data bits specified".to_string(),
                ));
            }
        };
    }
    if let Some(v) = stop_bits {
        config.stop_bits = match v {
            1 => StopBits::One,
            2 => StopBits::Two,
            _ => {
                return Err(SerialError::Configuration(
                    "Invalid stop bits specified".to_string(),
                ));
            }
        };
    }
    if let Some(v) = parity {
        let v = v.to_lowercase();
        if v == "odd" {
            config.parity = Parity::Odd;
        } else if v == "even" {
            config.parity = Parity::Even;
        } else if v == "none" {
            config.parity = Parity::None;
        } else {
            return Err(SerialError::Configuration(
                "Invalid parity specified".to_string(),
            ));
        }
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LogFn;
    use std::future::Future;

    #[test]
    /// MB-R-072 — unset serial parameters leave the Modbus serial-line default (8E1) in place.
    fn ut_serial_config_valid_minimal() {
        // No optional fields set: the configuration carries the stated baud rate and the
        // library's Modbus defaults for everything else.
        let cfg = serial_config_from(9600, None, None, None).unwrap();
        assert_eq!(cfg.baud_rate, 9600);
        assert_eq!(cfg.data_bits, DataBits::Eight);
        assert_eq!(cfg.parity, Parity::Even);
        assert_eq!(cfg.stop_bits, StopBits::One);
    }

    #[test]
    /// MB-R-073 — valid `data_bits`/`stop_bits`/`parity` values are accepted.
    fn ut_serial_config_valid_full() {
        let r = serial_config_from(19200, Some(8), Some(1), Some("even"));
        assert!(r.is_ok());
    }

    #[test]
    /// MB-R-073 — `parity` accepts `even`/`odd`/`none` case-insensitively.
    fn ut_serial_config_parity_case_insensitive() {
        // Parity is lower-cased before matching, so mixed case is accepted.
        assert!(serial_config_from(9600, None, None, Some("ODD")).is_ok());
        assert!(serial_config_from(9600, None, None, Some("None")).is_ok());
    }

    #[test]
    /// MB-R-073 — a `data_bits` value other than 5/6/7/8 fails with a serial configuration error.
    fn ut_serial_config_rejects_bad_data_bits() {
        let e = serial_config_from(9600, Some(9), None, None).unwrap_err();
        assert!(matches!(e, SerialError::Configuration(_)));
        assert!(e.to_string().contains("data bits"));
    }

    #[test]
    /// MB-R-073 — a `stop_bits` value other than 1/2 fails with a serial configuration error.
    fn ut_serial_config_rejects_bad_stop_bits() {
        let e = serial_config_from(9600, None, Some(3), None).unwrap_err();
        assert!(matches!(e, SerialError::Configuration(_)));
        assert!(e.to_string().contains("stop bits"));
    }

    #[test]
    /// MB-R-073 — a `parity` value other than even/odd/none fails with a serial configuration error.
    fn ut_serial_config_rejects_bad_parity() {
        let e = serial_config_from(9600, None, None, Some("bogus")).unwrap_err();
        assert!(matches!(e, SerialError::Configuration(_)));
        assert!(e.to_string().contains("parity"));
    }

    #[test]
    /// MB-R-073 — `data_bits` accepts exactly 5, 6, 7, and 8.
    fn ut_serial_config_accepts_all_data_bit_widths() {
        for bits in [5u8, 6, 7, 8] {
            assert!(serial_config_from(9600, Some(bits), None, None).is_ok());
        }
    }

    #[test]
    /// MB-R-073 — `stop_bits` accepts exactly 1 and 2.
    fn ut_serial_config_accepts_both_stop_bits() {
        assert!(serial_config_from(9600, None, Some(1), None).is_ok());
        assert!(serial_config_from(9600, None, Some(2), None).is_ok());
    }

    // Verifies the stable `LogFn` blanket impl (replacing the former nightly `async_fn_traits`
    // bound) is satisfied by an ordinary closure returning a `Send` async block. Compile-time
    // check only — no runtime needed (this crate's tokio has no `rt` feature).
    #[test]
    fn ut_logfn_impl_for_closure_returning_async_block() {
        fn assert_logfn<L: LogFn>(_: &L) {}
        let f = move |s: String| async move {
            let _ = s.len();
        };
        assert_logfn(&f);
    }

    // The future a `LogFn` hands back must be `Send` (background tasks are spawned onto a
    // multi-threaded runtime); pin it behind a `Send` bound to lock that in.
    #[test]
    fn ut_logfn_future_is_send() {
        fn assert_send_fut<F: Future + Send>(_: &F) {}
        let f = |s: String| async move {
            let _ = s;
        };
        let fut = f.invoke("hi".to_string());
        assert_send_fut(&fut);
    }
}
