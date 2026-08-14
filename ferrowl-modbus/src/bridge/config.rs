use crate::{rtu, tcp};

/// One `--upstream`/`--downstream` descriptor, transport-tagged (BR-R-004).
pub struct BridgeEndpointSpec {
    pub kind: BridgeEndpointKind,
    /// BR-R-015 — only consulted when this spec is the *upstream* endpoint; parsed but
    /// ignored when it is downstream (mirrors the RTU `slave` key's inertness elsewhere).
    pub unit_ids: Option<UnitIdFilter>,
}

pub enum BridgeEndpointKind {
    Tcp(tcp::Config),
    Rtu(rtu::Config),
}

/// BR-R-015 — an allow-list of unit ids parsed from `unit_ids=1,3,5-8`. `None` (the
/// descriptor key absent) means "forward every unit id" (MB-R-065's no-filter default).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitIdFilter {
    ids: std::collections::BTreeSet<u8>,
}

impl UnitIdFilter {
    /// Parses `"1,3,5-8"` into the set `{1,3,5,6,7,8}`. A malformed segment (not a bare
    /// `u8`, not `u8-u8`, or a range with `start > end`) is a parse error naming the
    /// offending segment.
    pub fn parse(input: &str) -> Result<Self, String> {
        let mut ids = std::collections::BTreeSet::new();
        for part in input.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            match part.split_once('-') {
                Some((lo, hi)) => {
                    let lo: u8 = lo
                        .trim()
                        .parse()
                        .map_err(|_| format!("invalid unit_ids segment '{part}'"))?;
                    let hi: u8 = hi
                        .trim()
                        .parse()
                        .map_err(|_| format!("invalid unit_ids segment '{part}'"))?;
                    if lo > hi {
                        return Err(format!("invalid unit_ids range '{part}': start > end"));
                    }
                    ids.extend(lo..=hi);
                }
                None => {
                    let id: u8 = part
                        .parse()
                        .map_err(|_| format!("invalid unit_ids segment '{part}'"))?;
                    ids.insert(id);
                }
            }
        }
        Ok(Self { ids })
    }

    /// BR-R-015 — whether `unit` is on the allow-list.
    pub fn allows(&self, unit: rust_modbus::UnitId) -> bool {
        self.ids.contains(&unit.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BR-R-015 — `unit_ids=1,3,5-8` allows exactly `{1,3,5,6,7,8}`.
    #[test]
    fn ut_unit_id_filter_parses_list_and_range() {
        let filter = UnitIdFilter::parse("1,3,5-8").unwrap();
        for allowed in [1u8, 3, 5, 6, 7, 8] {
            assert!(
                filter.allows(rust_modbus::UnitId(allowed)),
                "expected {allowed} allowed"
            );
        }
        for disallowed in [0u8, 2, 4, 9] {
            assert!(
                !filter.allows(rust_modbus::UnitId(disallowed)),
                "expected {disallowed} disallowed"
            );
        }
    }

    /// BR-R-015 — a malformed segment or a reversed range is a parse error.
    #[test]
    fn ut_unit_id_filter_rejects_malformed_segment() {
        assert!(UnitIdFilter::parse("1,x,3").is_err());
        assert!(UnitIdFilter::parse("5-2").is_err());
    }

    /// BR-R-015 — an empty `unit_ids=` value allows nothing, distinct from the key being
    /// absent entirely (represented by `BridgeEndpointSpec::unit_ids: None`).
    #[test]
    fn ut_unit_id_filter_empty_input_allows_nothing() {
        let filter = UnitIdFilter::parse("").unwrap();
        for id in 0u8..=255 {
            assert!(!filter.allows(rust_modbus::UnitId(id)));
        }
    }
}
