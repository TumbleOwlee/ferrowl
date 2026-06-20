//! OCPP charging-station (client) module: a version-generic backend plus one concrete view per
//! OCPP version (1.6 implemented; 2.0.1 a placeholder for now).

pub mod backend;
pub mod v1_6;
pub mod v2_0_1;

pub use v1_6::OcppClientV16View;
pub use v2_0_1::OcppClientV201View;

use crate::module::ocpp::config::session::{OcppSpec, OcppVersion};
use crate::module::view::ModuleView;

/// Build the concrete client view for a spec's OCPP version.
pub fn build_client_view(spec: OcppSpec) -> Box<dyn ModuleView> {
    match spec.version {
        OcppVersion::V1_6 => Box::new(OcppClientV16View::new(spec)),
        OcppVersion::V2_0_1 => Box::new(OcppClientV201View::new(spec)),
    }
}
