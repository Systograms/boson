//! End-user identity: registration, login, refresh-token sessions, and JWTs.
//!
//! Passwords are stored as Argon2id hashes. Refresh tokens are opaque and
//! persisted only as SHA-256 hashes. Access tokens are short-lived HS256 JWTs
//! validated locally, never by calling another service.

use std::sync::Arc;

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::{
    Extension, Json, Router,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use boson_admin::AdminPrincipal;
use boson_capability::{Capability, CapabilityDescriptor};
use boson_db::Database;
use boson_kernel::{AuthConfig, RequestContext};
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::Row;
use thiserror::Error;
use uuid::Uuid;

const MIN_PASSWORD_CHARS: usize = 10;
const MAX_PASSWORD_CHARS: usize = 128;
const TOKEN_TYPE: &str = "Bearer";

/// Validates and issues end-user access tokens without leaving the process.
#[derive(Clone)]
pub struct IdentityAuth {
    issuer: Arc<str>,
    secret: Arc<str>,
    access_ttl_seconds: u64,
}

/// Claims carried by a Boson end-user access token.
#[derive(Debug, Serialize, Deserialize)]
pub struct AccessClaims {
    /// User id.
    pub sub: String,
    /// Session id backing this access token.
    pub sid: String,
    pub iss: String,
    pub iat: i64,
    pub exp: i64,
}

/// Request extension inserted by the identity JWT middleware.
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub user_id: Uuid,
    pub session_id: Uuid,
}

/// Narrow identity lookup service for capabilities that depend on identity.
///
/// Keeping this query here preserves identity's ownership of its schema.
#[derive(Clone)]
pub struct IdentityDirectory {
    database: Option<Database>,
}

impl IdentityDirectory {
    /// Returns the normalized email for an enabled user.
    ///
    /// # Errors
    ///
    /// Returns a database error when the identity store cannot be queried.
    pub async fn email_for_user(&self, user_id: Uuid) -> Result<Option<String>, sqlx::Error> {
        let Some(database) = &self.database else {
            return Ok(None);
        };
        sqlx::query_scalar(
            "SELECT email FROM identity.users
             WHERE id = $1 AND disabled_at IS NULL",
        )
        .bind(user_id)
        .fetch_optional(database.pool())
        .await
    }
}

impl IdentityAuth {
    #[must_use]
    pub fn new(config: &AuthConfig) -> Self {
        Self {
            issuer: Arc::from(config.issuer.as_str()),
            secret: Arc::from(config.jwt_secret.as_str()),
            access_ttl_seconds: config.access_ttl_seconds,
        }
    }

    /// Issues a signed access token bound to a user and refresh session.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::Unavailable`] when no signing secret is
    /// configured or encoding fails.
    pub fn issue(
        &self,
        user_id: Uuid,
        session_id: Uuid,
    ) -> Result<IssuedAccessToken, IdentityError> {
        let key = self.encoding_key()?;
        let now = Utc::now();
        let expires_in = i64::try_from(self.access_ttl_seconds).unwrap_or(i64::MAX);
        let claims = AccessClaims {
            sub: user_id.to_string(),
            sid: session_id.to_string(),
            iss: self.issuer.to_string(),
            iat: now.timestamp(),
            exp: now.timestamp().saturating_add(expires_in),
        };
        let token = jsonwebtoken::encode(&Header::new(Algorithm::HS256), &claims, &key)
            .map_err(|error| IdentityError::Unavailable(error.to_string()))?;
        Ok(IssuedAccessToken { token, expires_in })
    }

