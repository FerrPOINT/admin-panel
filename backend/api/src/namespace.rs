//! Human registry API; owner resource commands use separate registered machine credentials.
use crate::{Caller, SharedState};
use admin_panel_domain::{DomainError, namespace::*};
use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

fn failure(error: DomainError) -> Response {
    let (status, code) = match error {
        DomainError::NotFound(_) => (StatusCode::NOT_FOUND, "namespace_not_found".to_string()),
        DomainError::Conflict(code) => (StatusCode::CONFLICT, code),
        DomainError::PreconditionFailed(code) => (StatusCode::PRECONDITION_FAILED, code),
        DomainError::Validation(code) => (StatusCode::UNPROCESSABLE_ENTITY, code),
        DomainError::InvalidTransition(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "namespace_storage_unavailable".into(),
        ),
    };
    (status, Json(json!({"error":{"code":code}}))).into_response()
}
fn unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error":{"code":"namespace_not_configured"}})),
    )
        .into_response()
}
fn writable(caller: &Caller) -> bool {
    caller.can_mutate
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct Page {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[utoipa::path(get,path="/api/v1/namespace-owners",tag="namespaces",responses((status=200,body=Vec<OwnerInstance>),(status=503,description="Registry unavailable")))]
pub async fn owners(State(state): State<SharedState>) -> Response {
    let Some(service) = &state.namespaces else {
        return unavailable();
    };
    Json(service.instances()).into_response()
}

#[utoipa::path(get,path="/api/v1/namespaces",tag="namespaces",params(Page),responses((status=200,body=Vec<Namespace>),(status=503,description="Namespace registry not configured")))]
pub async fn list(State(state): State<SharedState>, Query(page): Query<Page>) -> Response {
    let Some(service) = &state.namespaces else {
        return unavailable();
    };
    match service
        .repository
        .list(page.limit.unwrap_or(50), page.offset.unwrap_or(0))
        .await
    {
        Ok(result) => Json(result).into_response(),
        Err(error) => failure(error),
    }
}
#[utoipa::path(post,path="/api/v1/namespaces",tag="namespaces",request_body=CreateNamespace,responses((status=201,body=Namespace),(status=409,description="Original-key payload conflict")))]
pub async fn create(
    State(state): State<SharedState>,
    Extension(caller): Extension<Caller>,
    Json(cmd): Json<CreateNamespace>,
) -> Response {
    if !writable(&caller) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(service) = &state.namespaces else {
        return unavailable();
    };
    match service.repository.create(&cmd, &caller.subject).await {
        Ok(result) => (StatusCode::CREATED, Json(result)).into_response(),
        Err(error) => failure(error),
    }
}
#[utoipa::path(get,path="/api/v1/namespaces/{id}/context",tag="namespaces",params(("id"=Uuid,Path)),responses((status=200,body=NamespaceContext),(status=404,description="Namespace not found")))]
pub async fn context(State(state): State<SharedState>, Path(id): Path<Uuid>) -> Response {
    let Some(service) = &state.namespaces else {
        return unavailable();
    };
    match service.repository.context(id).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => failure(error),
    }
}
#[utoipa::path(patch,path="/api/v1/namespaces/{id}",tag="namespaces",params(("id"=Uuid,Path)),request_body=UpdateNamespace,responses((status=200,body=Namespace),(status=412,description="Revision conflict")))]
pub async fn update(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Extension(caller): Extension<Caller>,
    Json(cmd): Json<UpdateNamespace>,
) -> Response {
    if !writable(&caller) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(service) = &state.namespaces else {
        return unavailable();
    };
    match service.repository.update(id, &cmd, &caller.subject).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => failure(error),
    }
}
#[utoipa::path(post,path="/api/v1/namespaces/{id}/operations",tag="namespaces",params(("id"=Uuid,Path)),request_body=NamespaceCommand,responses((status=200,body=Operation),(status=202,body=Operation),(status=409,description="Resource reserved or original readback required")))]
pub async fn execute(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Extension(caller): Extension<Caller>,
    Json(cmd): Json<NamespaceCommand>,
) -> Response {
    if !writable(&caller) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(service) = &state.namespaces else {
        return unavailable();
    };
    match service.execute(id, &cmd, &caller.subject).await {
        Ok(result) => (
            if result.state == "completed" {
                StatusCode::OK
            } else {
                StatusCode::ACCEPTED
            },
            Json(result),
        )
            .into_response(),
        Err(error) => failure(error),
    }
}
#[utoipa::path(get,path="/api/v1/namespace-operations/{id}",tag="namespaces",params(("id"=Uuid,Path)),responses((status=200,body=Operation),(status=404,description="Operation not found")))]
pub async fn operation(State(state): State<SharedState>, Path(id): Path<Uuid>) -> Response {
    let Some(service) = &state.namespaces else {
        return unavailable();
    };
    match service.repository.operation(id).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => failure(error),
    }
}
#[utoipa::path(post,path="/api/v1/namespace-operations/{id}/reconcile",tag="namespaces",params(("id"=Uuid,Path)),responses((status=200,body=Operation),(status=202,body=Operation)))]
pub async fn reconcile(
    State(state): State<SharedState>,
    Path(id): Path<Uuid>,
    Extension(caller): Extension<Caller>,
) -> Response {
    if !writable(&caller) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(service) = &state.namespaces else {
        return unavailable();
    };
    match service.reconcile(id).await {
        Ok(result) => (
            if result.state == "completed" {
                StatusCode::OK
            } else {
                StatusCode::ACCEPTED
            },
            Json(result),
        )
            .into_response(),
        Err(error) => failure(error),
    }
}
