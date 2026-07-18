//! Organizations, memberships, invitations, and explicit role authorization.

use std::{fmt, str::FromStr};

use axum::{
    Extension, Json, Router,
    extract::{Path, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use boson_admin::AdminPrincipal;
use boson_capability::{Capability, CapabilityDescriptor};
use boson_db::Database;
use boson_identity::{AuthenticatedUser, IdentityAuth, IdentityDirectory};
use boson_kernel::RequestContext;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::Row;
use thiserror::Error;
use uuid::Uuid;

const DEFAULT_INVITATION_DAYS: i64 = 7;
const MAX_INVITATION_DAYS: i64 = 30;
const MAX_NAME_CHARS: usize = 100;
const MIN_SLUG_CHARS: usize = 3;
const MAX_SLUG_CHARS: usize = 63;

/// The deliberately small set of organization roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Owner,
    Admin,
    Member,
}

impl Role {
    #[must_use]
    pub const fn allows(self, permission: Permission) -> bool {
        match self {
            Self::Owner => true,
            Self::Admin => matches!(
                permission,
                Permission::View | Permission::Update | Permission::ManageMembers
            ),
            Self::Member => matches!(permission, Permission::View),
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Admin => "admin",
            Self::Member => "member",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Role {
    type Err = OrganizationsError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "owner" => Ok(Self::Owner),
            "admin" => Ok(Self::Admin),
            "member" => Ok(Self::Member),
            _ => Err(OrganizationsError::Invalid(
                "role must be owner, admin, or member".into(),
            )),
        }
    }
}

/// Operations understood by organization roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    View,
    Update,
    ManageMembers,
    ChangeOwner,
    Delete,
}

/// Reusable organization membership authorization for future capabilities.
#[derive(Clone)]
pub struct OrganizationAuthorizer {
    database: Option<Database>,
}

impl OrganizationAuthorizer {
    /// Loads the active role for a user in an organization.
    ///
    /// # Errors
    ///
    /// Returns [`OrganizationsError::Unavailable`] when `PostgreSQL` is disabled
    /// or the membership cannot be queried.
    pub async fn role_for(
        &self,
        organization_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<Role>, OrganizationsError> {
        let database = self
            .database
            .as_ref()
            .ok_or_else(|| OrganizationsError::Unavailable("PostgreSQL is disabled".into()))?;
        let role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM organizations.memberships
             WHERE organization_id = $1 AND user_id = $2",
        )
        .bind(organization_id)
        .bind(user_id)
        .fetch_optional(database.pool())
        .await
        .map_err(unavailable)?;
        role.map(|value| value.parse()).transpose()
    }

    /// Requires a specific permission and returns the caller's role.
    ///
    /// # Errors
    ///
    /// Returns not-found for non-members to avoid leaking organization ids, or
    /// forbidden when the caller is a member without the requested permission.
    pub async fn require(
        &self,
        organization_id: Uuid,
        user_id: Uuid,
        permission: Permission,
    ) -> Result<Role, OrganizationsError> {
        let role = self
            .role_for(organization_id, user_id)
            .await?
            .ok_or(OrganizationsError::NotFound)?;
        if role.allows(permission) {
            Ok(role)
        } else {
            Err(OrganizationsError::Forbidden)
        }
    }
}

#[derive(Clone)]
struct OrganizationsState {
    database: Option<Database>,
    auth: IdentityAuth,
    directory: IdentityDirectory,
    authorizer: OrganizationAuthorizer,
}

#[derive(Clone)]
pub struct OrganizationsCapability {
    state: OrganizationsState,
}

impl OrganizationsCapability {
    #[must_use]
    pub fn new(
        database: Option<Database>,
        auth: IdentityAuth,
        directory: IdentityDirectory,
    ) -> Self {
        Self {
            state: OrganizationsState {
                authorizer: OrganizationAuthorizer {
                    database: database.clone(),
                },
                database,
                auth,
                directory,
            },
        }
    }

    #[must_use]
    pub fn authorizer(&self) -> OrganizationAuthorizer {
        self.state.authorizer.clone()
    }
}

impl Capability for OrganizationsCapability {
    fn descriptor(&self) -> CapabilityDescriptor {
        CapabilityDescriptor {
            name: "organizations",
            version: env!("CARGO_PKG_VERSION"),
            dependencies: &["identity"],
        }
    }

    fn scopes(&self) -> &'static [&'static str] {
        &["organizations:read"]
    }

    fn app_router(&self) -> Router {
        Router::new()
            .route(
                "/organizations",
                post(create_organization).get(list_organizations),
            )
            .route(
                "/organizations/{organization_id}",
                get(get_organization)
                    .patch(update_organization)
                    .delete(delete_organization),
            )
            .route(
                "/organizations/{organization_id}/members",
                get(list_members),
            )
            .route(
                "/organizations/{organization_id}/members/{user_id}",
                axum::routing::patch(change_member_role).delete(remove_member),
            )
            .route(
                "/organizations/{organization_id}/invitations",
                post(create_invitation),
            )
            .route("/organization-invitations/accept", post(accept_invitation))
            .route_layer(middleware::from_fn_with_state(
                self.state.clone(),
                require_user,
            ))
            .with_state(self.state.clone())
    }

    fn admin_router(&self) -> Router {
        Router::new()
            .route("/organizations", get(admin_list_organizations))
            .route("/organization-memberships", get(admin_list_memberships))
            .route("/organization-invitations", get(admin_list_invitations))
            .with_state(self.state.clone())
    }
}

