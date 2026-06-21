//! OCPP **server** (CSMS) module: a version-generic [`view::ServerView`] plus the two version
//! modules supplying observed-state types, inbound handlers, and the [`view::ServerVersion`] glue.

pub mod backend;
mod detail;
mod v1_6;
mod v2_0_1;
pub mod view;

use ferrowl_ocpp::{V1_6, V2_0_1};

use crate::module::ocpp::config::device::OcppDeviceConfig;
use crate::module::ocpp::config::session::{OcppSpec, OcppVersion};
use crate::module::view::ModuleView;

/// Build the concrete CSMS view for a spec's OCPP version, carrying the device-config path and the
/// loaded device config (role/version/timeout/scripts).
pub fn build_server_view(
    spec: OcppSpec,
    device_path: String,
    device: OcppDeviceConfig,
) -> Box<dyn ModuleView> {
    match spec.version {
        OcppVersion::V1_6 => Box::new(view::ServerView::<V1_6>::new(spec, device_path, device)),
        OcppVersion::V2_0_1 => Box::new(view::ServerView::<V2_0_1>::new(spec, device_path, device)),
    }
}