    /// Validates a bearer token locally: signature, expiry, and issuer.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::Unauthorized`] for invalid tokens and
    /// [`IdentityError::Unavailable`] when no signing secret is configured.
    pub fn validate(&self, token: &str) -> Result<AuthenticatedUser, IdentityError> {
        if self.secret.is_empty() {
            return Err(IdentityError::Unavailable(
                "auth.jwt_secret is not configured".into(),
            ));
        }
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[self.issuer.as_ref()]);
        validation.set_required_spec_claims(&["exp", "iss"]);
        let data = jsonwebtoken::decode::<AccessClaims>(
            token,
            &DecodingKey::from_secret(self.secret.as_bytes()),
            &validation,
        )
        .map_err(|_| IdentityError::Unauthorized)?;
        Ok(AuthenticatedUser {
            user_id: Uuid::parse_str(&data.claims.sub).map_err(|_| IdentityError::Unauthorized)?,
            session_id: Uuid::parse_str(&data.claims.sid)
                .map_err(|_| IdentityError::Unauthorized)?,
        })
    }

    fn encoding_key(&self) -> Result<EncodingKey, IdentityError> {
        if self.secret.is_empty() {
            return Err(IdentityError::Unavailable(
                "auth.jwt_secret is not configured".into(),
            ));
        }
        Ok(EncodingKey::from_secret(self.secret.as_bytes()))
    }
}

/// An encoded access token together with its lifetime in seconds.
#[derive(Debug)]
pub struct IssuedAccessToken {
    pub token: String,
    pub expires_in: i64,
}

#[derive(Clone)]
struct IdentityState {
    database: Option<Database>,
    auth: IdentityAuth,
    refresh_ttl_days: u64,
    email_verification_ttl_hours: u64,
    password_reset_ttl_minutes: u64,
}

#[derive(Clone)]
pub struct IdentityCapability {
    state: IdentityState,
}

impl IdentityCapability {
    #[must_use]
    pub fn new(database: Option<Database>, config: &AuthConfig) -> Self {
        Self {
            state: IdentityState {
                database,
                auth: IdentityAuth::new(config),
                refresh_ttl_days: config.refresh_ttl_days,
                email_verification_ttl_hours: config.email_verification_ttl_hours,
                password_reset_ttl_minutes: config.password_reset_ttl_minutes,
            },
        }
    }

    #[must_use]
    pub fn auth(&self) -> IdentityAuth {
        self.state.auth.clone()
    }

    #[must_use]
    pub fn directory(&self) -> IdentityDirectory {
        IdentityDirectory {
            database: self.state.database.clone(),
        }
    }
}

impl Capability for IdentityCapability {
    fn descriptor(&self) -> CapabilityDescriptor {
        CapabilityDescriptor {
            name: "identity",
            version: env!("CARGO_PKG_VERSION"),
            dependencies: &["admin"],
        }
    }

    fn scopes(&self) -> &'static [&'static str] {
        &["identity:read"]
    }

    fn app_router(&self) -> Router {
        let protected = Router::new()
            .route("/auth/me", get(me))
            .route(
                "/auth/email-verification/request",
                post(request_email_verification),
            )
            .route_layer(middleware::from_fn_with_state(
                self.state.clone(),
                require_user,
            ))
            .with_state(self.state.clone());
        Router::new()
            .route("/auth/register", post(register))
            .route("/auth/login", post(login))
            .route("/auth/refresh", post(refresh))
            .route("/auth/logout", post(logout))
            .route(
                "/auth/email-verification/confirm",
                post(confirm_email_verification),
            )
            .route("/auth/password-reset/request", post(request_password_reset))
            .route("/auth/password-reset/confirm", post(confirm_password_reset))
            .with_state(self.state.clone())
            .merge(protected)
    }

    fn admin_router(&self) -> Router {
        Router::new()
            .route("/users", get(admin_list_users))
            .route("/sessions", get(admin_list_sessions))
            .with_state(self.state.clone())
    }
}

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("invalid or expired credentials")]
    Unauthorized,
    #[error("missing required scope `{0}`")]
    Forbidden(&'static str),
    #[error("identity resource not found")]
    NotFound,
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("identity service unavailable: {0}")]
    Unavailable(String),
    #[error("an account with this email already exists")]
    Conflict,
}

impl IntoResponse for IdentityError {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "identity.unauthorized"),
            Self::Forbidden(_) => (StatusCode::FORBIDDEN, "identity.forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "identity.not_found"),
            Self::Invalid(_) => (StatusCode::BAD_REQUEST, "identity.invalid"),
            Self::Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "identity.unavailable"),
            Self::Conflict => (StatusCode::CONFLICT, "identity.conflict"),
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

