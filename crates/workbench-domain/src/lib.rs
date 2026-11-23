//! Domain model for API Studio: endpoints, path templates, mock responses,
//! and the declarative rules that pick a response.
//!
//! Everything here is plain data plus validation. Execution lives in
//! `mock-engine`; persistence lives in `workbench-storage`.

pub mod endpoint;
pub mod method;
pub mod path;
pub mod response;

pub use endpoint::{
    AuthSimulation, Endpoint, EndpointValidationError, ParamSpec, RequestSpec, RouteConflict,
    find_route_conflicts,
};
pub use method::{Method, ParseMethodError};
pub use path::{PathTemplate, PathTemplateError, Segment};
pub use response::{Condition, MockResponse, Operator, ResponseBody, ResponseRule, ValueSource};