#[derive(Debug, Error)]
pub enum OrganizationsError {
    #[error("invalid or expired credentials")]
    Unauthorized,
    #[error("organization permission denied")]
    Forbidden,
    #[error("organization resource not found")]
    NotFound,
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("organization resource conflicts with existing state: {0}")]
    Conflict(String),
    #[error("organizations service unavailable: {0}")]
    Unavailable(String),
}

impl IntoResponse for OrganizationsError {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "organizations.unauthorized"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "organizations.forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "organizations.not_found"),
            Self::Invalid(_) => (StatusCode::BAD_REQUEST, "organizations.invalid"),
            Self::Conflict(_) => (StatusCode::CONFLICT, "organizations.conflict"),
            Self::Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "organizations.unavailable"),
        };
        (
            status,
            Json(json!({
                "error": {
                    "code": code,
                    "message": self.to_string()
                }
            })),
        )
            .into_response()
    }
}

#[derive(Debug, Serialize)]
struct OrganizationDto {
    id: Uuid,
    name: String,
    slug: String,
    created_by: Uuid,
    role: Role,
    member_count: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct MemberDto {
    organization_id: Uuid,
    user_id: Uuid,
    role: Role,
    joined_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct InvitationDto {
    id: Uuid,
    organization_id: Uuid,
    email: String,
    role: Role,
    expires_at: DateTime<Utc>,
    accepted_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    invited_by: Uuid,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct IssuedInvitation {
    invitation: InvitationDto,
    /// Returned once; only its SHA-256 hash is persisted.
    token: String,
}

#[derive(Debug, Deserialize)]
struct CreateOrganizationRequest {
    name: String,
    slug: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateOrganizationRequest {
    name: Option<String>,
    slug: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChangeRoleRequest {
    role: String,
}

#[derive(Debug, Deserialize)]
struct CreateInvitationRequest {
    email: String,
    role: String,
    expires_in_days: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct AcceptInvitationRequest {
    token: String,
}

async fn create_organization(
    State(state): State<OrganizationsState>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(context): Extension<RequestContext>,
    Json(input): Json<CreateOrganizationRequest>,
) -> Result<(StatusCode, Json<OrganizationDto>), OrganizationsError> {
    let name = validate_name(&input.name)?;
    let slug = normalize_slug(input.slug.as_deref().unwrap_or(&name))?;
    let database = require_database(&state)?;
    let organization_id = Uuid::now_v7();
    let mut transaction = database.pool().begin().await.map_err(unavailable)?;
    let row = sqlx::query(
        "INSERT INTO organizations.organizations (id, name, slug, created_by)
         VALUES ($1, $2, $3, $4)
         RETURNING created_at, updated_at",
    )
    .bind(organization_id)
    .bind(&name)
    .bind(&slug)
    .bind(user.user_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(map_write_error)?;
    sqlx::query(
        "INSERT INTO organizations.memberships
         (organization_id, user_id, role) VALUES ($1, $2, 'owner')",
    )
    .bind(organization_id)
    .bind(user.user_id)
    .execute(&mut *transaction)
    .await
    .map_err(unavailable)?;
    insert_outbox_event(
        &mut transaction,
        "organizations.organization_created.v1",
        json!({
            "organization_id": organization_id,
            "created_by": user.user_id,
            "name": name,
            "slug": slug
        }),
        Some(context.request_id),
    )
    .await
    .map_err(unavailable)?;
    insert_outbox_event(
        &mut transaction,
        "organizations.member_added.v1",
        json!({
            "organization_id": organization_id,
            "user_id": user.user_id,
            "role": "owner"
        }),
        Some(context.request_id),
    )
    .await
    .map_err(unavailable)?;
    transaction.commit().await.map_err(unavailable)?;
    Ok((
        StatusCode::CREATED,
        Json(OrganizationDto {
            id: organization_id,
            name,
            slug,
            created_by: user.user_id,
            role: Role::Owner,
            member_count: 1,
            created_at: row.try_get("created_at").map_err(unavailable)?,
            updated_at: row.try_get("updated_at").map_err(unavailable)?,
        }),
    ))
}

async fn list_organizations(
    State(state): State<OrganizationsState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<serde_json::Value>, OrganizationsError> {
    let database = require_database(&state)?;
    let rows = sqlx::query(
        "SELECT o.id, o.name, o.slug, o.created_by, m.role,
                o.created_at, o.updated_at, count(all_members.user_id) AS member_count
         FROM organizations.organizations o
         JOIN organizations.memberships m
           ON m.organization_id = o.id AND m.user_id = $1
         JOIN organizations.memberships all_members
           ON all_members.organization_id = o.id
         GROUP BY o.id, m.role
         ORDER BY o.created_at DESC",
    )
    .bind(user.user_id)
    .fetch_all(database.pool())
    .await
    .map_err(unavailable)?;
    let organizations = rows
        .iter()
        .map(organization_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({ "data": organizations })))
}

async fn get_organization(
    State(state): State<OrganizationsState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(organization_id): Path<Uuid>,
) -> Result<Json<OrganizationDto>, OrganizationsError> {
    state
        .authorizer
        .require(organization_id, user.user_id, Permission::View)
        .await?;
    let database = require_database(&state)?;
    let row = sqlx::query(
        "SELECT o.id, o.name, o.slug, o.created_by, m.role,
                o.created_at, o.updated_at, count(all_members.user_id) AS member_count
         FROM organizations.organizations o
         JOIN organizations.memberships m
           ON m.organization_id = o.id AND m.user_id = $2
         JOIN organizations.memberships all_members
           ON all_members.organization_id = o.id
         WHERE o.id = $1
         GROUP BY o.id, m.role",
    )
    .bind(organization_id)
    .bind(user.user_id)
    .fetch_optional(database.pool())
    .await
    .map_err(unavailable)?
    .ok_or(OrganizationsError::NotFound)?;
    Ok(Json(organization_from_row(&row)?))
}

async fn update_organization(
    State(state): State<OrganizationsState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(organization_id): Path<Uuid>,
    Json(input): Json<UpdateOrganizationRequest>,
) -> Result<Json<OrganizationDto>, OrganizationsError> {
    if input.name.is_none() && input.slug.is_none() {
        return Err(OrganizationsError::Invalid(
            "name or slug is required".into(),
        ));
    }
    let name = input.name.as_deref().map(validate_name).transpose()?;
    let slug = input.slug.as_deref().map(normalize_slug).transpose()?;
    let database = require_database(&state)?;
    let mut transaction = database.pool().begin().await.map_err(unavailable)?;
    let memberships = lock_memberships(&mut transaction, organization_id).await?;
    let actor_role = role_in_locked(&memberships, user.user_id)?;
    if !actor_role.allows(Permission::Update) {
        return Err(OrganizationsError::Forbidden);
    }
    sqlx::query(
        "UPDATE organizations.organizations
         SET name = COALESCE($2, name), slug = COALESCE($3, slug), updated_at = now()
         WHERE id = $1",
    )
    .bind(organization_id)
    .bind(name)
    .bind(slug)
    .execute(&mut *transaction)
    .await
    .map_err(map_write_error)?;
    let row = sqlx::query(
        "SELECT o.id, o.name, o.slug, o.created_by, $2::TEXT AS role,
                o.created_at, o.updated_at, count(m.user_id) AS member_count
         FROM organizations.organizations o
         JOIN organizations.memberships m ON m.organization_id = o.id
         WHERE o.id = $1
         GROUP BY o.id",
    )
    .bind(organization_id)
    .bind(actor_role.as_str())
    .fetch_one(&mut *transaction)
    .await
    .map_err(unavailable)?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(Json(organization_from_row(&row)?))
}

async fn delete_organization(
    State(state): State<OrganizationsState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(organization_id): Path<Uuid>,
) -> Result<StatusCode, OrganizationsError> {
    let database = require_database(&state)?;
    let mut transaction = database.pool().begin().await.map_err(unavailable)?;
    let memberships = lock_memberships(&mut transaction, organization_id).await?;
    let actor_role = role_in_locked(&memberships, user.user_id)?;
    if !actor_role.allows(Permission::Delete) {
        return Err(OrganizationsError::Forbidden);
    }
    let deleted = sqlx::query("DELETE FROM organizations.organizations WHERE id = $1")
        .bind(organization_id)
        .execute(&mut *transaction)
        .await
        .map_err(unavailable)?;
    if deleted.rows_affected() == 0 {
        return Err(OrganizationsError::NotFound);
    }
    transaction.commit().await.map_err(unavailable)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_members(
    State(state): State<OrganizationsState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(organization_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, OrganizationsError> {
    state
        .authorizer
        .require(organization_id, user.user_id, Permission::View)
        .await?;
    let database = require_database(&state)?;
    let rows = sqlx::query(
        "SELECT organization_id, user_id, role, joined_at, updated_at
         FROM organizations.memberships
         WHERE organization_id = $1
         ORDER BY joined_at",
    )
    .bind(organization_id)
    .fetch_all(database.pool())
    .await
    .map_err(unavailable)?;
    let members = rows
        .iter()
        .map(member_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({ "data": members })))
}

async fn change_member_role(
    State(state): State<OrganizationsState>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(context): Extension<RequestContext>,
    Path((organization_id, target_user_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<ChangeRoleRequest>,
) -> Result<Json<MemberDto>, OrganizationsError> {
    let new_role: Role = input.role.parse()?;
    let database = require_database(&state)?;
    let mut transaction = database.pool().begin().await.map_err(unavailable)?;
    let memberships = lock_memberships(&mut transaction, organization_id).await?;
    let actor_role = role_in_locked(&memberships, user.user_id)?;
    if !actor_role.allows(Permission::ManageMembers) {
        return Err(OrganizationsError::Forbidden);
    }
    let old_role = role_in_locked(&memberships, target_user_id)?;
    if (old_role == Role::Owner || new_role == Role::Owner)
        && !actor_role.allows(Permission::ChangeOwner)
    {
        return Err(OrganizationsError::Forbidden);
    }
    if old_role == Role::Owner && new_role != Role::Owner && owner_count(&memberships) == 1 {
        return Err(OrganizationsError::Conflict(
            "the final owner cannot be demoted".into(),
        ));
    }
    let row = sqlx::query(
        "UPDATE organizations.memberships
         SET role = $3, updated_at = now()
         WHERE organization_id = $1 AND user_id = $2
         RETURNING organization_id, user_id, role, joined_at, updated_at",
    )
    .bind(organization_id)
    .bind(target_user_id)
    .bind(new_role.as_str())
    .fetch_one(&mut *transaction)
    .await
    .map_err(unavailable)?;
    if old_role != new_role {
        insert_outbox_event(
            &mut transaction,
            "organizations.member_role_changed.v1",
            json!({
                "organization_id": organization_id,
                "user_id": target_user_id,
                "old_role": old_role,
                "new_role": new_role,
                "changed_by": user.user_id
            }),
            Some(context.request_id),
        )
        .await
        .map_err(unavailable)?;
    }
    transaction.commit().await.map_err(unavailable)?;
    Ok(Json(member_from_row(&row)?))
}

async fn remove_member(
    State(state): State<OrganizationsState>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(context): Extension<RequestContext>,
    Path((organization_id, target_user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, OrganizationsError> {
    let database = require_database(&state)?;
    let mut transaction = database.pool().begin().await.map_err(unavailable)?;
    let memberships = lock_memberships(&mut transaction, organization_id).await?;
    let actor_role = role_in_locked(&memberships, user.user_id)?;
    if !actor_role.allows(Permission::ManageMembers) {
        return Err(OrganizationsError::Forbidden);
    }
    let target_role = role_in_locked(&memberships, target_user_id)?;
    if target_role == Role::Owner && !actor_role.allows(Permission::ChangeOwner) {
        return Err(OrganizationsError::Forbidden);
    }
    if target_role == Role::Owner && owner_count(&memberships) == 1 {
        return Err(OrganizationsError::Conflict(
            "the final owner cannot be removed".into(),
        ));
    }
    sqlx::query(
        "DELETE FROM organizations.memberships
         WHERE organization_id = $1 AND user_id = $2",
    )
    .bind(organization_id)
    .bind(target_user_id)
    .execute(&mut *transaction)
    .await
    .map_err(unavailable)?;
    insert_outbox_event(
        &mut transaction,
        "organizations.member_removed.v1",
        json!({
            "organization_id": organization_id,
            "user_id": target_user_id,
            "role": target_role,
            "removed_by": user.user_id
        }),
        Some(context.request_id),
    )
    .await
    .map_err(unavailable)?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_invitation(
    State(state): State<OrganizationsState>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(context): Extension<RequestContext>,
    Path(organization_id): Path<Uuid>,
    Json(input): Json<CreateInvitationRequest>,
) -> Result<(StatusCode, Json<IssuedInvitation>), OrganizationsError> {
    let email = normalize_email(&input.email)?;
    let role: Role = input.role.parse()?;
    let days = input.expires_in_days.unwrap_or(DEFAULT_INVITATION_DAYS);
    if !(1..=MAX_INVITATION_DAYS).contains(&days) {
        return Err(OrganizationsError::Invalid(format!(
            "expires_in_days must be between 1 and {MAX_INVITATION_DAYS}"
        )));
    }
    let database = require_database(&state)?;
    let id = Uuid::now_v7();
    let token = generate_invitation_token();
    let expires_at = Utc::now() + Duration::days(days);
    let mut transaction = database.pool().begin().await.map_err(unavailable)?;
    let memberships = lock_memberships(&mut transaction, organization_id).await?;
    let actor_role = role_in_locked(&memberships, user.user_id)?;
    if !actor_role.allows(Permission::ManageMembers)
        || (role == Role::Owner && !actor_role.allows(Permission::ChangeOwner))
    {
        return Err(OrganizationsError::Forbidden);
    }
    let row = sqlx::query(
        "INSERT INTO organizations.invitations
         (id, organization_id, email, role, token_hash, expires_at, invited_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING created_at",
    )
    .bind(id)
    .bind(organization_id)
    .bind(&email)
    .bind(role.as_str())
    .bind(hash_invitation_token(&token))
    .bind(expires_at)
    .bind(user.user_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(map_write_error)?;
    insert_outbox_event(
        &mut transaction,
        "organizations.invitation_created.v1",
        json!({
            "invitation_id": id,
            "organization_id": organization_id,
            "email": email,
            "role": role,
            "invited_by": user.user_id,
            "expires_at": expires_at,
            "token": &token
        }),
        Some(context.request_id),
    )
    .await
    .map_err(unavailable)?;
    transaction.commit().await.map_err(unavailable)?;
    Ok((
        StatusCode::CREATED,
        Json(IssuedInvitation {
            invitation: InvitationDto {
                id,
                organization_id,
                email,
                role,
                expires_at,
                accepted_at: None,
                revoked_at: None,
                invited_by: user.user_id,
                created_at: row.try_get("created_at").map_err(unavailable)?,
            },
            token,
        }),
    ))
}

async fn accept_invitation(
    State(state): State<OrganizationsState>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(context): Extension<RequestContext>,
    Json(input): Json<AcceptInvitationRequest>,
) -> Result<(StatusCode, Json<MemberDto>), OrganizationsError> {
    if input.token.is_empty() || input.token.len() > 256 {
        return Err(OrganizationsError::Invalid(
            "invitation token is required".into(),
        ));
    }
    let user_email = state
        .directory
        .email_for_user(user.user_id)
        .await
        .map_err(unavailable)?
        .ok_or(OrganizationsError::Unauthorized)?;
    let database = require_database(&state)?;
    let mut transaction = database.pool().begin().await.map_err(unavailable)?;
    let token_hash = hash_invitation_token(&input.token);
    let organization_id: Uuid = sqlx::query_scalar(
        "SELECT organization_id FROM organizations.invitations
         WHERE token_hash = $1",
    )
    .bind(&token_hash)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(unavailable)?
    .ok_or(OrganizationsError::NotFound)?;
    lock_organization(&mut transaction, organization_id).await?;
    let invitation = sqlx::query(
        "SELECT id, organization_id, email, role, expires_at,
                accepted_at, revoked_at, invited_by, created_at
         FROM organizations.invitations
         WHERE token_hash = $1
         FOR UPDATE",
    )
    .bind(token_hash)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(unavailable)?
    .ok_or(OrganizationsError::NotFound)?;
    let email: String = invitation.try_get("email").map_err(unavailable)?;
    let expires_at: DateTime<Utc> = invitation.try_get("expires_at").map_err(unavailable)?;
    let accepted_at: Option<DateTime<Utc>> =
        invitation.try_get("accepted_at").map_err(unavailable)?;
    let revoked_at: Option<DateTime<Utc>> =
        invitation.try_get("revoked_at").map_err(unavailable)?;
    validate_invitation_state(&email, &user_email, expires_at, accepted_at, revoked_at)?;
    let invitation_id: Uuid = invitation.try_get("id").map_err(unavailable)?;
    let role: Role = invitation
        .try_get::<String, _>("role")
        .map_err(unavailable)?
        .parse()?;
    let member_row = sqlx::query(
        "INSERT INTO organizations.memberships
         (organization_id, user_id, role) VALUES ($1, $2, $3)
         RETURNING organization_id, user_id, role, joined_at, updated_at",
    )
    .bind(organization_id)
    .bind(user.user_id)
    .bind(role.as_str())
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| {
        if is_unique_violation(&error) {
            OrganizationsError::Conflict("user is already a member".into())
        } else {
            unavailable(error)
        }
    })?;
    sqlx::query(
        "UPDATE organizations.invitations
         SET accepted_at = now() WHERE id = $1",
    )
    .bind(invitation_id)
    .execute(&mut *transaction)
    .await
    .map_err(unavailable)?;
    publish_invitation_accepted_events(
        &mut transaction,
        invitation_id,
        organization_id,
        user.user_id,
        role,
        context.request_id,
    )
    .await?;
    transaction.commit().await.map_err(unavailable)?;
    Ok((StatusCode::CREATED, Json(member_from_row(&member_row)?)))
}

async fn publish_invitation_accepted_events(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    invitation_id: Uuid,
    organization_id: Uuid,
    user_id: Uuid,
    role: Role,
    request_id: Uuid,
) -> Result<(), OrganizationsError> {
    insert_outbox_event(
        transaction,
        "organizations.member_added.v1",
        json!({
            "organization_id": organization_id,
            "user_id": user_id,
            "role": role
        }),
        Some(request_id),
    )
    .await
    .map_err(unavailable)?;
    insert_outbox_event(
        transaction,
        "organizations.invitation_accepted.v1",
        json!({
            "invitation_id": invitation_id,
            "organization_id": organization_id,
            "user_id": user_id
        }),
        Some(request_id),
    )
    .await
    .map_err(unavailable)?;
    Ok(())
}

async fn admin_list_organizations(
    State(state): State<OrganizationsState>,
    Extension(principal): Extension<AdminPrincipal>,
) -> Result<Json<serde_json::Value>, OrganizationsError> {
    require_admin_scope(&principal)?;
    let database = require_database(&state)?;
    let rows = sqlx::query(
        "SELECT o.id, o.name, o.slug, o.created_by, o.created_at, o.updated_at,
                count(m.user_id) AS member_count
         FROM organizations.organizations o
         LEFT JOIN organizations.memberships m ON m.organization_id = o.id
         GROUP BY o.id
         ORDER BY o.created_at DESC
         LIMIT 200",
    )
    .fetch_all(database.pool())
    .await
    .map_err(unavailable)?;
    let data = rows
        .iter()
        .map(|row| {
            Ok(json!({
                "id": row.try_get::<Uuid, _>("id").map_err(unavailable)?,
                "name": row.try_get::<String, _>("name").map_err(unavailable)?,
                "slug": row.try_get::<String, _>("slug").map_err(unavailable)?,
                "created_by": row.try_get::<Uuid, _>("created_by").map_err(unavailable)?,
                "member_count": row.try_get::<i64, _>("member_count").map_err(unavailable)?,
                "created_at": row.try_get::<DateTime<Utc>, _>("created_at").map_err(unavailable)?,
                "updated_at": row.try_get::<DateTime<Utc>, _>("updated_at").map_err(unavailable)?
            }))
        })
        .collect::<Result<Vec<_>, OrganizationsError>>()?;
    Ok(Json(json!({ "data": data })))
}

async fn admin_list_memberships(
    State(state): State<OrganizationsState>,
    Extension(principal): Extension<AdminPrincipal>,
) -> Result<Json<serde_json::Value>, OrganizationsError> {
    require_admin_scope(&principal)?;
    let database = require_database(&state)?;
    let rows = sqlx::query(
        "SELECT organization_id, user_id, role, joined_at, updated_at
         FROM organizations.memberships
         ORDER BY joined_at DESC
         LIMIT 500",
    )
    .fetch_all(database.pool())
    .await
    .map_err(unavailable)?;
    let data = rows
        .iter()
        .map(member_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({ "data": data })))
}

async fn admin_list_invitations(
    State(state): State<OrganizationsState>,
    Extension(principal): Extension<AdminPrincipal>,
) -> Result<Json<serde_json::Value>, OrganizationsError> {
    require_admin_scope(&principal)?;
    let database = require_database(&state)?;
    let rows = sqlx::query(
        "SELECT id, organization_id, email, role, expires_at,
                accepted_at, revoked_at, invited_by, created_at
         FROM organizations.invitations
         ORDER BY created_at DESC
         LIMIT 500",
    )
    .fetch_all(database.pool())
    .await
    .map_err(unavailable)?;
    let data = rows
        .iter()
        .map(invitation_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({ "data": data })))
}

async fn require_user(
    State(state): State<OrganizationsState>,
    request: Request,
    next: Next,
) -> Response {
    let supplied = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let Some(token) = supplied else {
        return OrganizationsError::Unauthorized.into_response();
    };
    match state.auth.validate(token) {
        Ok(user) => {
            let mut request = request;
            request.extensions_mut().insert(user);
            next.run(request).await
        }
        Err(_) => OrganizationsError::Unauthorized.into_response(),
    }
}

async fn lock_memberships(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    organization_id: Uuid,
) -> Result<Vec<(Uuid, Role)>, OrganizationsError> {
    lock_organization(transaction, organization_id).await?;
    let rows = sqlx::query(
        "SELECT user_id, role FROM organizations.memberships
         WHERE organization_id = $1
         ORDER BY user_id
         FOR UPDATE",
    )
    .bind(organization_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(unavailable)?;
    rows.iter()
        .map(|row| {
            Ok((
                row.try_get("user_id").map_err(unavailable)?,
                row.try_get::<String, _>("role")
                    .map_err(unavailable)?
                    .parse()?,
            ))
        })
        .collect()
}

async fn lock_organization(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    organization_id: Uuid,
) -> Result<(), OrganizationsError> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM organizations.organizations
         WHERE id = $1
         FOR UPDATE",
    )
    .bind(organization_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(unavailable)?
    .ok_or(OrganizationsError::NotFound)?;
    Ok(())
}

fn role_in_locked(memberships: &[(Uuid, Role)], user_id: Uuid) -> Result<Role, OrganizationsError> {
    memberships
        .iter()
        .find_map(|(candidate, role)| (*candidate == user_id).then_some(*role))
        .ok_or(OrganizationsError::NotFound)
}

fn owner_count(memberships: &[(Uuid, Role)]) -> usize {
    memberships
        .iter()
        .filter(|(_, role)| *role == Role::Owner)
        .count()
}

fn validate_invitation_state(
    invited_email: &str,
    user_email: &str,
    expires_at: DateTime<Utc>,
    accepted_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
) -> Result<(), OrganizationsError> {
    if accepted_at.is_some() {
        return Err(OrganizationsError::Conflict(
            "invitation has already been accepted".into(),
        ));
    }
    if revoked_at.is_some() {
        return Err(OrganizationsError::Conflict(
            "invitation has been revoked".into(),
        ));
    }
    if expires_at <= Utc::now() {
        return Err(OrganizationsError::Conflict(
            "invitation has expired".into(),
        ));
    }
    if invited_email != user_email {
        return Err(OrganizationsError::Forbidden);
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<String, OrganizationsError> {
    let name = name.trim();
    let length = name.chars().count();
    if length == 0 || length > MAX_NAME_CHARS {
        return Err(OrganizationsError::Invalid(format!(
            "name must contain 1 to {MAX_NAME_CHARS} characters"
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(OrganizationsError::Invalid(
            "name must not contain control characters".into(),
        ));
    }
    Ok(name.to_owned())
}

fn normalize_slug(value: &str) -> Result<String, OrganizationsError> {
    let mut slug = String::with_capacity(value.len());
    let mut separator = false;
    for character in value.trim().chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            separator = false;
        } else if !slug.is_empty() && !separator {
            slug.push('-');
            separator = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if !(MIN_SLUG_CHARS..=MAX_SLUG_CHARS).contains(&slug.len()) {
        return Err(OrganizationsError::Invalid(format!(
            "slug must normalize to {MIN_SLUG_CHARS} to {MAX_SLUG_CHARS} ASCII characters"
        )));
    }
    Ok(slug)
}

fn normalize_email(value: &str) -> Result<String, OrganizationsError> {
    let email = value.trim().to_lowercase();
    let invalid = || OrganizationsError::Invalid("valid email is required".into());
    if email.is_empty() || email.len() > 320 || email.chars().any(char::is_whitespace) {
        return Err(invalid());
    }
    let (local, domain) = email.split_once('@').ok_or_else(invalid)?;
    if local.is_empty() || domain.is_empty() || !domain.contains('.') || domain.contains('@') {
        return Err(invalid());
    }
    Ok(email)
}

fn generate_invitation_token() -> String {
    format!(
        "boson_inv_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

fn hash_invitation_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn require_admin_scope(principal: &AdminPrincipal) -> Result<(), OrganizationsError> {
    if principal.allows("organizations:read") {
        Ok(())
    } else {
        Err(OrganizationsError::Forbidden)
    }
}

fn require_database(state: &OrganizationsState) -> Result<&Database, OrganizationsError> {
    state
        .database
        .as_ref()
        .ok_or_else(|| OrganizationsError::Unavailable("PostgreSQL is disabled".into()))
}

#[allow(clippy::needless_pass_by_value)]
fn unavailable(error: impl ToString) -> OrganizationsError {
    OrganizationsError::Unavailable(error.to_string())
}

fn map_write_error(error: sqlx::Error) -> OrganizationsError {
    if is_unique_violation(&error) {
        OrganizationsError::Conflict("slug or invitation already exists".into())
    } else {
        unavailable(error)
    }
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .is_some_and(|code| code == "23505")
}

fn organization_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<OrganizationDto, OrganizationsError> {
    Ok(OrganizationDto {
        id: row.try_get("id").map_err(unavailable)?,
        name: row.try_get("name").map_err(unavailable)?,
        slug: row.try_get("slug").map_err(unavailable)?,
        created_by: row.try_get("created_by").map_err(unavailable)?,
        role: row
            .try_get::<String, _>("role")
            .map_err(unavailable)?
            .parse()?,
        member_count: row.try_get("member_count").map_err(unavailable)?,
        created_at: row.try_get("created_at").map_err(unavailable)?,
        updated_at: row.try_get("updated_at").map_err(unavailable)?,
    })
}

fn member_from_row(row: &sqlx::postgres::PgRow) -> Result<MemberDto, OrganizationsError> {
    Ok(MemberDto {
        organization_id: row.try_get("organization_id").map_err(unavailable)?,
        user_id: row.try_get("user_id").map_err(unavailable)?,
        role: row
            .try_get::<String, _>("role")
            .map_err(unavailable)?
            .parse()?,
        joined_at: row.try_get("joined_at").map_err(unavailable)?,
        updated_at: row.try_get("updated_at").map_err(unavailable)?,
    })
}

fn invitation_from_row(row: &sqlx::postgres::PgRow) -> Result<InvitationDto, OrganizationsError> {
    Ok(InvitationDto {
        id: row.try_get("id").map_err(unavailable)?,
        organization_id: row.try_get("organization_id").map_err(unavailable)?,
        email: row.try_get("email").map_err(unavailable)?,
        role: row
            .try_get::<String, _>("role")
            .map_err(unavailable)?
            .parse()?,
        expires_at: row.try_get("expires_at").map_err(unavailable)?,
        accepted_at: row.try_get("accepted_at").map_err(unavailable)?,
        revoked_at: row.try_get("revoked_at").map_err(unavailable)?,
        invited_by: row.try_get("invited_by").map_err(unavailable)?,
        created_at: row.try_get("created_at").map_err(unavailable)?,
    })
}

async fn insert_outbox_event(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    topic: &str,
    payload: serde_json::Value,
    correlation_id: Option<Uuid>,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO kernel.outbox
         (id, topic, payload, correlation_id, occurred_at)
         VALUES ($1, $2, $3, $4, now())",
    )
    .bind(Uuid::now_v7())
    .bind(topic)
    .bind(payload)
    .bind(correlation_id.map(|id| id.to_string()))
    .execute(&mut **transaction)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_normalized_and_validated() {
        assert_eq!(
            normalize_slug("  Acme, Incorporated! ").unwrap(),
            "acme-incorporated"
        );
        assert_eq!(normalize_slug("A---B___C").unwrap(), "a-b-c");
        assert!(normalize_slug("!!").is_err());
        assert!(normalize_slug(&"a".repeat(MAX_SLUG_CHARS + 1)).is_err());
    }

    #[test]
    fn roles_parse_and_render_exactly() {
        for role in [Role::Owner, Role::Admin, Role::Member] {
            assert_eq!(role.to_string().parse::<Role>().unwrap(), role);
        }
        assert!("Owner".parse::<Role>().is_err());
    }

    #[test]
    fn permission_matrix_is_explicit() {
        for permission in [
            Permission::View,
            Permission::Update,
            Permission::ManageMembers,
            Permission::ChangeOwner,
            Permission::Delete,
        ] {
            assert!(Role::Owner.allows(permission));
        }
        assert!(Role::Admin.allows(Permission::View));
        assert!(Role::Admin.allows(Permission::Update));
        assert!(Role::Admin.allows(Permission::ManageMembers));
        assert!(!Role::Admin.allows(Permission::ChangeOwner));
        assert!(!Role::Admin.allows(Permission::Delete));
        assert!(Role::Member.allows(Permission::View));
        assert!(!Role::Member.allows(Permission::Update));
    }

    #[test]
    fn invitation_tokens_are_only_stored_as_hashes() {
        let token = generate_invitation_token();
        let hash = hash_invitation_token(&token);
        assert!(token.starts_with("boson_inv_"));
        assert_ne!(token, hash);
        assert_eq!(hash.len(), 64);
        assert_eq!(hash, hash_invitation_token(&token));
    }
}
