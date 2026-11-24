//! Route table for `/api/v1` (spec §8).

mod datasets;
mod endpoints;
mod environments;
mod projects;
mod resources;
mod servers;
mod workspaces;

use axum::Router;
use axum::routing::{get, post, put};

use crate::AppState;

pub(crate) fn api() -> Router<AppState> {
    Router::new()
        .route(
            "/workspaces",
            get(workspaces::list).post(workspaces::create),
        )
        .route(
            "/workspaces/{id}",
            get(workspaces::get)
                .patch(workspaces::update)
                .delete(workspaces::delete),
        )
        .route("/projects", get(projects::list).post(projects::create))
        .route(
            "/projects/{id}",
            get(projects::get)
                .patch(projects::update)
                .delete(projects::delete),
        )
        .route("/projects/{id}/duplicate", post(projects::duplicate))
        .route(
            "/projects/{id}/environments",
            get(environments::list).post(environments::create),
        )
        .route(
            "/environments/{id}",
            get(environments::get)
                .patch(environments::update)
                .delete(environments::delete),
        )
        .route(
            "/environments/{id}/secrets",
            get(environments::list_secrets),
        )
        .route(
            "/environments/{id}/secrets/{name}",
            put(environments::put_secret).delete(environments::delete_secret),
        )
        .route(
            "/projects/{id}/endpoints",
            get(endpoints::list).post(endpoints::create),
        )
        .route("/projects/{id}/endpoints/reorder", post(endpoints::reorder))
        .route(
            "/endpoints/{id}",
            get(endpoints::get)
                .patch(endpoints::update)
                .delete(endpoints::delete),
        )
        .route("/endpoints/{id}/duplicate", post(endpoints::duplicate))
        .route("/endpoints/{id}/enable", post(endpoints::enable))
        .route("/endpoints/{id}/disable", post(endpoints::disable))
        .route(
            "/projects/{id}/resources",
            get(resources::list).post(resources::create),
        )
        .route(
            "/resources/{id}",
            get(resources::get)
                .patch(resources::update)
                .delete(resources::delete),
        )
        .route("/resources/{id}/records", get(resources::records))
        .route("/resources/{id}/reset", post(resources::reset))
        .route(
            "/projects/{id}/datasets",
            get(datasets::list).post(datasets::create),
        )
        .route(
            "/projects/{id}/datasets/preview",
            post(datasets::preview_unsaved),
        )
        .route(
            "/datasets/{id}",
            get(datasets::get)
                .patch(datasets::update)
                .delete(datasets::delete),
        )
        .route("/datasets/{id}/preview", get(datasets::preview))
        .route("/datasets/{id}/export", get(datasets::export))
        .route(
            "/projects/{id}/mock-servers",
            get(servers::list_for_project).post(servers::start),
        )
        .route("/mock-servers", get(servers::list_all))
        .route(
            "/mock-servers/{id}",
            get(servers::get)
                .patch(servers::reload)
                .delete(servers::stop),
        )
        .route("/mock-servers/{id}/logs", get(servers::server_logs))
        .route(
            "/projects/{id}/request-logs",
            get(servers::project_logs).delete(servers::clear_logs),
        )
}