/// Safe user representation. Never carries the password hash.
#[derive(Debug, Serialize)]
struct UserDto {
    id: Uuid,
    email: String,
    display_name: String,
    email_verified_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct SessionDto {
    id: Uuid,
    user_id: Uuid,
    expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
    last_used_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct TokenResponse {
    user: UserDto,
    access_token: String,
    /// Returned exactly once. Boson stores only its SHA-256 hash.
    refresh_token: String,
    token_type: &'static str,
    expires_in: i64,
}

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    email: String,
    display_name: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct RefreshRequest {
    refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct ActionTokenRequest {
    token: String,
}

#[derive(Debug, Deserialize)]
struct PasswordResetRequest {
    email: String,
}

#[derive(Debug, Deserialize)]
struct ConfirmPasswordResetRequest {
    token: String,
    password: String,
}

#[derive(Debug, Clone, Copy)]
enum EmailActionPurpose {
    VerifyEmail,
    ResetPassword,
}

impl EmailActionPurpose {
    const fn as_str(self) -> &'static str {
        match self {
            Self::VerifyEmail => "verify_email",
            Self::ResetPassword => "reset_password",
        }
    }

    const fn token_prefix(self) -> &'static str {
        match self {
            Self::VerifyEmail => "boson_verify_",
            Self::ResetPassword => "boson_reset_",
        }
    }
}

struct NewEmailAction {
    id: Uuid,
    token: String,
    token_hash: String,
    purpose: EmailActionPurpose,
    expires_at: DateTime<Utc>,
}

impl NewEmailAction {
    fn generate(purpose: EmailActionPurpose, ttl: Duration) -> Self {
        let token = format!(
            "{}{}{}",
            purpose.token_prefix(),
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        Self {
            id: Uuid::now_v7(),
            token_hash: hash_action_token(&token),
            token,
            purpose,
            expires_at: Utc::now() + ttl,
        }
    }

    async fn insert(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: Uuid,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE identity.email_action_tokens
             SET consumed_at = now()
             WHERE user_id = $1 AND purpose = $2 AND consumed_at IS NULL",
        )
        .bind(user_id)
        .bind(self.purpose.as_str())
        .execute(&mut **transaction)
        .await?;
        sqlx::query(
            "INSERT INTO identity.email_action_tokens
             (id, user_id, purpose, token_hash, expires_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(self.id)
        .bind(user_id)
        .bind(self.purpose.as_str())
        .bind(&self.token_hash)
        .bind(self.expires_at)
        .execute(&mut **transaction)
        .await?;
        Ok(())
    }
}

async fn register(
    State(state): State<IdentityState>,
    Extension(context): Extension<RequestContext>,
    Json(input): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<TokenResponse>), IdentityError> {
    let email = normalize_email(&input.email)?;
    let display_name = input.display_name.trim().to_owned();
    if display_name.is_empty() {
        return Err(IdentityError::Invalid("display_name is required".into()));
    }
    validate_password(&input.password)?;
    let password_hash = hash_password(&input.password)?;
    let database = require_database(&state)?;

    let user_id = Uuid::now_v7();
    let session = NewSession::generate(state.refresh_ttl_days);
    let verification = NewEmailAction::generate(
        EmailActionPurpose::VerifyEmail,
        Duration::hours(i64::try_from(state.email_verification_ttl_hours).unwrap_or(i64::MAX)),
    );
    let mut transaction = database.pool().begin().await.map_err(unavailable)?;
    let inserted = sqlx::query(
        "INSERT INTO identity.users (id, email, display_name, password_hash)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(user_id)
    .bind(&email)
    .bind(&display_name)
    .bind(&password_hash)
    .execute(&mut *transaction)
    .await;
    if let Err(error) = inserted {
        if is_unique_violation(&error) {
            return Err(IdentityError::Conflict);
        }
        return Err(unavailable(error));
    }
    session
        .insert(&mut transaction, user_id)
        .await
        .map_err(unavailable)?;
    verification
        .insert(&mut transaction, user_id)
        .await
        .map_err(unavailable)?;
    insert_outbox_event(
        &mut transaction,
        "identity.user_created.v1",
        json!({ "user_id": user_id, "email": email }),
        Some(context.request_id),
    )
    .await
    .map_err(unavailable)?;
    insert_outbox_event(
        &mut transaction,
        "identity.email_verification_requested.v1",
        json!({
            "user_id": user_id,
            "email": email,
            "token": verification.token,
            "expires_at": verification.expires_at
        }),
        Some(context.request_id),
    )
    .await
    .map_err(unavailable)?;
    transaction.commit().await.map_err(unavailable)?;

    let access = state.auth.issue(user_id, session.id)?;
    Ok((
        StatusCode::CREATED,
        Json(TokenResponse {
            user: UserDto {
                id: user_id,
                email,
                display_name,
                email_verified_at: None,
                created_at: Utc::now(),
            },
            access_token: access.token,
            refresh_token: session.token,
            token_type: TOKEN_TYPE,
            expires_in: access.expires_in,
        }),
    ))
}

async fn login(
    State(state): State<IdentityState>,
    Extension(context): Extension<RequestContext>,
    Json(input): Json<LoginRequest>,
) -> Result<Json<TokenResponse>, IdentityError> {
    let email = normalize_email(&input.email)?;
    let database = require_database(&state)?;
    let row = sqlx::query(
        "SELECT id, email, display_name, password_hash, email_verified_at, created_at
         FROM identity.users
         WHERE email = $1 AND disabled_at IS NULL",
    )
    .bind(&email)
    .fetch_optional(database.pool())
    .await
    .map_err(unavailable)?
    .ok_or(IdentityError::Unauthorized)?;

    let password_hash: String = row.try_get("password_hash").map_err(unavailable)?;
    if !verify_password(&input.password, &password_hash) {
        return Err(IdentityError::Unauthorized);
    }
    let user = user_from_row(&row)?;

    let session = NewSession::generate(state.refresh_ttl_days);
    let mut transaction = database.pool().begin().await.map_err(unavailable)?;
    session
        .insert(&mut transaction, user.id)
        .await
        .map_err(unavailable)?;
    insert_outbox_event(
        &mut transaction,
        "identity.user_logged_in.v1",
        json!({ "user_id": user.id, "session_id": session.id }),
        Some(context.request_id),
    )
    .await
    .map_err(unavailable)?;
    transaction.commit().await.map_err(unavailable)?;

    let access = state.auth.issue(user.id, session.id)?;
    Ok(Json(TokenResponse {
        user,
        access_token: access.token,
        refresh_token: session.token,
        token_type: TOKEN_TYPE,
        expires_in: access.expires_in,
    }))
}

async fn refresh(
    State(state): State<IdentityState>,
    Json(input): Json<RefreshRequest>,
) -> Result<Json<TokenResponse>, IdentityError> {
    let database = require_database(&state)?;
    let presented_hash = hash_refresh_token(&input.refresh_token);
    let mut transaction = database.pool().begin().await.map_err(unavailable)?;
    let row = sqlx::query(
        "SELECT s.id AS session_id, u.id, u.email, u.display_name,
                u.email_verified_at, u.created_at
         FROM identity.sessions s
         JOIN identity.users u ON u.id = s.user_id
         WHERE s.refresh_token_hash = $1
           AND s.revoked_at IS NULL
           AND s.expires_at > now()
           AND u.disabled_at IS NULL
         FOR UPDATE OF s",
    )
    .bind(&presented_hash)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(unavailable)?
    .ok_or(IdentityError::Unauthorized)?;

    let session_id: Uuid = row.try_get("session_id").map_err(unavailable)?;
    let user = user_from_row(&row)?;

    let rotated = NewSession::generate(state.refresh_ttl_days);
    sqlx::query(
        "UPDATE identity.sessions
         SET refresh_token_hash = $1, expires_at = $2, last_used_at = now()
         WHERE id = $3",
    )
    .bind(&rotated.token_hash)
    .bind(rotated.expires_at)
    .bind(session_id)
    .execute(&mut *transaction)
    .await
    .map_err(unavailable)?;
    transaction.commit().await.map_err(unavailable)?;

    let access = state.auth.issue(user.id, session_id)?;
    Ok(Json(TokenResponse {
        user,
        access_token: access.token,
        refresh_token: rotated.token,
        token_type: TOKEN_TYPE,
        expires_in: access.expires_in,
    }))
}

async fn logout(
    State(state): State<IdentityState>,
    Extension(context): Extension<RequestContext>,
    Json(input): Json<RefreshRequest>,
) -> Result<StatusCode, IdentityError> {
    let database = require_database(&state)?;
    let presented_hash = hash_refresh_token(&input.refresh_token);
    let mut transaction = database.pool().begin().await.map_err(unavailable)?;
    let revoked = sqlx::query(
        "UPDATE identity.sessions
         SET revoked_at = now()
         WHERE refresh_token_hash = $1 AND revoked_at IS NULL
         RETURNING id, user_id",
    )
    .bind(&presented_hash)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(unavailable)?;
    if let Some(row) = revoked {
        let session_id: Uuid = row.try_get("id").map_err(unavailable)?;
        let user_id: Uuid = row.try_get("user_id").map_err(unavailable)?;
        insert_outbox_event(
            &mut transaction,
            "identity.session_revoked.v1",
            json!({ "session_id": session_id, "user_id": user_id }),
            Some(context.request_id),
        )
        .await
        .map_err(unavailable)?;
    }
    transaction.commit().await.map_err(unavailable)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn me(
    State(state): State<IdentityState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<UserDto>, IdentityError> {
    let database = require_database(&state)?;
    let row = sqlx::query(
        "SELECT id, email, display_name, email_verified_at, created_at
         FROM identity.users
         WHERE id = $1 AND disabled_at IS NULL",
    )
    .bind(user.user_id)
    .fetch_optional(database.pool())
    .await
    .map_err(unavailable)?
    .ok_or(IdentityError::Unauthorized)?;
    Ok(Json(user_from_row(&row)?))
}

async fn request_email_verification(
    State(state): State<IdentityState>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(context): Extension<RequestContext>,
) -> Result<StatusCode, IdentityError> {
    let database = require_database(&state)?;
    let row = sqlx::query(
        "SELECT email, email_verified_at
         FROM identity.users
         WHERE id = $1 AND disabled_at IS NULL",
    )
    .bind(user.user_id)
    .fetch_optional(database.pool())
    .await
    .map_err(unavailable)?
    .ok_or(IdentityError::Unauthorized)?;
    if row
        .try_get::<Option<DateTime<Utc>>, _>("email_verified_at")
        .map_err(unavailable)?
        .is_some()
    {
        return Ok(StatusCode::NO_CONTENT);
    }
    let email: String = row.try_get("email").map_err(unavailable)?;
    let action = NewEmailAction::generate(
        EmailActionPurpose::VerifyEmail,
        Duration::hours(i64::try_from(state.email_verification_ttl_hours).unwrap_or(i64::MAX)),
    );
    let mut transaction = database.pool().begin().await.map_err(unavailable)?;
    action
        .insert(&mut transaction, user.user_id)
        .await
        .map_err(unavailable)?;
    insert_outbox_event(
        &mut transaction,
        "identity.email_verification_requested.v1",
        json!({
            "user_id": user.user_id,
            "email": email,
            "token": action.token,
            "expires_at": action.expires_at
        }),
        Some(context.request_id),
    )
    .await
    .map_err(unavailable)?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(StatusCode::ACCEPTED)
}

async fn confirm_email_verification(
    State(state): State<IdentityState>,
    Extension(context): Extension<RequestContext>,
    Json(input): Json<ActionTokenRequest>,
) -> Result<StatusCode, IdentityError> {
    let database = require_database(&state)?;
    let mut transaction = database.pool().begin().await.map_err(unavailable)?;
    let user_id: Uuid = sqlx::query_scalar(
        "UPDATE identity.email_action_tokens
         SET consumed_at = now()
         WHERE token_hash = $1
           AND purpose = 'verify_email'
           AND consumed_at IS NULL
           AND expires_at > now()
         RETURNING user_id",
    )
    .bind(hash_action_token(&input.token))
    .fetch_optional(&mut *transaction)
    .await
    .map_err(unavailable)?
    .ok_or(IdentityError::Unauthorized)?;
    sqlx::query(
        "UPDATE identity.users
         SET email_verified_at = COALESCE(email_verified_at, now()),
             updated_at = now()
         WHERE id = $1",
    )
    .bind(user_id)
    .execute(&mut *transaction)
    .await
    .map_err(unavailable)?;
    insert_outbox_event(
        &mut transaction,
        "identity.email_verified.v1",
        json!({ "user_id": user_id }),
        Some(context.request_id),
    )
    .await
    .map_err(unavailable)?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn request_password_reset(
    State(state): State<IdentityState>,
    Extension(context): Extension<RequestContext>,
    Json(input): Json<PasswordResetRequest>,
) -> Result<StatusCode, IdentityError> {
    let email = normalize_email(&input.email)?;
    let database = require_database(&state)?;
    let user_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM identity.users
         WHERE email = $1 AND disabled_at IS NULL",
    )
    .bind(&email)
    .fetch_optional(database.pool())
    .await
    .map_err(unavailable)?;
    // Always return Accepted to prevent account enumeration.
    let Some(user_id) = user_id else {
        return Ok(StatusCode::ACCEPTED);
    };
    let action = NewEmailAction::generate(
        EmailActionPurpose::ResetPassword,
        Duration::minutes(i64::try_from(state.password_reset_ttl_minutes).unwrap_or(i64::MAX)),
    );
    let mut transaction = database.pool().begin().await.map_err(unavailable)?;
    action
        .insert(&mut transaction, user_id)
        .await
        .map_err(unavailable)?;
    insert_outbox_event(
        &mut transaction,
        "identity.password_reset_requested.v1",
        json!({
            "user_id": user_id,
            "email": email,
            "token": action.token,
            "expires_at": action.expires_at
        }),
        Some(context.request_id),
    )
    .await
    .map_err(unavailable)?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(StatusCode::ACCEPTED)
}

async fn confirm_password_reset(
    State(state): State<IdentityState>,
    Extension(context): Extension<RequestContext>,
    Json(input): Json<ConfirmPasswordResetRequest>,
) -> Result<StatusCode, IdentityError> {
    validate_password(&input.password)?;
    let password_hash = hash_password(&input.password)?;
    let database = require_database(&state)?;
    let mut transaction = database.pool().begin().await.map_err(unavailable)?;
    let user_id: Uuid = sqlx::query_scalar(
        "UPDATE identity.email_action_tokens
         SET consumed_at = now()
         WHERE token_hash = $1
           AND purpose = 'reset_password'
           AND consumed_at IS NULL
           AND expires_at > now()
         RETURNING user_id",
    )
    .bind(hash_action_token(&input.token))
    .fetch_optional(&mut *transaction)
    .await
    .map_err(unavailable)?
    .ok_or(IdentityError::Unauthorized)?;
    sqlx::query(
        "UPDATE identity.users
         SET password_hash = $1, updated_at = now()
         WHERE id = $2 AND disabled_at IS NULL",
    )
    .bind(password_hash)
    .bind(user_id)
    .execute(&mut *transaction)
    .await
    .map_err(unavailable)?;
    sqlx::query(
        "UPDATE identity.sessions
         SET revoked_at = COALESCE(revoked_at, now())
         WHERE user_id = $1",
    )
    .bind(user_id)
    .execute(&mut *transaction)
    .await
    .map_err(unavailable)?;
    insert_outbox_event(
        &mut transaction,
        "identity.password_reset.v1",
        json!({ "user_id": user_id }),
        Some(context.request_id),
    )
    .await
    .map_err(unavailable)?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn require_user(
    State(state): State<IdentityState>,
    request: Request,
    next: Next,
) -> Response {
    let supplied = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let Some(token) = supplied else {
        return IdentityError::Unauthorized.into_response();
    };
    match state.auth.validate(token) {
        Ok(user) => {
            let mut request = request;
            request.extensions_mut().insert(user);
            next.run(request).await
        }
        Err(error) => error.into_response(),
    }
}

async fn admin_list_users(
    State(state): State<IdentityState>,
    Extension(principal): Extension<AdminPrincipal>,
) -> Result<Json<serde_json::Value>, IdentityError> {
    require_scope(&principal)?;
    let database = require_database(&state)?;
    let rows = sqlx::query(
        "SELECT id, email, display_name, email_verified_at, created_at
         FROM identity.users
         ORDER BY created_at DESC
         LIMIT 200",
    )
    .fetch_all(database.pool())
    .await
    .map_err(unavailable)?;
    let users = rows
        .iter()
        .map(user_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({ "data": users })))
}

async fn admin_list_sessions(
    State(state): State<IdentityState>,
    Extension(principal): Extension<AdminPrincipal>,
) -> Result<Json<serde_json::Value>, IdentityError> {
    require_scope(&principal)?;
    let database = require_database(&state)?;
    let rows = sqlx::query(
        "SELECT id, user_id, expires_at, revoked_at, last_used_at, created_at
         FROM identity.sessions
         ORDER BY created_at DESC
         LIMIT 200",
    )
    .fetch_all(database.pool())
    .await
    .map_err(unavailable)?;
    let sessions = rows
        .iter()
        .map(session_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({ "data": sessions })))
}

fn require_scope(principal: &AdminPrincipal) -> Result<(), IdentityError> {
    if principal.allows("identity:read") || principal.allows("admins:read") {
        Ok(())
    } else {
        Err(IdentityError::Forbidden("identity:read"))
    }
}

struct NewSession {
    id: Uuid,
    token: String,
    token_hash: String,
    expires_at: DateTime<Utc>,
}

impl NewSession {
    fn generate(refresh_ttl_days: u64) -> Self {
        let token = generate_refresh_token();
        let token_hash = hash_refresh_token(&token);
        let ttl_days = i64::try_from(refresh_ttl_days).unwrap_or(30);
        Self {
            id: Uuid::now_v7(),
            token,
            token_hash,
            expires_at: Utc::now() + Duration::days(ttl_days),
        }
    }

    async fn insert(
        &self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: Uuid,
    ) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
        sqlx::query(
            "INSERT INTO identity.sessions
             (id, user_id, refresh_token_hash, expires_at)
             VALUES ($1, $2, $3, $4)",
        )
        .bind(self.id)
        .bind(user_id)
        .bind(&self.token_hash)
        .bind(self.expires_at)
        .execute(&mut **transaction)
        .await
    }
}

fn normalize_email(email: &str) -> Result<String, IdentityError> {
    let email = email.trim().to_lowercase();
    let invalid = || IdentityError::Invalid("valid email is required".into());
    if email.is_empty() || email.len() > 320 || email.chars().any(char::is_whitespace) {
        return Err(invalid());
    }
    let (local, domain) = email.split_once('@').ok_or_else(invalid)?;
    if local.is_empty() || domain.is_empty() || !domain.contains('.') || domain.contains('@') {
        return Err(invalid());
    }
    Ok(email)
}

fn validate_password(password: &str) -> Result<(), IdentityError> {
    let length = password.chars().count();
    if length < MIN_PASSWORD_CHARS {
        return Err(IdentityError::Invalid(format!(
            "password must be at least {MIN_PASSWORD_CHARS} characters"
        )));
    }
    if length > MAX_PASSWORD_CHARS {
        return Err(IdentityError::Invalid(format!(
            "password must be at most {MAX_PASSWORD_CHARS} characters"
        )));
    }
    Ok(())
}

fn hash_password(password: &str) -> Result<String, IdentityError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| IdentityError::Unavailable(error.to_string()))
}

fn verify_password(password: &str, password_hash: &str) -> bool {
    PasswordHash::new(password_hash).is_ok_and(|parsed| {
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    })
}

fn generate_refresh_token() -> String {
    format!(
        "boson_rt_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

fn hash_refresh_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn hash_action_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn require_database(state: &IdentityState) -> Result<&Database, IdentityError> {
    state
        .database
        .as_ref()
        .ok_or_else(|| IdentityError::Unavailable("PostgreSQL is disabled".into()))
}

#[allow(clippy::needless_pass_by_value)]
fn unavailable(error: impl ToString) -> IdentityError {
    IdentityError::Unavailable(error.to_string())
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .is_some_and(|code| code == "23505")
}

fn user_from_row(row: &sqlx::postgres::PgRow) -> Result<UserDto, IdentityError> {
    Ok(UserDto {
        id: row.try_get("id").map_err(unavailable)?,
        email: row.try_get("email").map_err(unavailable)?,
        display_name: row.try_get("display_name").map_err(unavailable)?,
        email_verified_at: row.try_get("email_verified_at").map_err(unavailable)?,
        created_at: row.try_get("created_at").map_err(unavailable)?,
    })
}

fn session_from_row(row: &sqlx::postgres::PgRow) -> Result<SessionDto, IdentityError> {
    Ok(SessionDto {
        id: row.try_get("id").map_err(unavailable)?,
        user_id: row.try_get("user_id").map_err(unavailable)?,
        expires_at: row.try_get("expires_at").map_err(unavailable)?,
        revoked_at: row.try_get("revoked_at").map_err(unavailable)?,
        last_used_at: row.try_get("last_used_at").map_err(unavailable)?,
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

    fn test_auth(secret: &str) -> IdentityAuth {
        IdentityAuth::new(&AuthConfig {
            issuer: "boson-test".into(),
            jwt_secret: secret.into(),
            access_ttl_seconds: 900,
            refresh_ttl_days: 30,
            email_verification_ttl_hours: 24,
            password_reset_ttl_minutes: 60,
        })
    }

    #[test]
    fn email_is_normalized_to_lowercase() {
        assert_eq!(
            normalize_email("  Person@Example.COM ").unwrap(),
            "person@example.com"
        );
    }

    #[test]
    fn invalid_emails_are_rejected() {
        for email in [
            "",
            "no-at-sign",
            "@example.com",
            "user@",
            "user@nodot",
            "a b@example.com",
        ] {
            assert!(normalize_email(email).is_err(), "accepted `{email}`");
        }
    }

    #[test]
    fn short_passwords_are_rejected() {
        assert!(validate_password("九文字だけです密码").is_err());
        assert!(validate_password("exactly10!").is_ok());
    }

    #[test]
    fn password_hash_verifies_and_is_not_plaintext() {
        let hash = hash_password("correct horse battery").unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password("correct horse battery", &hash));
        assert!(!verify_password("wrong password!", &hash));
    }

    #[test]
    fn password_salts_are_random() {
        let first = hash_password("correct horse battery").unwrap();
        let second = hash_password("correct horse battery").unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn refresh_token_is_stored_only_as_hash() {
        let session = NewSession::generate(30);
        assert!(session.token.starts_with("boson_rt_"));
        assert_ne!(session.token, session.token_hash);
        assert_eq!(session.token_hash, hash_refresh_token(&session.token));
        assert!(session.expires_at > Utc::now());
    }

    #[test]
    fn access_token_roundtrip() {
        let auth = test_auth("test-secret");
        let user_id = Uuid::now_v7();
        let session_id = Uuid::now_v7();
        let issued = auth.issue(user_id, session_id).unwrap();
        assert_eq!(issued.expires_in, 900);
        let validated = auth.validate(&issued.token).unwrap();
        assert_eq!(validated.user_id, user_id);
        assert_eq!(validated.session_id, session_id);
    }

    #[test]
    fn token_from_wrong_issuer_or_secret_is_rejected() {
        let auth = test_auth("test-secret");
        let issued = auth.issue(Uuid::now_v7(), Uuid::now_v7()).unwrap();
        assert!(matches!(
            test_auth("other-secret").validate(&issued.token),
            Err(IdentityError::Unauthorized)
        ));

        let other_issuer = IdentityAuth::new(&AuthConfig {
            issuer: "someone-else".into(),
            jwt_secret: "test-secret".into(),
            ..AuthConfig::default()
        });
        let foreign = other_issuer.issue(Uuid::now_v7(), Uuid::now_v7()).unwrap();
        assert!(matches!(
            auth.validate(&foreign.token),
            Err(IdentityError::Unauthorized)
        ));
    }

    #[test]
    fn missing_secret_fails_closed() {
        let auth = test_auth("");
        assert!(matches!(
            auth.issue(Uuid::now_v7(), Uuid::now_v7()),
            Err(IdentityError::Unavailable(_))
        ));
        assert!(matches!(
            auth.validate("anything"),
            Err(IdentityError::Unavailable(_))
        ));
    }
}
