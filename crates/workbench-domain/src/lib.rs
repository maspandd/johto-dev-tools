//! Domain model for API Studio: workspaces, projects, environments,
//! endpoints, path templates, mock responses, response rules and
//! captured request logs.
//!
//! Everything here is plain data plus validation. Execution lives in
//! `mock-engine`; persistence lives in `workbench-storage`.

pub mod dataset;
pub mod endpoint;
pub mod environment;
pub mod method;
pub mod path;
pub mod project;
pub mod request_log;
pub mod resource;
pub mod response;
pub mod validation;
pub mod workspace;

pub use dataset::{
    DataSchema, Dataset, FieldKind, FieldSpec, Locale, MAX_ARRAY_ITEMS, MAX_RECORDS,
};
pub use endpoint::{
    AuthSimulation, Endpoint, EndpointValidationError, ParamSpec, RequestSpec, RouteConflict,
    find_route_conflicts,
};
pub use environment::Environment;
pub use method::{Method, ParseMethodError};
pub use path::{PathTemplate, PathTemplateError, Segment};
pub use project::{PROJECT_FORMAT_VERSION, Project};
pub use request_log::{CapturedBody, HistoryFilter, HistoryRecord};
pub use resource::{
    DeleteBehavior, KeyStrategy, MAX_PAGE_LIMIT, Pagination, RecordChange, Resource, SeedSpec,
    StoredRecord, key_of,
};
pub use response::{Condition, MockResponse, Operator, ResponseBody, ResponseRule, ValueSource};
pub use validation::{FieldError, MAX_NAME_LEN};
pub use workspace::Workspace;
