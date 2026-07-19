use super::repo::{
    AdminKeyRepository, ApiKeyRepository, CredentialRepository, OAuthClientRepository,
    OAuthConsentRepository, OrganizationRepository, OutboxRepository, ServiceAccountRepository,
    SuperAdminRepository, UserRepository,
};
use crate::backend::Pool;
use crate::db::DbStore;
use crate::ent::{
    AdminKey, ApiKey, ApiKeyAuth, AuditOperation, AuditResource, AuditScopeSelector, Credential,
    CredentialHistory, CredentialType, OAuthClient, OAuthConsent, OrgMember, OrgMembership,
    Organization, Profile, ServiceAccount, SuperAdmin, TrustedAuditContext, User,
    ValidatedAuditEvent,
};
use crate::repo::ApiKeyAuditRecord;
use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value as JsonValue;

#[cfg(feature = "postgres")]
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations/postgres");

#[cfg(feature = "sqlite")]
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations/sqlite");

#[derive(Clone)]
pub struct Service {
    pool: Pool,
    admin_key: AdminKeyRepository,
    user: UserRepository,
    credential: CredentialRepository,
    oauth_client: OAuthClientRepository,
    oauth_consent: OAuthConsentRepository,
    api_key: ApiKeyRepository,
    outbox: OutboxRepository,
    service_account: ServiceAccountRepository,
    organization: OrganizationRepository,
    super_admin: SuperAdminRepository,
}

impl Service {
    pub fn new(pool: Pool) -> Self {
        Self {
            pool,
            admin_key: AdminKeyRepository::new(),
            user: UserRepository::new(),
            credential: CredentialRepository::new(),
            oauth_client: OAuthClientRepository::new(),
            oauth_consent: OAuthConsentRepository::new(),
            api_key: ApiKeyRepository::new(),
            outbox: OutboxRepository::new(),
            service_account: ServiceAccountRepository::new(),
            organization: OrganizationRepository::new(),
            super_admin: SuperAdminRepository::new(),
        }
    }

    pub async fn migrate(&self) -> Result<()> {
        MIGRATOR.run(&self.pool).await?;
        Ok(())
    }

    /// Lightweight connectivity check — executes `SELECT 1`.
    pub async fn ping(&self) -> Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }
}

#[derive(Serialize)]
struct UserAuditSnapshot<'a> {
    email: &'a str,
    nickname: &'a str,
}

impl<'a> From<&'a User> for UserAuditSnapshot<'a> {
    fn from(user: &'a User) -> Self {
        Self {
            email: &user.email,
            nickname: &user.nickname,
        }
    }
}

#[derive(Serialize)]
struct SuperAdminAuditSnapshot<'a> {
    user_id: &'a str,
    active: bool,
}

impl<'a> From<&'a SuperAdmin> for SuperAdminAuditSnapshot<'a> {
    fn from(super_admin: &'a SuperAdmin) -> Self {
        Self {
            user_id: &super_admin.user_id,
            active: super_admin.active,
        }
    }
}

#[derive(Serialize)]
struct OrganizationAuditSnapshot<'a> {
    name: &'a str,
}

impl<'a> From<&'a Organization> for OrganizationAuditSnapshot<'a> {
    fn from(organization: &'a Organization) -> Self {
        Self {
            name: &organization.name,
        }
    }
}

#[derive(Serialize)]
struct OrganizationMembershipAuditSnapshot<'a> {
    organization_id: &'a str,
    user_id: &'a str,
    role: &'a str,
}

#[derive(Serialize)]
struct OAuthClientAuditSnapshot<'a> {
    name: &'a str,
    enabled: bool,
    token_endpoint_auth_method: &'a str,
    grant_types: &'a [String],
    response_types: &'a [String],
    redirect_uris: &'a [String],
    scopes: &'a [String],
    audiences: &'a [String],
}

impl<'a> From<&'a OAuthClient> for OAuthClientAuditSnapshot<'a> {
    fn from(client: &'a OAuthClient) -> Self {
        Self {
            name: &client.name,
            enabled: client.enabled,
            token_endpoint_auth_method: &client.token_endpoint_auth_method,
            grant_types: client.grant_types.as_slice(),
            response_types: client.response_types.as_slice(),
            redirect_uris: client.redirect_uris.as_slice(),
            scopes: client.scopes.as_slice(),
            audiences: client.audiences.as_slice(),
        }
    }
}

#[derive(Serialize)]
struct ApiKeyAuditSnapshot<'a> {
    label: &'a str,
    revoked: bool,
    owner_type: &'a str,
    owner_id: &'a str,
    organization_id: Option<&'a str>,
}

impl<'a> From<&'a ApiKeyAuditRecord> for ApiKeyAuditSnapshot<'a> {
    fn from(record: &'a ApiKeyAuditRecord) -> Self {
        Self {
            label: &record.api_key.label,
            revoked: record.api_key.revoked,
            owner_type: &record.owner_type,
            owner_id: &record.owner_id,
            organization_id: record.org_id.as_deref(),
        }
    }
}

#[derive(Serialize)]
struct AdminKeyAuditSnapshot<'a> {
    label: Option<&'a str>,
    permissions: &'a [String],
    revoked: bool,
}

impl<'a> From<&'a AdminKey> for AdminKeyAuditSnapshot<'a> {
    fn from(admin_key: &'a AdminKey) -> Self {
        Self {
            label: admin_key.label.as_deref(),
            permissions: admin_key.permissions.as_slice(),
            revoked: admin_key.revoked,
        }
    }
}

#[derive(Serialize)]
struct ServiceAccountAuditSnapshot<'a> {
    name: &'a str,
    organization_id: Option<&'a str>,
}

impl<'a> From<&'a ServiceAccount> for ServiceAccountAuditSnapshot<'a> {
    fn from(account: &'a ServiceAccount) -> Self {
        Self {
            name: &account.name,
            organization_id: account.org_id.as_deref(),
        }
    }
}

fn snapshot(value: impl Serialize) -> Result<JsonValue> {
    serde_json::to_value(value).context("failed to serialize safe audit snapshot")
}

#[allow(clippy::too_many_arguments)]
fn build_outbox_event(
    context: &TrustedAuditContext,
    occurred_at: DateTime<Utc>,
    resource: AuditResource,
    action: &'static str,
    operation: AuditOperation,
    before: Option<JsonValue>,
    after: Option<JsonValue>,
    metadata: JsonValue,
) -> Result<ValidatedAuditEvent> {
    let mut builder = context
        .raw_event_builder(occurred_at)
        .resource(resource)
        .action(action)
        .operation(operation)
        .metadata(metadata);
    if let Some(before) = before {
        builder = builder.before(before);
    }
    if let Some(after) = after {
        builder = builder.after(after);
    }
    Ok(builder.build()?)
}

fn user_event_source(context: &TrustedAuditContext) -> Result<&'static str> {
    match (context.actor().actor_type(), context.boundary().scope()) {
        ("admin" | "admin_key", AuditScopeSelector::ControlPlane) => Ok("admin_api"),
        ("anonymous", AuditScopeSelector::Application) => Ok("signup"),
        ("external_identity", AuditScopeSelector::Application) => {
            let actor_id = context
                .actor()
                .id()
                .context("external identity audit actor requires an id")?;
            if actor_id.starts_with("github:") {
                Ok("github")
            } else if actor_id.starts_with("google:") {
                Ok("google")
            } else {
                bail!("unsupported external identity audit actor")
            }
        }
        _ => bail!("actor and scope are not approved for a user audit producer"),
    }
}

fn ensure_admin_control_plane(context: &TrustedAuditContext) -> Result<()> {
    ensure!(
        matches!(context.actor().actor_type(), "admin" | "admin_key")
            && matches!(context.boundary().scope(), AuditScopeSelector::ControlPlane),
        "admin producer requires an authenticated admin control-plane context"
    );
    Ok(())
}

fn api_key_event_source(context: &TrustedAuditContext) -> Result<&'static str> {
    match (context.actor().actor_type(), context.boundary().scope()) {
        ("admin" | "admin_key", AuditScopeSelector::ControlPlane) => Ok("admin_api"),
        (
            "admin" | "admin_key" | "service",
            AuditScopeSelector::Application | AuditScopeSelector::Organization { .. },
        ) => Ok("oauth_revoke"),
        _ => bail!("actor and scope are not approved for an API-key audit producer"),
    }
}

fn ensure_api_key_owner(record: &ApiKeyAuditRecord) -> Result<()> {
    ensure!(
        matches!(record.owner_type.as_str(), "user" | "service_account")
            && !record.owner_id.trim().is_empty(),
        "API key audit requires a persisted owner binding"
    );
    Ok(())
}

fn ensure_api_key_scope(
    context: &TrustedAuditContext,
    record: &ApiKeyAuditRecord,
    source: &str,
) -> Result<()> {
    if source == "admin_api" {
        return ensure_admin_control_plane(context);
    }

    match (record.org_id.as_deref(), context.boundary().scope()) {
        (None, AuditScopeSelector::Application) => Ok(()),
        (Some(persisted), AuditScopeSelector::Organization { organization_id })
            if persisted == organization_id =>
        {
            Ok(())
        }
        _ => bail!("OAuth revoke audit scope must match persisted API-key ownership"),
    }
}

fn ensure_password_change_context(context: &TrustedAuditContext) -> Result<()> {
    ensure!(
        context.actor().actor_type() == "user"
            && matches!(context.boundary().scope(), AuditScopeSelector::Application),
        "password change requires an authenticated user application context"
    );
    Ok(())
}

fn ensure_credential_rehash_context(context: &TrustedAuditContext) -> Result<()> {
    ensure!(
        context.actor().actor_type() == "system"
            && matches!(context.boundary().scope(), AuditScopeSelector::Application)
            && context.request().is_none(),
        "credential rehash requires a background system application context"
    );
    Ok(())
}

fn ensure_super_admin_bootstrap_context(context: &TrustedAuditContext) -> Result<()> {
    ensure!(
        context.actor().actor_type() == "system"
            && matches!(context.boundary().scope(), AuditScopeSelector::ControlPlane)
            && context.request().is_none(),
        "super-admin bootstrap requires a background system control-plane context"
    );
    Ok(())
}

fn user_changed_fields(before: &User, after: &User) -> Vec<&'static str> {
    let mut fields = Vec::new();
    if before.email != after.email {
        fields.push("email");
    }
    if before.nickname != after.nickname {
        fields.push("nickname");
    }
    if before.given_name != after.given_name {
        fields.push("given_name");
    }
    if before.family_name != after.family_name {
        fields.push("family_name");
    }
    if before.picture != after.picture {
        fields.push("picture");
    }
    if before.phone_number != after.phone_number {
        fields.push("phone_number");
    }
    if before.attrs != after.attrs {
        fields.push("attrs");
    }
    fields
}

fn organization_changed_fields(before: &Organization, after: &Organization) -> Vec<&'static str> {
    let mut fields = Vec::new();
    if before.name != after.name {
        fields.push("name");
    }
    if before.description != after.description {
        fields.push("description");
    }
    if before.attrs != after.attrs {
        fields.push("attrs");
    }
    fields
}

fn oauth_client_changed_fields(before: &OAuthClient, after: &OAuthClient) -> Vec<&'static str> {
    let mut fields = Vec::new();
    if before.name != after.name {
        fields.push("name");
    }
    if before.description != after.description {
        fields.push("description");
    }
    if before.token_endpoint_auth_method != after.token_endpoint_auth_method {
        fields.push("token_endpoint_auth_method");
    }
    if before.grant_types != after.grant_types {
        fields.push("grant_types");
    }
    if before.response_types != after.response_types {
        fields.push("response_types");
    }
    if before.redirect_uris != after.redirect_uris {
        fields.push("redirect_uris");
    }
    if before.scopes != after.scopes {
        fields.push("scopes");
    }
    if before.audiences != after.audiences {
        fields.push("audiences");
    }
    if before.attrs != after.attrs {
        fields.push("attrs");
    }
    fields
}

fn service_account_changed_fields(
    before: &ServiceAccount,
    after: &ServiceAccount,
) -> Vec<&'static str> {
    let mut fields = Vec::new();
    if before.name != after.name {
        fields.push("name");
    }
    if before.description != after.description {
        fields.push("description");
    }
    fields
}

/// A batch of audit outbox rows claimed for relaying, holding the open
/// transaction that locks them via `FOR UPDATE SKIP LOCKED`.
///
/// Dropping the claim without calling [`OutboxClaim::commit_published`] rolls the
/// transaction back and releases the rows, so a later attempt — this node or
/// another — can re-claim them. That is exactly the desired behaviour when the
/// broker publish fails: nothing is marked published, nothing is lost.
#[cfg(feature = "postgres")]
pub struct OutboxClaim {
    tx: crate::backend::Tx<'static>,
    rows: Vec<crate::ent::OutboxEventRow>,
}

#[cfg(feature = "postgres")]
impl OutboxClaim {
    /// The claimed rows, ordered by ascending `seq`.
    pub fn rows(&self) -> &[crate::ent::OutboxEventRow] {
        &self.rows
    }

    /// Whether no unpublished rows were available to claim.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Mark every claimed row as published and commit the transaction. Call this
    /// only once the rows have been durably handed to the broker.
    pub async fn commit_published(mut self) -> Result<()> {
        let seqs: Vec<i64> = self.rows.iter().map(|row| row.seq).collect();
        OutboxRepository::new()
            .mark_published(&mut self.tx, &seqs)
            .await?;
        self.tx.commit().await?;
        Ok(())
    }
}

#[cfg(feature = "postgres")]
impl Service {
    /// Claim up to `limit` unpublished audit outbox rows for relaying.
    ///
    /// The returned [`OutboxClaim`] owns an open transaction that locks the rows;
    /// ship them to the broker, then call [`OutboxClaim::commit_published`], or
    /// drop the claim to release them.
    pub async fn claim_unpublished_outbox_events(&self, limit: i64) -> Result<OutboxClaim> {
        let mut tx = self.pool.begin().await?;
        let rows = self.outbox.claim_unpublished(&mut tx, limit).await?;
        Ok(OutboxClaim { tx, rows })
    }
}

/// Connect to the `postgres` maintenance database and create the target
/// database if it does not already exist.
#[cfg(feature = "postgres")]
pub async fn ensure_database(user: &str, password: &str, host: &str, database: &str) {
    let maintenance_url = format!("postgres://{}:{}@{}/postgres", user, password, host);
    let pool = match sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&maintenance_url)
        .await
    {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Cannot connect to PostgreSQL server: {}", e);
            return;
        }
    };

    // Check if the database exists (parameterized query to avoid injection)
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)")
            .bind(database)
            .fetch_one(&pool)
            .await
            .unwrap_or(false);

    if !exists {
        // CREATE DATABASE cannot use bind parameters, but `database` comes
        // from an env var set by the operator, not from user input.
        let stmt = format!("CREATE DATABASE \"{}\"", database);
        match sqlx::query(sqlx::AssertSqlSafe(stmt)).execute(&pool).await {
            Ok(_) => eprintln!("Created database '{}'", database),
            Err(e) => eprintln!("Failed to create database '{}': {}", database, e),
        }
    }

    pool.close().await;
}

// Generic init function that works with any database URL
pub async fn init(conn: &str) -> Result<Service> {
    let max_connections: u32 = std::env::var("DB_POOL_MAX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);

    #[cfg(feature = "sqlite")]
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(max_connections)
        .connect(conn)
        .await?;

    #[cfg(feature = "postgres")]
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(conn)
        .await?;

    let service = Service::new(pool);

    service.migrate().await?;

    Ok(service)
}

#[async_trait::async_trait]
impl DbStore for Service {
    // ── Users ───────────────────────────────────────────────────────────────

    async fn create_user(
        &self,
        profile: Profile,
        credential_type: CredentialType,
        value: &str,
        context: TrustedAuditContext,
    ) -> Result<User> {
        let source = user_event_source(&context)?;
        let mut tx = self.pool.begin().await?;

        let user = User::new(profile.email, profile.nickname)
            .given_name(profile.given_name)
            .family_name(profile.family_name)
            .picture(profile.picture)
            .phone_number(profile.phone_number)
            .attrs(profile.attrs);

        let user_id = user.id.clone();
        let record = self.user.create(&mut tx, user).await?;
        self.credential
            .create(&mut tx, &user_id, credential_type, value)
            .await?;
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("user", &record.id),
            "user.created",
            AuditOperation::Create,
            None,
            Some(snapshot(UserAuditSnapshot::from(&record))?),
            serde_json::json!({ "source": source }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(record)
    }

    async fn create_super_admin_user(
        &self,
        profile: Profile,
        credential_type: CredentialType,
        value: &str,
        context: TrustedAuditContext,
    ) -> Result<User> {
        ensure_super_admin_bootstrap_context(&context)?;
        let mut tx = self.pool.begin().await?;

        let user = User::new(profile.email, profile.nickname)
            .given_name(profile.given_name)
            .family_name(profile.family_name)
            .picture(profile.picture)
            .phone_number(profile.phone_number)
            .attrs(profile.attrs);

        let user_id = user.id.clone();
        let record = self.user.create(&mut tx, user).await?;
        self.credential
            .create(&mut tx, &user_id, credential_type, value)
            .await?;
        let super_admin = self.super_admin.create(&mut tx, &user_id).await?;
        let occurred_at = Utc::now();
        let user_event = build_outbox_event(
            &context,
            occurred_at,
            AuditResource::new("user", &record.id),
            "user.created",
            AuditOperation::Create,
            None,
            Some(snapshot(UserAuditSnapshot::from(&record))?),
            serde_json::json!({ "source": "cli_bootstrap" }),
        )?;
        let super_admin_event = build_outbox_event(
            &context,
            occurred_at,
            AuditResource::new("super_admin", &record.id),
            "super_admin.granted",
            AuditOperation::Create,
            None,
            Some(snapshot(SuperAdminAuditSnapshot::from(&super_admin))?),
            serde_json::json!({ "source": "cli_bootstrap" }),
        )?;
        self.outbox.insert(&mut tx, &user_event, None).await?;
        self.outbox
            .insert(&mut tx, &super_admin_event, None)
            .await?;
        tx.commit().await?;
        Ok(record)
    }

    async fn get_user_by_username(&self, username: &str) -> Result<Option<User>> {
        self.user.get_by_username(&self.pool, username).await
    }

    async fn get_user_by_id(&self, id: &str) -> Result<Option<User>> {
        self.user.get_by_id(&self.pool, id).await
    }

    async fn get_all_users(&self, limit: i64, offset: i64) -> Result<Vec<User>> {
        self.user.get_all(&self.pool, limit, offset).await
    }

    async fn count_users(&self) -> Result<i64> {
        self.user.count(&self.pool).await
    }

    async fn get_super_admin_users(&self) -> Result<Vec<User>> {
        self.super_admin.get_active_users(&self.pool).await
    }

    async fn get_super_admin_by_user_id(&self, user_id: &str) -> Result<Option<SuperAdmin>> {
        self.super_admin.get_by_user_id(&self.pool, user_id).await
    }

    async fn super_admin_exists(&self) -> Result<bool> {
        Ok(self.super_admin.count_active(&self.pool).await? > 0)
    }

    async fn search_users(&self, query: &str, limit: i64, offset: i64) -> Result<Vec<User>> {
        self.user
            .search_with_pagination(&self.pool, query, limit, offset)
            .await
    }

    async fn count_search_users(&self, query: &str) -> Result<i64> {
        self.user.count_search(&self.pool, query).await
    }

    async fn update_user(&self, user: User, context: TrustedAuditContext) -> Result<User> {
        let source = user_event_source(&context)?;
        let mut tx = self.pool.begin().await?;
        let before = self
            .user
            .get_by_id_for_update(&mut tx, &user.id)
            .await?
            .context("cannot audit update for a missing user")?;
        let updated_user = self.user.update(&mut tx, user).await?;
        let changed_fields = user_changed_fields(&before, &updated_user);
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("user", &updated_user.id),
            "user.updated",
            AuditOperation::Update,
            Some(snapshot(UserAuditSnapshot::from(&before))?),
            Some(snapshot(UserAuditSnapshot::from(&updated_user))?),
            serde_json::json!({
                "source": source,
                "changed_fields": changed_fields,
            }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(updated_user)
    }

    async fn delete_user(&self, id: &str, context: TrustedAuditContext) -> Result<Vec<String>> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        let before = self
            .user
            .get_by_id_for_update(&mut tx, id)
            .await?
            .context("cannot audit deletion of a missing user")?;
        // Revoke before the delete: the cascade only removes the ownership
        // link rows, so unrevoked api_keys rows would keep authenticating.
        let revoked_key_hashes = self.api_key.revoke_by_user(&mut tx, id).await?;
        self.user.delete(&mut tx, id).await?;
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("user", id),
            "user.deleted",
            AuditOperation::Delete,
            Some(snapshot(UserAuditSnapshot::from(&before))?),
            None,
            serde_json::json!({
                "source": "admin_api",
                "revoked_api_key_count": revoked_key_hashes.len(),
            }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(revoked_key_hashes)
    }

    async fn change_password(
        &self,
        user_id: &str,
        new_password: &str,
        context: TrustedAuditContext,
    ) -> Result<Credential> {
        ensure_password_change_context(&context)?;
        let mut tx = self.pool.begin().await?;
        // Keep the replaced hash so the `not recently used` password policy
        // can check against it; committed atomically with the change.
        if let Some(previous) = self
            .credential
            .get_by_user_id(&mut *tx, user_id, CredentialType::Password)
            .await?
        {
            self.credential
                .insert_history(&mut tx, user_id, &previous.value)
                .await?;
            self.credential
                .prune_history(&mut tx, user_id, crate::repo::CREDENTIAL_HISTORY_KEEP)
                .await?;
        }
        let credential = self
            .credential
            .change_password(&mut tx, user_id, new_password)
            .await?;
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("credential", user_id),
            "credential.password_changed",
            AuditOperation::Update,
            None,
            None,
            serde_json::json!({
                "source": "password_reset",
                "credential_type": "password",
            }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(credential)
    }

    async fn get_credential(
        &self,
        user_id: &str,
        credential_type: CredentialType,
    ) -> Result<Option<Credential>> {
        self.credential
            .get_by_user_id(&self.pool, user_id, credential_type)
            .await
    }

    async fn get_credential_history(
        &self,
        user_id: &str,
        limit: i64,
    ) -> Result<Vec<CredentialHistory>> {
        self.credential
            .get_history(&self.pool, user_id, limit)
            .await
    }

    async fn rehash_credential(
        &self,
        user_id: &str,
        old_value: &str,
        new_value: &str,
        context: TrustedAuditContext,
    ) -> Result<bool> {
        ensure_credential_rehash_context(&context)?;
        let mut tx = self.pool.begin().await?;
        let updated = self
            .credential
            .rehash_value(&mut tx, user_id, old_value, new_value)
            .await?;
        if updated {
            let event = build_outbox_event(
                &context,
                Utc::now(),
                AuditResource::new("credential", user_id),
                "credential.rehashed",
                AuditOperation::Update,
                None,
                None,
                serde_json::json!({
                    "credential_type": "password",
                    "trigger": "login",
                }),
            )?;
            self.outbox.insert(&mut tx, &event, None).await?;
        }
        tx.commit().await?;
        Ok(updated)
    }

    // ── OAuth Clients ──────────────────────────────────────────────────────

    async fn create_oauth_client(
        &self,
        client: OAuthClient,
        context: TrustedAuditContext,
    ) -> Result<OAuthClient> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        let client = self.oauth_client.create(&mut tx, client).await?;
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("oauth_client", &client.client_id),
            "oauth_client.created",
            AuditOperation::Create,
            None,
            Some(snapshot(OAuthClientAuditSnapshot::from(&client))?),
            serde_json::json!({ "source": "admin_api" }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(client)
    }

    async fn get_oauth_client_by_client_id(&self, client_id: &str) -> Result<Option<OAuthClient>> {
        self.oauth_client
            .get_by_client_id(&self.pool, client_id)
            .await
    }

    async fn get_all_oauth_clients(&self, limit: i64, offset: i64) -> Result<Vec<OAuthClient>> {
        self.oauth_client.get_all(&self.pool, limit, offset).await
    }

    async fn count_oauth_clients(&self) -> Result<i64> {
        self.oauth_client.count(&self.pool).await
    }

    async fn search_oauth_clients(
        &self,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<OAuthClient>> {
        self.oauth_client
            .search(&self.pool, query, limit, offset)
            .await
    }

    async fn count_search_oauth_clients(&self, query: &str) -> Result<i64> {
        self.oauth_client.count_search(&self.pool, query).await
    }

    async fn update_oauth_client(
        &self,
        client: OAuthClient,
        context: TrustedAuditContext,
    ) -> Result<OAuthClient> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        let before = self
            .oauth_client
            .get_by_client_id_for_update(&mut tx, &client.client_id)
            .await?
            .context("OAuth client not found")?;
        let client = self.oauth_client.update(&mut tx, client).await?;
        let changed_fields = oauth_client_changed_fields(&before, &client);
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("oauth_client", &client.client_id),
            "oauth_client.updated",
            AuditOperation::Update,
            Some(snapshot(OAuthClientAuditSnapshot::from(&before))?),
            Some(snapshot(OAuthClientAuditSnapshot::from(&client))?),
            serde_json::json!({
                "changed_fields": changed_fields,
                "source": "admin_api",
            }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(client)
    }

    async fn update_oauth_client_secret_hash(
        &self,
        client_id: &str,
        client_secret_hash: Option<&str>,
        context: TrustedAuditContext,
    ) -> Result<OAuthClient> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        self.oauth_client
            .get_by_client_id_for_update(&mut tx, client_id)
            .await?
            .context("OAuth client not found")?;
        let client = self
            .oauth_client
            .update_secret_hash(&mut tx, client_id, client_secret_hash)
            .await?;
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("oauth_client", &client.client_id),
            "oauth_client.secret_rotated",
            AuditOperation::Update,
            None,
            None,
            serde_json::json!({ "source": "admin_api" }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(client)
    }

    async fn set_oauth_client_enabled(
        &self,
        client_id: &str,
        enabled: bool,
        context: TrustedAuditContext,
    ) -> Result<OAuthClient> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        let before = self
            .oauth_client
            .get_by_client_id_for_update(&mut tx, client_id)
            .await?
            .context("OAuth client not found")?;
        let client = self
            .oauth_client
            .set_enabled(&mut tx, client_id, enabled)
            .await?;
        let action = if enabled {
            "oauth_client.enabled"
        } else {
            "oauth_client.disabled"
        };
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("oauth_client", &client.client_id),
            action,
            AuditOperation::Update,
            Some(snapshot(OAuthClientAuditSnapshot::from(&before))?),
            Some(snapshot(OAuthClientAuditSnapshot::from(&client))?),
            serde_json::json!({ "source": "admin_api" }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(client)
    }

    async fn delete_oauth_client(
        &self,
        client_id: &str,
        context: TrustedAuditContext,
    ) -> Result<()> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        let before = self
            .oauth_client
            .get_by_client_id_for_update(&mut tx, client_id)
            .await?
            .context("OAuth client not found")?;
        self.oauth_client
            .delete_by_client_id(&mut tx, client_id)
            .await?;
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("oauth_client", client_id),
            "oauth_client.deleted",
            AuditOperation::Delete,
            Some(snapshot(OAuthClientAuditSnapshot::from(&before))?),
            None,
            serde_json::json!({ "source": "admin_api" }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(())
    }

    // ── OAuth Consents ─────────────────────────────────────────────────────

    async fn upsert_oauth_consent(&self, consent: OAuthConsent) -> Result<OAuthConsent> {
        let mut tx = self.pool.begin().await?;
        let consent = self.oauth_consent.upsert(&mut tx, consent).await?;
        tx.commit().await?;
        Ok(consent)
    }

    async fn get_active_oauth_consent(
        &self,
        user_id: &str,
        client_id: &str,
    ) -> Result<Option<OAuthConsent>> {
        self.oauth_consent
            .get_active_for_user_client(&self.pool, user_id, client_id, chrono::Utc::now())
            .await
    }

    async fn revoke_oauth_consent(&self, user_id: &str, client_id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.oauth_consent
            .revoke_for_user_client(&mut tx, user_id, client_id)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    // ── API Keys ────────────────────────────────────────────────────────────

    async fn create_api_key(
        &self,
        key_hash: &str,
        label: &str,
        attrs: Option<JsonValue>,
    ) -> Result<ApiKey> {
        let mut tx = self.pool.begin().await?;
        let api_key = self.api_key.create(&mut tx, key_hash, label, attrs).await?;
        tx.commit().await?;
        Ok(api_key)
    }

    async fn create_user_api_key(
        &self,
        user_id: &str,
        key_hash: &str,
        label: &str,
        attrs: Option<JsonValue>,
        org_id: Option<&str>,
        context: TrustedAuditContext,
    ) -> Result<ApiKey> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        let api_key = self.api_key.create(&mut tx, key_hash, label, attrs).await?;
        self.api_key
            .link_to_user(&mut tx, &api_key.id, user_id, org_id)
            .await?;
        let record = self
            .api_key
            .get_audit_by_id_for_update(&mut tx, &api_key.id)
            .await?
            .context("created API key owner binding not found")?;
        ensure_api_key_owner(&record)?;
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("api_key", &api_key.id),
            "api_key.created",
            AuditOperation::Create,
            None,
            Some(snapshot(ApiKeyAuditSnapshot::from(&record))?),
            serde_json::json!({ "source": "admin_api" }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(api_key)
    }

    async fn create_service_account_api_key(
        &self,
        service_account_id: &str,
        key_hash: &str,
        label: &str,
        attrs: Option<JsonValue>,
        context: TrustedAuditContext,
    ) -> Result<ApiKey> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        let api_key = self.api_key.create(&mut tx, key_hash, label, attrs).await?;
        self.api_key
            .link_to_service_account(&mut tx, &api_key.id, service_account_id)
            .await?;
        let record = self
            .api_key
            .get_audit_by_id_for_update(&mut tx, &api_key.id)
            .await?
            .context("created API key owner binding not found")?;
        ensure_api_key_owner(&record)?;
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("api_key", &api_key.id),
            "api_key.created",
            AuditOperation::Create,
            None,
            Some(snapshot(ApiKeyAuditSnapshot::from(&record))?),
            serde_json::json!({ "source": "admin_api" }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(api_key)
    }

    async fn get_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>> {
        self.api_key.get_by_hash(&self.pool, key_hash).await
    }

    async fn get_api_key_auth_by_hash(&self, key_hash: &str) -> Result<Option<ApiKeyAuth>> {
        self.api_key.get_auth_by_hash(&self.pool, key_hash).await
    }

    async fn get_api_key_by_id(&self, id: &str) -> Result<Option<ApiKey>> {
        self.api_key.get_by_id(&self.pool, id).await
    }

    async fn get_all_api_keys(&self, limit: i64, offset: i64) -> Result<Vec<ApiKey>> {
        self.api_key.get_all(&self.pool, limit, offset).await
    }

    async fn get_api_keys_by_user_id(&self, user_id: &str) -> Result<Vec<ApiKey>> {
        self.api_key.get_by_user_id(&self.pool, user_id).await
    }

    async fn get_all_user_api_keys(&self, limit: i64, offset: i64) -> Result<Vec<ApiKey>> {
        self.api_key
            .get_all_by_user_type(&self.pool, limit, offset)
            .await
    }

    async fn count_user_api_keys(&self) -> Result<i64> {
        self.api_key.count_by_user_type(&self.pool).await
    }

    async fn get_api_keys_by_service_account_id(
        &self,
        service_account_id: &str,
    ) -> Result<Vec<ApiKey>> {
        self.api_key
            .get_by_service_account_id(&self.pool, service_account_id)
            .await
    }

    async fn get_all_service_account_api_keys(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ApiKey>> {
        self.api_key
            .get_all_by_service_account_type(&self.pool, limit, offset)
            .await
    }

    async fn count_service_account_api_keys(&self) -> Result<i64> {
        self.api_key.count_by_service_account_type(&self.pool).await
    }

    async fn count_api_keys(&self) -> Result<i64> {
        self.api_key.count(&self.pool).await
    }

    async fn search_api_keys(&self, query: &str, limit: i64, offset: i64) -> Result<Vec<ApiKey>> {
        self.api_key.search(&self.pool, query, limit, offset).await
    }

    async fn count_search_api_keys(&self, query: &str) -> Result<i64> {
        self.api_key.count_search(&self.pool, query).await
    }

    async fn update_api_key(
        &self,
        api_key: ApiKey,
        context: TrustedAuditContext,
    ) -> Result<ApiKey> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        let before = self
            .api_key
            .get_audit_by_id_for_update(&mut tx, &api_key.id)
            .await?
            .context("API key owner binding not found")?;
        ensure_api_key_owner(&before)?;
        let updated = self.api_key.update(&mut tx, api_key).await?;
        let after = self
            .api_key
            .get_audit_by_id_for_update(&mut tx, &updated.id)
            .await?
            .context("updated API key owner binding not found")?;
        ensure_api_key_owner(&after)?;
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("api_key", &updated.id),
            "api_key.updated",
            AuditOperation::Update,
            Some(snapshot(ApiKeyAuditSnapshot::from(&before))?),
            Some(snapshot(ApiKeyAuditSnapshot::from(&after))?),
            serde_json::json!({
                "changed_fields": ["attrs"],
                "source": "admin_api",
            }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(updated)
    }

    async fn revoke_api_key(&self, id: &str, context: TrustedAuditContext) -> Result<()> {
        let source = api_key_event_source(&context)?;
        let mut tx = self.pool.begin().await?;
        let before = self
            .api_key
            .get_audit_by_id_for_update(&mut tx, id)
            .await?
            .context("API key owner binding not found")?;
        ensure_api_key_owner(&before)?;
        ensure_api_key_scope(&context, &before, source)?;
        if before.api_key.revoked {
            tx.commit().await?;
            return Ok(());
        }
        self.api_key.revoke_by_id(&mut tx, id).await?;
        let after = self
            .api_key
            .get_audit_by_id_for_update(&mut tx, id)
            .await?
            .context("revoked API key owner binding not found")?;
        ensure_api_key_owner(&after)?;
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("api_key", id),
            "api_key.revoked",
            AuditOperation::Update,
            Some(snapshot(ApiKeyAuditSnapshot::from(&before))?),
            Some(snapshot(ApiKeyAuditSnapshot::from(&after))?),
            serde_json::json!({ "source": source }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn delete_api_key(&self, id: &str, context: TrustedAuditContext) -> Result<()> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        let before = self
            .api_key
            .get_audit_by_id_for_update(&mut tx, id)
            .await?
            .context("API key owner binding not found")?;
        ensure_api_key_owner(&before)?;
        self.api_key.delete_by_id(&mut tx, id).await?;
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("api_key", id),
            "api_key.deleted",
            AuditOperation::Delete,
            Some(snapshot(ApiKeyAuditSnapshot::from(&before))?),
            None,
            serde_json::json!({ "source": "admin_api" }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(())
    }

    // ── Admin Keys ──────────────────────────────────────────────────────────

    async fn create_admin_key(
        &self,
        key_hash: &str,
        label: Option<String>,
        permissions: Vec<String>,
        context: TrustedAuditContext,
    ) -> Result<AdminKey> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        let admin_key = self
            .admin_key
            .create(&mut tx, key_hash, label, permissions)
            .await?;
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("admin_key", &admin_key.id),
            "admin_key.created",
            AuditOperation::Create,
            None,
            Some(snapshot(AdminKeyAuditSnapshot::from(&admin_key))?),
            serde_json::json!({ "source": "admin_api" }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(admin_key)
    }

    async fn get_admin_key_by_hash(&self, key_hash: &str) -> Result<Option<AdminKey>> {
        self.admin_key.get_by_hash(&self.pool, key_hash).await
    }

    async fn get_admin_key_by_id(&self, id: &str) -> Result<Option<AdminKey>> {
        self.admin_key.get_by_id(&self.pool, id).await
    }

    async fn get_all_admin_keys(&self, limit: i64, offset: i64) -> Result<Vec<AdminKey>> {
        self.admin_key.get_all(&self.pool, limit, offset).await
    }

    async fn count_admin_keys(&self) -> Result<i64> {
        self.admin_key.count(&self.pool).await
    }

    async fn update_admin_key(
        &self,
        admin_key: AdminKey,
        context: TrustedAuditContext,
    ) -> Result<AdminKey> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        let before = self
            .admin_key
            .get_by_id_for_update(&mut tx, &admin_key.id)
            .await?
            .context("admin key not found")?;
        let updated = self.admin_key.update(&mut tx, admin_key).await?;
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("admin_key", &updated.id),
            "admin_key.permissions_updated",
            AuditOperation::Update,
            Some(snapshot(AdminKeyAuditSnapshot::from(&before))?),
            Some(snapshot(AdminKeyAuditSnapshot::from(&updated))?),
            serde_json::json!({
                "changed_fields": ["permissions"],
                "source": "admin_api",
            }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(updated)
    }

    async fn revoke_admin_key(&self, id: &str, context: TrustedAuditContext) -> Result<()> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        let before = self
            .admin_key
            .get_by_id_for_update(&mut tx, id)
            .await?
            .context("admin key not found")?;
        if before.revoked {
            tx.commit().await?;
            return Ok(());
        }
        self.admin_key.revoke_by_id(&mut tx, id).await?;
        let after = self
            .admin_key
            .get_by_id_for_update(&mut tx, id)
            .await?
            .context("revoked admin key not found")?;
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("admin_key", id),
            "admin_key.revoked",
            AuditOperation::Update,
            Some(snapshot(AdminKeyAuditSnapshot::from(&before))?),
            Some(snapshot(AdminKeyAuditSnapshot::from(&after))?),
            serde_json::json!({ "source": "admin_api" }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn delete_admin_key(&self, id: &str, context: TrustedAuditContext) -> Result<()> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        let before = self
            .admin_key
            .get_by_id_for_update(&mut tx, id)
            .await?
            .context("admin key not found")?;
        self.admin_key.delete_by_id(&mut tx, id).await?;
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("admin_key", id),
            "admin_key.deleted",
            AuditOperation::Delete,
            Some(snapshot(AdminKeyAuditSnapshot::from(&before))?),
            None,
            serde_json::json!({ "source": "admin_api" }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(())
    }

    // ── Service Accounts ────────────────────────────────────────────────────

    async fn create_service_account(
        &self,
        name: &str,
        description: Option<&str>,
        org_id: Option<&str>,
        context: TrustedAuditContext,
    ) -> Result<ServiceAccount> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        let sa = self
            .service_account
            .create(&mut tx, name, description, org_id)
            .await?;
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("service_account", &sa.id),
            "service_account.created",
            AuditOperation::Create,
            None,
            Some(snapshot(ServiceAccountAuditSnapshot::from(&sa))?),
            serde_json::json!({ "source": "admin_api" }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(sa)
    }

    async fn get_service_account_by_id(&self, id: &str) -> Result<Option<ServiceAccount>> {
        self.service_account.get_by_id(&self.pool, id).await
    }

    async fn get_all_service_accounts(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ServiceAccount>> {
        self.service_account
            .get_all(&self.pool, limit, offset)
            .await
    }

    async fn count_service_accounts(&self) -> Result<i64> {
        self.service_account.count(&self.pool).await
    }

    async fn search_service_accounts(
        &self,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ServiceAccount>> {
        self.service_account
            .search(&self.pool, query, limit, offset)
            .await
    }

    async fn count_search_service_accounts(&self, query: &str) -> Result<i64> {
        self.service_account.count_search(&self.pool, query).await
    }

    async fn update_service_account(
        &self,
        id: &str,
        name: &str,
        description: Option<&str>,
        context: TrustedAuditContext,
    ) -> Result<ServiceAccount> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        let before = self
            .service_account
            .get_by_id_for_update(&mut tx, id)
            .await?
            .context("service account not found")?;
        let sa = self
            .service_account
            .update(&mut tx, id, name, description, before.org_id.as_deref())
            .await?;
        let changed_fields = service_account_changed_fields(&before, &sa);
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("service_account", &sa.id),
            "service_account.updated",
            AuditOperation::Update,
            Some(snapshot(ServiceAccountAuditSnapshot::from(&before))?),
            Some(snapshot(ServiceAccountAuditSnapshot::from(&sa))?),
            serde_json::json!({
                "changed_fields": changed_fields,
                "source": "admin_api",
            }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(sa)
    }

    async fn delete_service_account(
        &self,
        id: &str,
        context: TrustedAuditContext,
    ) -> Result<Vec<String>> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        let before = self
            .service_account
            .get_by_id_for_update(&mut tx, id)
            .await?
            .context("service account not found")?;
        let revoked_key_hashes = self.api_key.revoke_by_service_account(&mut tx, id).await?;
        self.service_account.delete(&mut tx, id).await?;
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("service_account", id),
            "service_account.deleted",
            AuditOperation::Delete,
            Some(snapshot(ServiceAccountAuditSnapshot::from(&before))?),
            None,
            serde_json::json!({
                "revoked_api_key_count": revoked_key_hashes.len(),
                "source": "admin_api",
            }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(revoked_key_hashes)
    }

    // ── Organizations ───────────────────────────────────────────────────────

    async fn create_organization(
        &self,
        name: &str,
        description: Option<&str>,
        attrs: Option<&serde_json::Value>,
        context: TrustedAuditContext,
    ) -> Result<Organization> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        let org = self
            .organization
            .create(&mut tx, name, description, attrs)
            .await?;
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("organization", &org.id),
            "organization.created",
            AuditOperation::Create,
            None,
            Some(snapshot(OrganizationAuditSnapshot::from(&org))?),
            serde_json::json!({ "source": "admin_api" }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(org)
    }

    async fn get_organization_by_id(&self, id: &str) -> Result<Option<Organization>> {
        self.organization.get_by_id(&self.pool, id).await
    }

    async fn get_all_organizations(&self, limit: i64, offset: i64) -> Result<Vec<Organization>> {
        self.organization.get_all(&self.pool, limit, offset).await
    }

    async fn count_organizations(&self) -> Result<i64> {
        self.organization.count(&self.pool).await
    }

    async fn search_organizations(
        &self,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Organization>> {
        self.organization
            .search(&self.pool, query, limit, offset)
            .await
    }

    async fn count_search_organizations(&self, query: &str) -> Result<i64> {
        self.organization.count_search(&self.pool, query).await
    }

    async fn update_organization(
        &self,
        id: &str,
        name: &str,
        description: Option<&str>,
        attrs: Option<&serde_json::Value>,
        context: TrustedAuditContext,
    ) -> Result<Organization> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        let before = self
            .organization
            .get_by_id_for_update(&mut tx, id)
            .await?
            .context("cannot audit update for a missing organization")?;
        let org = self
            .organization
            .update(&mut tx, id, name, description, attrs)
            .await?;
        let changed_fields = organization_changed_fields(&before, &org);
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("organization", &org.id),
            "organization.updated",
            AuditOperation::Update,
            Some(snapshot(OrganizationAuditSnapshot::from(&before))?),
            Some(snapshot(OrganizationAuditSnapshot::from(&org))?),
            serde_json::json!({
                "source": "admin_api",
                "changed_fields": changed_fields,
            }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(org)
    }

    async fn delete_organization(
        &self,
        id: &str,
        context: TrustedAuditContext,
    ) -> Result<Vec<String>> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        let before = self
            .organization
            .get_by_id_for_update(&mut tx, id)
            .await?
            .context("cannot audit deletion of a missing organization")?;
        // Keys acting in the org are revoked, not left to degrade org-less:
        // user keys bound to the org and keys of its (cascading) service
        // accounts.
        let revoked_key_hashes = self.api_key.revoke_by_org(&mut tx, id).await?;
        self.organization.delete(&mut tx, id).await?;
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("organization", id),
            "organization.deleted",
            AuditOperation::Delete,
            Some(snapshot(OrganizationAuditSnapshot::from(&before))?),
            None,
            serde_json::json!({
                "source": "admin_api",
                "revoked_api_key_count": revoked_key_hashes.len(),
            }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(revoked_key_hashes)
    }

    async fn add_user_to_organization(
        &self,
        user_id: &str,
        org_id: &str,
        role: &str,
        context: TrustedAuditContext,
    ) -> Result<()> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        self.organization
            .add_user(&mut tx, user_id, org_id, role)
            .await?;
        let after = self
            .organization
            .get_membership(&mut *tx, user_id, org_id)
            .await?
            .context("membership is missing after accepted upsert")?;
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("organization_membership", format!("{org_id}:{user_id}")),
            "organization.member_added",
            AuditOperation::Create,
            None,
            Some(snapshot(OrganizationMembershipAuditSnapshot {
                organization_id: org_id,
                user_id,
                role: &after.role,
            })?),
            serde_json::json!({ "source": "admin_api" }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn remove_user_from_organization(
        &self,
        user_id: &str,
        org_id: &str,
        context: TrustedAuditContext,
    ) -> Result<Vec<String>> {
        ensure_admin_control_plane(&context)?;
        let mut tx = self.pool.begin().await?;
        let before = self
            .organization
            .get_membership_for_update(&mut tx, user_id, org_id)
            .await?
            .context("cannot audit removal of a missing organization membership")?;
        // Org-bound keys are revoked, not unbound: unbinding would silently
        // turn them into working org-less credentials.
        let revoked_key_hashes = self
            .api_key
            .revoke_by_user_and_org(&mut tx, user_id, org_id)
            .await?;
        self.organization
            .remove_user(&mut tx, user_id, org_id)
            .await?;
        let event = build_outbox_event(
            &context,
            Utc::now(),
            AuditResource::new("organization_membership", format!("{org_id}:{user_id}")),
            "organization.member_removed",
            AuditOperation::Delete,
            Some(snapshot(OrganizationMembershipAuditSnapshot {
                organization_id: org_id,
                user_id,
                role: &before.role,
            })?),
            None,
            serde_json::json!({
                "source": "admin_api",
                "revoked_api_key_count": revoked_key_hashes.len(),
            }),
        )?;
        self.outbox.insert(&mut tx, &event, None).await?;
        tx.commit().await?;
        Ok(revoked_key_hashes)
    }

    async fn get_organization_users(&self, org_id: &str) -> Result<Vec<OrgMember>> {
        self.organization.get_users(&self.pool, org_id).await
    }

    async fn get_organization_users_paginated(
        &self,
        org_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<OrgMember>> {
        self.organization
            .get_users_paginated(&self.pool, org_id, limit, offset)
            .await
    }

    async fn count_organization_users(&self, org_id: &str) -> Result<i64> {
        self.organization.count_users(&self.pool, org_id).await
    }

    async fn get_user_organizations(&self, user_id: &str) -> Result<Vec<OrgMembership>> {
        self.organization
            .get_orgs_by_user(&self.pool, user_id)
            .await
    }

    async fn get_user_organization(
        &self,
        user_id: &str,
        org_id: &str,
    ) -> Result<Option<OrgMembership>> {
        self.organization
            .get_membership(&self.pool, user_id, org_id)
            .await
    }
}

// ── sqlite integration tests ────────────────────────────────────────────────
//
// Each test runs against its own in-memory sqlite database with the real
// migrations applied (pool capped at one connection: every sqlite `:memory:`
// connection is a separate database).
#[cfg(all(test, feature = "sqlite"))]
mod sqlite_tests {
    use super::*;
    use crate::db::DbStore;
    use crate::ent::{
        CredentialType, Organization, Profile, TrustedAdminActor, TrustedAuditContext,
        TrustedAuditRequest, User,
    };

    async fn test_service() -> Service {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite pool");
        let service = Service::new(pool);
        service.migrate().await.expect("migrations apply");
        service
    }

    fn trusted_ctx() -> TrustedAuditContext {
        TrustedAuditContext::admin_control_plane(
            TrustedAdminActor::admin("01JZ000000000000000000000A"),
            TrustedAuditRequest::from_http(ulid::Ulid::new().to_string(), None, None, None),
        )
    }

    async fn create_test_user(svc: &Service, tag: &str) -> User {
        let profile = Profile::new(format!("{tag}@example.com"), tag.to_string());
        svc.create_user(profile, CredentialType::Password, "hash", trusted_ctx())
            .await
            .expect("create user")
    }

    async fn create_test_org(svc: &Service, name: &str) -> Organization {
        let attrs = serde_json::json!({});
        svc.create_organization(name, None, Some(&attrs), trusted_ctx())
            .await
            .expect("create organization")
    }

    #[tokio::test]
    async fn membership_defaults_and_upsert_updates_role_keeping_created_at() {
        let svc = test_service().await;
        let user = create_test_user(&svc, "alice").await;
        let org = create_test_org(&svc, "acme").await;

        svc.add_user_to_organization(&user.id, &org.id, "member", trusted_ctx())
            .await
            .expect("add membership");

        let membership = svc
            .get_user_organization(&user.id, &org.id)
            .await
            .expect("query membership")
            .expect("membership exists");
        assert_eq!(membership.role, "member");
        assert_eq!(membership.organization.id, org.id);
        let member_since = membership.member_since.expect("member_since set");

        // Re-adding upserts the role without duplicating the row or touching
        // the original membership timestamp.
        svc.add_user_to_organization(&user.id, &org.id, "admin", trusted_ctx())
            .await
            .expect("upsert role");

        let membership = svc
            .get_user_organization(&user.id, &org.id)
            .await
            .expect("query membership")
            .expect("membership exists");
        assert_eq!(membership.role, "admin");
        assert_eq!(membership.member_since, Some(member_since));
        assert_eq!(svc.count_organization_users(&org.id).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn membership_lookup_is_none_for_non_members() {
        let svc = test_service().await;
        let user = create_test_user(&svc, "alice").await;
        let org = create_test_org(&svc, "acme").await;

        assert!(
            svc.get_user_organization(&user.id, &org.id)
                .await
                .expect("query membership")
                .is_none()
        );
    }

    #[tokio::test]
    async fn membership_listings_carry_roles_in_both_directions() {
        let svc = test_service().await;
        let alice = create_test_user(&svc, "alice").await;
        let bob = create_test_user(&svc, "bob").await;
        let acme = create_test_org(&svc, "acme").await;
        let globex = create_test_org(&svc, "globex").await;

        svc.add_user_to_organization(&alice.id, &acme.id, "owner", trusted_ctx())
            .await
            .unwrap();
        svc.add_user_to_organization(&alice.id, &globex.id, "member", trusted_ctx())
            .await
            .unwrap();
        svc.add_user_to_organization(&bob.id, &acme.id, "member", trusted_ctx())
            .await
            .unwrap();

        let mut alice_orgs = svc.get_user_organizations(&alice.id).await.unwrap();
        alice_orgs.sort_by(|a, b| a.organization.name.cmp(&b.organization.name));
        assert_eq!(alice_orgs.len(), 2);
        assert_eq!(alice_orgs[0].organization.id, acme.id);
        assert_eq!(alice_orgs[0].role, "owner");
        assert_eq!(alice_orgs[1].organization.id, globex.id);
        assert_eq!(alice_orgs[1].role, "member");

        let mut members = svc
            .get_organization_users_paginated(&acme.id, 10, 0)
            .await
            .unwrap();
        members.sort_by(|a, b| a.user.nickname.cmp(&b.user.nickname));
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].user.id, alice.id);
        assert_eq!(members[0].role, "owner");
        assert_eq!(members[1].user.id, bob.id);
        assert_eq!(members[1].role, "member");
    }

    #[tokio::test]
    async fn user_api_key_org_binding_flows_to_auth_lookup() {
        let svc = test_service().await;
        let user = create_test_user(&svc, "alice").await;
        let org = create_test_org(&svc, "acme").await;
        svc.add_user_to_organization(&user.id, &org.id, "member", trusted_ctx())
            .await
            .unwrap();

        svc.create_user_api_key(
            &user.id,
            "hash-bound",
            "bound",
            None,
            Some(&org.id),
            trusted_ctx(),
        )
        .await
        .expect("create bound key");
        svc.create_user_api_key(
            &user.id,
            "hash-unbound",
            "unbound",
            None,
            None,
            trusted_ctx(),
        )
        .await
        .expect("create unbound key");

        let bound = svc
            .get_api_key_auth_by_hash("hash-bound")
            .await
            .expect("query bound key")
            .expect("bound key exists");
        assert_eq!(bound.org_id.as_deref(), Some(org.id.as_str()));
        assert!(!bound.api_key.revoked);

        let unbound = svc
            .get_api_key_auth_by_hash("hash-unbound")
            .await
            .expect("query unbound key")
            .expect("unbound key exists");
        assert_eq!(unbound.org_id, None);
    }

    #[tokio::test]
    async fn service_account_api_key_inherits_org_in_auth_lookup() {
        let svc = test_service().await;
        let org = create_test_org(&svc, "acme").await;
        let sa = svc
            .create_service_account("worker", None, Some(&org.id), trusted_ctx())
            .await
            .expect("create service account");

        svc.create_service_account_api_key(&sa.id, "hash-sa", "sa-key", None, trusted_ctx())
            .await
            .expect("create sa key");

        let auth = svc
            .get_api_key_auth_by_hash("hash-sa")
            .await
            .expect("query sa key")
            .expect("sa key exists");
        assert_eq!(auth.org_id.as_deref(), Some(org.id.as_str()));
    }

    async fn assert_key_revoked(svc: &Service, hash: &str, expected: bool) {
        let auth = svc
            .get_api_key_auth_by_hash(hash)
            .await
            .expect("query key")
            .expect("key row exists");
        assert_eq!(auth.api_key.revoked, expected, "revoked state of {hash}");
    }

    async fn audit_payloads(svc: &Service) -> Vec<JsonValue> {
        let payloads: Vec<String> =
            sqlx::query_scalar("SELECT payload FROM outbox_events ORDER BY seq")
                .fetch_all(&svc.pool)
                .await
                .expect("read audit outbox payloads");
        payloads
            .into_iter()
            .map(|payload| serde_json::from_str(&payload).expect("valid audit JSON"))
            .collect()
    }

    fn audit_event_by_action<'a>(events: &'a [JsonValue], action: &str) -> &'a JsonValue {
        events
            .iter()
            .find(|event| event["action"] == action)
            .unwrap_or_else(|| panic!("missing audit action {action}"))
    }

    fn anonymous_application_context() -> TrustedAuditContext {
        TrustedAuditContext::application(
            crate::ent::TrustedAuditActor::anonymous(),
            TrustedAuditRequest::from_http(ulid::Ulid::new().to_string(), None, None, None),
        )
    }

    fn user_application_context(user_id: &str) -> TrustedAuditContext {
        TrustedAuditContext::application(
            crate::ent::TrustedAuditActor::user(user_id),
            TrustedAuditRequest::from_http(ulid::Ulid::new().to_string(), None, None, None),
        )
    }

    #[tokio::test]
    async fn audit_user_lifecycle_uses_control_plane_safe_snapshots_without_legacy_writes() {
        let svc = test_service().await;
        let create_secret = "CREATE_PASSWORD_HASH_SECRET";
        let attrs_secret = "USER_ATTRS_SECRET";
        let profile = Profile::new("alice@example.com".to_string(), "alice".to_string())
            .given_name(Some("Private Given Name".to_string()))
            .family_name(Some("Private Family Name".to_string()))
            .phone_number(Some("PRIVATE_PHONE".to_string()))
            .picture(Some("PRIVATE_PICTURE".to_string()))
            .attrs(serde_json::json!({ "secret": attrs_secret }));

        let user = svc
            .create_user(
                profile,
                CredentialType::Password,
                create_secret,
                trusted_ctx(),
            )
            .await
            .expect("create user");

        let mut changed = user.clone();
        changed.email = "alice.updated@example.com".to_string();
        changed.nickname = "alice-updated".to_string();
        changed.given_name = Some("Still Private".to_string());
        changed.attrs = serde_json::json!({ "secret": "UPDATED_ATTR_SECRET" });
        svc.update_user(changed, trusted_ctx())
            .await
            .expect("update user");
        svc.delete_user(&user.id, trusted_ctx())
            .await
            .expect("delete user");

        let events = audit_payloads(&svc).await;
        assert_eq!(events.len(), 3);
        let created = audit_event_by_action(&events, "user.created");
        assert_eq!(created["scope"]["kind"], "control_plane");
        assert_eq!(created["actor"]["type"], "admin");
        assert_eq!(created["resource"]["type"], "user");
        assert_eq!(created["resource"]["id"], user.id);
        assert!(created["before"].is_null());
        assert_eq!(
            created["after"],
            serde_json::json!({ "email": "alice@example.com", "nickname": "alice" })
        );
        assert_eq!(created["metadata"]["source"], "admin_api");

        let updated = audit_event_by_action(&events, "user.updated");
        assert_eq!(
            updated["before"],
            serde_json::json!({ "email": "alice@example.com", "nickname": "alice" })
        );
        assert_eq!(
            updated["after"],
            serde_json::json!({
                "email": "alice.updated@example.com",
                "nickname": "alice-updated",
            })
        );
        assert_eq!(
            updated["metadata"]["changed_fields"],
            serde_json::json!(["email", "nickname", "given_name", "attrs"])
        );

        let deleted = audit_event_by_action(&events, "user.deleted");
        assert_eq!(deleted["operation"], "delete");
        assert_eq!(
            deleted["before"],
            serde_json::json!({
                "email": "alice.updated@example.com",
                "nickname": "alice-updated",
            })
        );
        assert!(deleted["after"].is_null());
        assert_eq!(deleted["metadata"]["revoked_api_key_count"], 0);

        let serialized = serde_json::to_string(&events).expect("serialize events");
        for prohibited in [
            create_secret,
            attrs_secret,
            "UPDATED_ATTR_SECRET",
            "Private Given Name",
            "Private Family Name",
            "PRIVATE_PHONE",
            "PRIVATE_PICTURE",
        ] {
            assert!(!serialized.contains(prohibited), "leaked {prohibited}");
        }
    }

    #[tokio::test]
    async fn audit_non_admin_user_and_credential_events_use_explicit_application_contexts() {
        let svc = test_service().await;
        let initial_hash = "INITIAL_PASSWORD_HASH_SECRET";
        let profile = Profile::new("signup@example.com".to_string(), "signup".to_string());
        let user = svc
            .create_user(
                profile,
                CredentialType::Password,
                initial_hash,
                anonymous_application_context(),
            )
            .await
            .expect("signup user");

        let mut social_update = user.clone();
        social_update.picture = Some("SOCIAL_PICTURE_SECRET".to_string());
        let social_context = TrustedAuditContext::application(
            crate::ent::TrustedAuditActor::external_identity("github:verified-subject"),
            TrustedAuditRequest::from_http(ulid::Ulid::new().to_string(), None, None, None),
        );
        svc.update_user(social_update, social_context)
            .await
            .expect("social update");

        let changed_hash = "CHANGED_PASSWORD_HASH_SECRET";
        svc.change_password(&user.id, changed_hash, user_application_context(&user.id))
            .await
            .expect("change password");
        let rehashed = "REHASHED_PASSWORD_HASH_SECRET";
        assert!(
            svc.rehash_credential(
                &user.id,
                changed_hash,
                rehashed,
                TrustedAuditContext::background(
                    crate::ent::TrustedAuditBoundary::application(),
                    crate::ent::TrustedBackgroundActor::system(None),
                ),
            )
            .await
            .expect("rehash credential")
        );

        let events = audit_payloads(&svc).await;
        let created = audit_event_by_action(&events, "user.created");
        assert_eq!(created["scope"]["kind"], "application");
        assert_eq!(created["actor"]["type"], "anonymous");
        assert_eq!(created["metadata"]["source"], "signup");

        let updated = audit_event_by_action(&events, "user.updated");
        assert_eq!(updated["scope"]["kind"], "application");
        assert_eq!(updated["actor"]["type"], "external_identity");
        assert_eq!(updated["actor"]["id"], "github:verified-subject");
        assert_eq!(updated["metadata"]["source"], "github");
        assert_eq!(
            updated["metadata"]["changed_fields"],
            serde_json::json!(["picture"])
        );

        let password_changed = audit_event_by_action(&events, "credential.password_changed");
        assert_eq!(password_changed["actor"]["type"], "user");
        assert_eq!(password_changed["actor"]["id"], user.id);
        assert_eq!(password_changed["scope"]["kind"], "application");
        assert!(password_changed["before"].is_null());
        assert!(password_changed["after"].is_null());
        assert_eq!(
            password_changed["metadata"],
            serde_json::json!({
                "credential_type": "password",
                "source": "password_reset",
            })
        );

        let credential_rehashed = audit_event_by_action(&events, "credential.rehashed");
        assert_eq!(credential_rehashed["actor"]["type"], "system");
        assert!(credential_rehashed["actor"]["id"].is_null());
        assert!(credential_rehashed["request"].is_null());
        assert_eq!(credential_rehashed["scope"]["kind"], "application");
        assert_eq!(
            credential_rehashed["metadata"],
            serde_json::json!({ "credential_type": "password", "trigger": "login" })
        );

        let serialized = serde_json::to_string(&events).expect("serialize events");
        for prohibited in [
            initial_hash,
            changed_hash,
            rehashed,
            "SOCIAL_PICTURE_SECRET",
        ] {
            assert!(!serialized.contains(prohibited), "leaked {prohibited}");
        }
    }

    #[tokio::test]
    async fn audit_organization_and_membership_events_remain_control_plane_and_safe() {
        let svc = test_service().await;
        let user = create_test_user(&svc, "member").await;
        let attrs = serde_json::json!({ "secret": "ORGANIZATION_ATTR_SECRET" });
        let org = svc
            .create_organization(
                "Acme",
                Some("ORGANIZATION_DESCRIPTION_SECRET"),
                Some(&attrs),
                trusted_ctx(),
            )
            .await
            .expect("create organization");
        svc.add_user_to_organization(&user.id, &org.id, "owner", trusted_ctx())
            .await
            .expect("add membership");
        svc.update_organization(
            &org.id,
            "Acme Updated",
            Some("UPDATED_DESCRIPTION_SECRET"),
            Some(&serde_json::json!({ "secret": "UPDATED_ORG_ATTR_SECRET" })),
            trusted_ctx(),
        )
        .await
        .expect("update organization");
        svc.remove_user_from_organization(&user.id, &org.id, trusted_ctx())
            .await
            .expect("remove membership");
        svc.delete_organization(&org.id, trusted_ctx())
            .await
            .expect("delete organization");

        let events = audit_payloads(&svc).await;
        for action in [
            "organization.created",
            "organization.updated",
            "organization.member_added",
            "organization.member_removed",
            "organization.deleted",
        ] {
            assert_eq!(
                audit_event_by_action(&events, action)["scope"]["kind"],
                "control_plane",
                "unexpected scope for {action}"
            );
        }

        let created = audit_event_by_action(&events, "organization.created");
        assert_eq!(created["after"], serde_json::json!({ "name": "Acme" }));
        let updated = audit_event_by_action(&events, "organization.updated");
        assert_eq!(updated["before"], serde_json::json!({ "name": "Acme" }));
        assert_eq!(
            updated["after"],
            serde_json::json!({ "name": "Acme Updated" })
        );
        assert_eq!(
            updated["metadata"]["changed_fields"],
            serde_json::json!(["name", "description", "attrs"])
        );

        let membership_id = format!("{}:{}", org.id, user.id);
        let added = audit_event_by_action(&events, "organization.member_added");
        assert_eq!(added["resource"]["type"], "organization_membership");
        assert_eq!(added["resource"]["id"], membership_id);
        assert_eq!(
            added["after"],
            serde_json::json!({
                "organization_id": org.id,
                "user_id": user.id,
                "role": "owner",
            })
        );
        let removed = audit_event_by_action(&events, "organization.member_removed");
        assert_eq!(removed["before"], added["after"]);
        assert!(removed["after"].is_null());
        assert_eq!(removed["metadata"]["revoked_api_key_count"], 0);
        let deleted = audit_event_by_action(&events, "organization.deleted");
        assert_eq!(
            deleted["before"],
            serde_json::json!({ "name": "Acme Updated" })
        );

        let serialized = serde_json::to_string(&events).expect("serialize events");
        for prohibited in [
            "ORGANIZATION_ATTR_SECRET",
            "ORGANIZATION_DESCRIPTION_SECRET",
            "UPDATED_DESCRIPTION_SECRET",
            "UPDATED_ORG_ATTR_SECRET",
        ] {
            assert!(!serialized.contains(prohibited), "leaked {prohibited}");
        }
    }

    #[tokio::test]
    async fn audit_super_admin_bootstrap_writes_two_distinct_events_with_one_timestamp() {
        let svc = test_service().await;
        let context = TrustedAuditContext::background(
            crate::ent::TrustedAuditBoundary::control_plane(),
            crate::ent::TrustedBackgroundActor::system(None),
        );
        let user = svc
            .create_super_admin_user(
                Profile::new("root@example.com".to_string(), "root".to_string()),
                CredentialType::Password,
                "BOOTSTRAP_PASSWORD_HASH_SECRET",
                context,
            )
            .await
            .expect("bootstrap super admin");

        let events = audit_payloads(&svc).await;
        assert_eq!(events.len(), 2);
        let user_event = audit_event_by_action(&events, "user.created");
        let grant_event = audit_event_by_action(&events, "super_admin.granted");
        assert_ne!(user_event["event_id"], grant_event["event_id"]);
        assert_eq!(user_event["occurred_at"], grant_event["occurred_at"]);
        assert_eq!(user_event["scope"]["kind"], "control_plane");
        assert_eq!(grant_event["scope"]["kind"], "control_plane");
        assert_eq!(user_event["actor"]["type"], "system");
        assert!(user_event["request"].is_null());
        assert_eq!(
            grant_event["after"],
            serde_json::json!({ "user_id": user.id, "active": true })
        );
        assert_eq!(grant_event["metadata"]["source"], "cli_bootstrap");
        assert!(
            !serde_json::to_string(&events)
                .expect("serialize events")
                .contains("BOOTSTRAP_PASSWORD_HASH_SECRET")
        );
    }

    #[tokio::test]
    async fn audit_validation_failure_rolls_back_business_mutation_and_outbox() {
        let svc = test_service().await;
        let invalid_request = TrustedAuditRequest::from_http(
            ulid::Ulid::new().to_string(),
            None,
            None,
            Some("x".repeat(513)),
        );
        let context = TrustedAuditContext::admin_control_plane(
            TrustedAdminActor::admin("01JZ000000000000000000000A"),
            invalid_request,
        );
        let email = "rollback@example.com";

        let result = svc
            .create_user(
                Profile::new(email.to_string(), "rollback".to_string()),
                CredentialType::Password,
                "ROLLBACK_PASSWORD_HASH_SECRET",
                context,
            )
            .await;
        assert!(
            result
                .expect_err("oversized request fact must reject event")
                .to_string()
                .contains("request.user_agent exceeds 512")
        );
        assert!(
            svc.get_user_by_username(email)
                .await
                .expect("query rolled-back user")
                .is_none()
        );
        assert!(audit_payloads(&svc).await.is_empty());
    }

    fn test_oauth_client(client_id: &str, secret_hash: Option<&str>) -> OAuthClient {
        OAuthClient::new(
            client_id.to_string(),
            secret_hash.map(str::to_string),
            "Audit Client".to_string(),
            Some("OAUTH_DESCRIPTION_SECRET".to_string()),
            "client_secret_basic".to_string(),
            vec!["client_credentials".to_string()],
            vec!["code".to_string()],
            vec!["https://client.example/callback".to_string()],
            vec!["openid".to_string()],
            vec!["gateway".to_string()],
            serde_json::json!({ "secret": "OAUTH_ATTRS_SECRET" }),
        )
    }

    fn events_for_resource<'a>(
        events: &'a [JsonValue],
        resource_type: &str,
        resource_id: &str,
    ) -> Vec<&'a JsonValue> {
        events
            .iter()
            .filter(|event| {
                event["resource"]["type"] == resource_type && event["resource"]["id"] == resource_id
            })
            .collect()
    }

    #[tokio::test]
    async fn audit_oauth_client_lifecycle_uses_semantic_actions_and_never_serializes_secrets() {
        let svc = test_service().await;
        let client_id = format!("audit-client-{}", ulid::Ulid::new());
        let initial_hash = "OAUTH_INITIAL_SECRET_HASH";
        let rotated_hash = "OAUTH_ROTATED_SECRET_HASH";
        let client = svc
            .create_oauth_client(
                test_oauth_client(&client_id, Some(initial_hash)),
                trusted_ctx(),
            )
            .await
            .expect("create OAuth client");

        let mut changed = client.clone();
        changed.name = "Audit Client Updated".to_string();
        changed.description = Some("OAUTH_UPDATED_DESCRIPTION_SECRET".to_string());
        changed.scopes = sqlx::types::Json(vec!["openid".to_string(), "profile".to_string()]);
        changed.attrs = serde_json::json!({ "secret": "OAUTH_UPDATED_ATTRS_SECRET" });
        svc.update_oauth_client(changed, trusted_ctx())
            .await
            .expect("update OAuth client");
        svc.set_oauth_client_enabled(&client_id, false, trusted_ctx())
            .await
            .expect("disable OAuth client");
        svc.set_oauth_client_enabled(&client_id, true, trusted_ctx())
            .await
            .expect("enable OAuth client");
        svc.update_oauth_client_secret_hash(&client_id, Some(rotated_hash), trusted_ctx())
            .await
            .expect("rotate OAuth client secret");
        svc.delete_oauth_client(&client_id, trusted_ctx())
            .await
            .expect("delete OAuth client");

        let events = audit_payloads(&svc).await;
        let client_events = events_for_resource(&events, "oauth_client", &client_id);
        let actions: Vec<&str> = client_events
            .iter()
            .map(|event| event["action"].as_str().expect("action string"))
            .collect();
        assert_eq!(
            actions,
            vec![
                "oauth_client.created",
                "oauth_client.updated",
                "oauth_client.disabled",
                "oauth_client.enabled",
                "oauth_client.secret_rotated",
                "oauth_client.deleted",
            ]
        );
        for event in &client_events {
            assert_eq!(event["scope"]["kind"], "control_plane");
            assert_eq!(event["actor"]["type"], "admin");
            assert_eq!(event["metadata"]["source"], "admin_api");
        }

        let created = client_events[0];
        assert!(created["before"].is_null());
        assert_eq!(created["after"]["name"], "Audit Client");
        assert_eq!(created["after"]["enabled"], true);
        assert!(created["after"].get("description").is_none());
        assert!(created["after"].get("attrs").is_none());
        let updated = client_events[1];
        assert_eq!(updated["before"]["name"], "Audit Client");
        assert_eq!(updated["after"]["name"], "Audit Client Updated");
        assert_eq!(
            updated["metadata"]["changed_fields"],
            serde_json::json!(["name", "description", "scopes", "attrs"])
        );
        assert_eq!(client_events[2]["before"]["enabled"], true);
        assert_eq!(client_events[2]["after"]["enabled"], false);
        assert_eq!(client_events[3]["before"]["enabled"], false);
        assert_eq!(client_events[3]["after"]["enabled"], true);
        assert!(client_events[4]["before"].is_null());
        assert!(client_events[4]["after"].is_null());
        assert_eq!(client_events[5]["before"]["name"], "Audit Client Updated");
        assert!(client_events[5]["after"].is_null());

        let serialized = serde_json::to_string(&client_events).expect("serialize events");
        for prohibited in [
            initial_hash,
            rotated_hash,
            "OAUTH_DESCRIPTION_SECRET",
            "OAUTH_ATTRS_SECRET",
            "OAUTH_UPDATED_DESCRIPTION_SECRET",
            "OAUTH_UPDATED_ATTRS_SECRET",
        ] {
            assert!(!serialized.contains(prohibited), "leaked {prohibited}");
        }
    }

    #[tokio::test]
    async fn audit_api_keys_use_persisted_owners_and_route_owned_scopes() {
        let svc = test_service().await;
        let user = create_test_user(&svc, "key-owner").await;
        let org = create_test_org(&svc, "key-org").await;
        svc.add_user_to_organization(&user.id, &org.id, "member", trusted_ctx())
            .await
            .expect("add membership");
        let sa = svc
            .create_service_account(
                "worker",
                Some("SA_DESCRIPTION_SECRET"),
                Some(&org.id),
                trusted_ctx(),
            )
            .await
            .expect("create service account");

        let user_hash = "USER_API_KEY_HASH_SECRET";
        let user_key = svc
            .create_user_api_key(
                &user.id,
                user_hash,
                "user-key",
                Some(serde_json::json!({ "secret": "API_KEY_ATTRS_SECRET" })),
                Some(&org.id),
                trusted_ctx(),
            )
            .await
            .expect("create user key");
        let sa_hash = "SERVICE_ACCOUNT_API_KEY_HASH_SECRET";
        let sa_key = svc
            .create_service_account_api_key(&sa.id, sa_hash, "sa-key", None, trusted_ctx())
            .await
            .expect("create service-account key");
        let unbound_hash = "UNBOUND_API_KEY_HASH_SECRET";
        let unbound_key = svc
            .create_user_api_key(
                &user.id,
                unbound_hash,
                "unbound-key",
                None,
                None,
                trusted_ctx(),
            )
            .await
            .expect("create unbound user key");

        let mut updated = user_key.clone();
        updated.attrs = serde_json::json!({ "secret": "UPDATED_API_KEY_ATTRS_SECRET" });
        svc.update_api_key(updated, trusted_ctx())
            .await
            .expect("update API key attrs");
        let oauth_context = TrustedAuditContext::organization(
            crate::ent::TrustedAuditActor::service("authorized-oauth-client"),
            TrustedAuditRequest::from_http(ulid::Ulid::new().to_string(), None, None, None),
            Some(&org.id),
            None,
        )
        .expect("persisted organization context");
        svc.revoke_api_key(&user_key.id, oauth_context)
            .await
            .expect("OAuth revoke API key");
        svc.revoke_api_key(
            &unbound_key.id,
            TrustedAuditContext::application(
                crate::ent::TrustedAuditActor::service("authorized-oauth-client"),
                TrustedAuditRequest::from_http(ulid::Ulid::new().to_string(), None, None, None),
            ),
        )
        .await
        .expect("OAuth revoke unbound API key");
        svc.delete_api_key(&sa_key.id, trusted_ctx())
            .await
            .expect("delete service-account key");

        let events = audit_payloads(&svc).await;
        let user_events = events_for_resource(&events, "api_key", &user_key.id);
        assert_eq!(user_events.len(), 3);
        assert_eq!(user_events[0]["action"], "api_key.created");
        assert_eq!(user_events[0]["scope"]["kind"], "control_plane");
        assert_eq!(user_events[0]["after"]["owner_type"], "user");
        assert_eq!(user_events[0]["after"]["owner_id"], user.id);
        assert_eq!(user_events[0]["after"]["organization_id"], org.id);
        assert_eq!(user_events[1]["action"], "api_key.updated");
        assert_eq!(
            user_events[1]["metadata"]["changed_fields"],
            serde_json::json!(["attrs"])
        );
        assert_eq!(user_events[2]["action"], "api_key.revoked");
        assert_eq!(user_events[2]["scope"]["kind"], "organization");
        assert_eq!(user_events[2]["scope"]["organization_id"], org.id);
        assert_eq!(user_events[2]["actor"]["type"], "service");
        assert_eq!(user_events[2]["actor"]["id"], "authorized-oauth-client");
        assert_eq!(user_events[2]["before"]["revoked"], false);
        assert_eq!(user_events[2]["after"]["revoked"], true);
        assert_eq!(user_events[2]["metadata"]["source"], "oauth_revoke");

        let sa_events = events_for_resource(&events, "api_key", &sa_key.id);
        assert_eq!(sa_events.len(), 2);
        assert_eq!(sa_events[0]["after"]["owner_type"], "service_account");
        assert_eq!(sa_events[0]["after"]["owner_id"], sa.id);
        assert_eq!(sa_events[0]["after"]["organization_id"], org.id);
        assert_eq!(sa_events[1]["action"], "api_key.deleted");
        assert_eq!(sa_events[1]["scope"]["kind"], "control_plane");

        let unbound_events = events_for_resource(&events, "api_key", &unbound_key.id);
        assert_eq!(unbound_events.len(), 2);
        assert_eq!(unbound_events[1]["action"], "api_key.revoked");
        assert_eq!(unbound_events[1]["scope"]["kind"], "application");
        assert!(unbound_events[1]["scope"]["organization_id"].is_null());
        assert_eq!(unbound_events[1]["metadata"]["source"], "oauth_revoke");

        let serialized = serde_json::to_string(&events).expect("serialize events");
        for prohibited in [
            user_hash,
            sa_hash,
            unbound_hash,
            "API_KEY_ATTRS_SECRET",
            "UPDATED_API_KEY_ATTRS_SECRET",
            "SA_DESCRIPTION_SECRET",
        ] {
            assert!(!serialized.contains(prohibited), "leaked {prohibited}");
        }
    }

    #[tokio::test]
    async fn audit_admin_keys_and_service_accounts_use_safe_control_plane_snapshots() {
        let svc = test_service().await;
        let org = create_test_org(&svc, "admin-resources-org").await;
        let admin_hash = "ADMIN_KEY_HASH_SECRET";
        let admin_key = svc
            .create_admin_key(
                admin_hash,
                Some("operations".to_string()),
                vec!["users".to_string()],
                trusted_ctx(),
            )
            .await
            .expect("create admin key");
        let mut permissions_update = admin_key.clone();
        permissions_update.set_permissions(vec!["users".to_string(), "api_keys".to_string()]);
        svc.update_admin_key(permissions_update, trusted_ctx())
            .await
            .expect("update admin-key permissions");
        svc.revoke_admin_key(&admin_key.id, trusted_ctx())
            .await
            .expect("revoke admin key");
        svc.delete_admin_key(&admin_key.id, trusted_ctx())
            .await
            .expect("delete admin key");

        let account = svc
            .create_service_account(
                "worker",
                Some("SERVICE_ACCOUNT_DESCRIPTION_SECRET"),
                Some(&org.id),
                trusted_ctx(),
            )
            .await
            .expect("create service account");
        let account = svc
            .update_service_account(
                &account.id,
                "worker-updated",
                Some("UPDATED_SERVICE_ACCOUNT_DESCRIPTION_SECRET"),
                trusted_ctx(),
            )
            .await
            .expect("update service account");
        assert_eq!(account.org_id.as_deref(), Some(org.id.as_str()));
        svc.create_service_account_api_key(
            &account.id,
            "SERVICE_ACCOUNT_DELETE_KEY_HASH_SECRET",
            "worker-key",
            None,
            trusted_ctx(),
        )
        .await
        .expect("create account key");
        svc.delete_service_account(&account.id, trusted_ctx())
            .await
            .expect("delete service account");

        let events = audit_payloads(&svc).await;
        let admin_events = events_for_resource(&events, "admin_key", &admin_key.id);
        assert_eq!(admin_events.len(), 4);
        assert_eq!(admin_events[0]["action"], "admin_key.created");
        assert_eq!(
            admin_events[0]["after"]["permissions"],
            serde_json::json!(["users"])
        );
        assert_eq!(admin_events[1]["action"], "admin_key.permissions_updated");
        assert_eq!(
            admin_events[1]["after"]["permissions"],
            serde_json::json!(["users", "api_keys"])
        );
        assert_eq!(admin_events[2]["action"], "admin_key.revoked");
        assert_eq!(admin_events[2]["before"]["revoked"], false);
        assert_eq!(admin_events[2]["after"]["revoked"], true);
        assert_eq!(admin_events[3]["action"], "admin_key.deleted");

        let account_events = events_for_resource(&events, "service_account", &account.id);
        assert_eq!(account_events.len(), 3);
        assert_eq!(account_events[0]["action"], "service_account.created");
        assert_eq!(account_events[0]["after"]["organization_id"], org.id);
        assert_eq!(account_events[1]["action"], "service_account.updated");
        assert_eq!(account_events[1]["before"]["name"], "worker");
        assert_eq!(account_events[1]["after"]["name"], "worker-updated");
        assert_eq!(
            account_events[1]["metadata"]["changed_fields"],
            serde_json::json!(["name", "description"])
        );
        assert_eq!(account_events[2]["action"], "service_account.deleted");
        assert_eq!(account_events[2]["metadata"]["revoked_api_key_count"], 1);
        for event in admin_events.iter().chain(account_events.iter()) {
            assert_eq!(event["scope"]["kind"], "control_plane");
            assert_eq!(event["metadata"]["source"], "admin_api");
        }

        let serialized = serde_json::to_string(&events).expect("serialize events");
        for prohibited in [
            admin_hash,
            "SERVICE_ACCOUNT_DESCRIPTION_SECRET",
            "UPDATED_SERVICE_ACCOUNT_DESCRIPTION_SECRET",
            "SERVICE_ACCOUNT_DELETE_KEY_HASH_SECRET",
        ] {
            assert!(!serialized.contains(prohibited), "leaked {prohibited}");
        }
    }

    #[tokio::test]
    async fn audit_a5_validation_failures_roll_back_every_resource_family() {
        let svc = test_service().await;
        let user = create_test_user(&svc, "a5-rollback-owner").await;
        let baseline = audit_payloads(&svc).await.len();
        let invalid_context = || {
            TrustedAuditContext::admin_control_plane(
                TrustedAdminActor::admin("01JZ000000000000000000000A"),
                TrustedAuditRequest::from_http(
                    ulid::Ulid::new().to_string(),
                    None,
                    None,
                    Some("x".repeat(513)),
                ),
            )
        };

        let client_id = format!("rollback-client-{}", ulid::Ulid::new());
        assert!(
            svc.create_oauth_client(
                test_oauth_client(&client_id, Some("ROLLBACK_OAUTH_HASH_SECRET")),
                invalid_context(),
            )
            .await
            .expect_err("invalid OAuth event must roll back")
            .to_string()
            .contains("request.user_agent exceeds 512")
        );
        assert!(
            svc.get_oauth_client_by_client_id(&client_id)
                .await
                .expect("query OAuth client")
                .is_none()
        );

        assert!(
            svc.create_user_api_key(
                &user.id,
                "ROLLBACK_API_KEY_HASH_SECRET",
                "rollback-key",
                None,
                None,
                invalid_context(),
            )
            .await
            .expect_err("invalid API-key event must roll back")
            .to_string()
            .contains("request.user_agent exceeds 512")
        );
        assert!(
            svc.get_api_key_by_hash("ROLLBACK_API_KEY_HASH_SECRET")
                .await
                .expect("query API key")
                .is_none()
        );

        assert!(
            svc.create_admin_key(
                "ROLLBACK_ADMIN_KEY_HASH_SECRET",
                None,
                vec!["users".to_string()],
                invalid_context(),
            )
            .await
            .expect_err("invalid admin-key event must roll back")
            .to_string()
            .contains("request.user_agent exceeds 512")
        );
        assert!(
            svc.get_admin_key_by_hash("ROLLBACK_ADMIN_KEY_HASH_SECRET")
                .await
                .expect("query admin key")
                .is_none()
        );

        assert!(
            svc.create_service_account("rollback-service-account", None, None, invalid_context(),)
                .await
                .expect_err("invalid service-account event must roll back")
                .to_string()
                .contains("request.user_agent exceeds 512")
        );
        assert!(
            svc.search_service_accounts("rollback-service-account", 10, 0)
                .await
                .expect("query service accounts")
                .is_empty()
        );
        assert_eq!(audit_payloads(&svc).await.len(), baseline);
    }

    #[tokio::test]
    async fn audit_a6_admin_contract_covers_every_resource_family_and_row_identity() {
        let svc = test_service().await;
        let user = create_test_user(&svc, "a6-admin-owner").await;
        let org = create_test_org(&svc, "a6-admin-org").await;
        svc.add_user_to_organization(&user.id, &org.id, "member", trusted_ctx())
            .await
            .expect("add organization member");

        let client_id = format!("a6-client-{}", ulid::Ulid::new());
        svc.create_oauth_client(
            test_oauth_client(&client_id, Some("A6_OAUTH_HASH_SECRET")),
            trusted_ctx(),
        )
        .await
        .expect("create OAuth client");
        svc.create_user_api_key(
            &user.id,
            "A6_API_KEY_HASH_SECRET",
            "a6-user-key",
            None,
            Some(&org.id),
            trusted_ctx(),
        )
        .await
        .expect("create API key");
        svc.create_admin_key(
            "A6_ADMIN_KEY_HASH_SECRET",
            Some("a6-admin-key".to_string()),
            vec!["users".to_string()],
            trusted_ctx(),
        )
        .await
        .expect("create admin key");
        svc.create_service_account("a6-worker", None, Some(&org.id), trusted_ctx())
            .await
            .expect("create service account");

        let rows: Vec<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT event_id, payload, operation_id, pair_role FROM outbox_events ORDER BY seq",
        )
        .fetch_all(&svc.pool)
        .await
        .expect("read outbox rows");
        assert_eq!(
            rows.len(),
            7,
            "the seven admin mutations must emit one event each and no target-scope duplicate"
        );

        let mut event_ids = std::collections::HashSet::new();
        for (row_event_id, payload, row_operation_id, pair_role) in &rows {
            let event: JsonValue = serde_json::from_str(payload).expect("valid audit JSON");
            assert_eq!(event["metadata"]["source"], "admin_api");
            assert_eq!(event["scope"]["kind"], "control_plane");
            assert!(event["scope"]["organization_id"].is_null());
            assert_eq!(event["event_id"].as_str(), Some(row_event_id.as_str()));
            assert!(event["operation_id"].is_null());
            assert!(row_operation_id.is_none());
            assert!(pair_role.is_none());
            assert!(event_ids.insert(row_event_id), "duplicate event_id");
        }

        let invalid_context = TrustedAuditContext::admin_control_plane(
            TrustedAdminActor::admin("01JZ000000000000000000000A"),
            TrustedAuditRequest::from_http(
                ulid::Ulid::new().to_string(),
                None,
                None,
                Some("x".repeat(513)),
            ),
        );
        assert!(
            svc.create_service_account("a6-rolled-back", None, None, invalid_context)
                .await
                .expect_err("invalid event must roll back its business mutation")
                .to_string()
                .contains("request.user_agent exceeds 512")
        );
        assert!(
            svc.search_service_accounts("a6-rolled-back", 10, 0)
                .await
                .expect("query rolled-back service account")
                .is_empty()
        );
        assert_eq!(audit_payloads(&svc).await.len(), 7);
    }

    #[tokio::test]
    async fn delete_user_revokes_owned_keys_in_the_same_transaction() {
        let svc = test_service().await;
        let user = create_test_user(&svc, "alice").await;
        svc.create_user_api_key(&user.id, "hash-a", "a", None, None, trusted_ctx())
            .await
            .unwrap();
        svc.create_user_api_key(&user.id, "hash-b", "b", None, None, trusted_ctx())
            .await
            .unwrap();

        let mut hashes = svc
            .delete_user(&user.id, trusted_ctx())
            .await
            .expect("delete user");
        hashes.sort();

        assert_eq!(hashes, vec!["hash-a".to_string(), "hash-b".to_string()]);
        // The rows survive the cascade orphaned — but revoked, so they no
        // longer authenticate.
        assert_key_revoked(&svc, "hash-a", true).await;
        assert_key_revoked(&svc, "hash-b", true).await;
    }

    #[tokio::test]
    async fn membership_removal_revokes_only_org_bound_keys() {
        let svc = test_service().await;
        let user = create_test_user(&svc, "alice").await;
        let org = create_test_org(&svc, "acme").await;
        svc.add_user_to_organization(&user.id, &org.id, "member", trusted_ctx())
            .await
            .unwrap();
        svc.create_user_api_key(
            &user.id,
            "hash-bound",
            "bound",
            None,
            Some(&org.id),
            trusted_ctx(),
        )
        .await
        .unwrap();
        svc.create_user_api_key(
            &user.id,
            "hash-unbound",
            "unbound",
            None,
            None,
            trusted_ctx(),
        )
        .await
        .unwrap();

        let hashes = svc
            .remove_user_from_organization(&user.id, &org.id, trusted_ctx())
            .await
            .expect("remove membership");

        assert_eq!(hashes, vec!["hash-bound".to_string()]);
        assert_key_revoked(&svc, "hash-bound", true).await;
        assert_key_revoked(&svc, "hash-unbound", false).await;
        assert!(
            svc.get_user_organization(&user.id, &org.id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn org_delete_revokes_bound_user_keys_and_service_account_keys() {
        let svc = test_service().await;
        let user = create_test_user(&svc, "alice").await;
        let org = create_test_org(&svc, "acme").await;
        svc.add_user_to_organization(&user.id, &org.id, "member", trusted_ctx())
            .await
            .unwrap();
        svc.create_user_api_key(
            &user.id,
            "hash-bound",
            "bound",
            None,
            Some(&org.id),
            trusted_ctx(),
        )
        .await
        .unwrap();
        svc.create_user_api_key(
            &user.id,
            "hash-unbound",
            "unbound",
            None,
            None,
            trusted_ctx(),
        )
        .await
        .unwrap();
        let sa = svc
            .create_service_account("worker", None, Some(&org.id), trusted_ctx())
            .await
            .unwrap();
        svc.create_service_account_api_key(&sa.id, "hash-sa", "sa-key", None, trusted_ctx())
            .await
            .unwrap();

        let mut hashes = svc
            .delete_organization(&org.id, trusted_ctx())
            .await
            .expect("delete org");
        hashes.sort();

        assert_eq!(
            hashes,
            vec!["hash-bound".to_string(), "hash-sa".to_string()]
        );
        assert_key_revoked(&svc, "hash-bound", true).await;
        assert_key_revoked(&svc, "hash-sa", true).await;
        assert_key_revoked(&svc, "hash-unbound", false).await;
    }

    #[tokio::test]
    async fn service_account_delete_revokes_its_keys() {
        let svc = test_service().await;
        let sa = svc
            .create_service_account("worker", None, None, trusted_ctx())
            .await
            .unwrap();
        svc.create_service_account_api_key(&sa.id, "hash-sa", "sa-key", None, trusted_ctx())
            .await
            .unwrap();

        let hashes = svc
            .delete_service_account(&sa.id, trusted_ctx())
            .await
            .expect("delete service account");

        assert_eq!(hashes, vec!["hash-sa".to_string()]);
        assert_key_revoked(&svc, "hash-sa", true).await;
    }
}

// ── Integration tests ──────────────────────────────────────────────────────────
//
// These tests require a live PostgreSQL database. Skip them in CI environments
// that lack one, or run them explicitly:
//
//   cargo test -p db --features postgres -- --ignored
//
// The tests use a unique `request_id` per invocation (ULID) as an isolation key
// so they can run against a shared database without interfering with each other.
#[cfg(all(test, feature = "postgres"))]
mod tests {
    use super::*;
    use crate::db::DbStore;
    use crate::ent::{
        CredentialType, Profile, TrustedAdminActor, TrustedAuditBoundary, TrustedAuditContext,
        TrustedAuditRequest, TrustedBackgroundActor,
    };

    async fn test_service() -> Service {
        dotenvy::dotenv().ok();
        let url = std::env::var("TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .expect("TEST_DATABASE_URL or DATABASE_URL must be set for postgres tests");
        init(&url).await.expect("DB init failed")
    }

    async fn count_outbox_by_request_id(svc: &Service, req_id: &str) -> i64 {
        sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM outbox_events
             WHERE payload::jsonb -> 'request' ->> 'request_id' = $1",
        )
        .bind(req_id)
        .fetch_one(&svc.pool)
        .await
        .unwrap_or(0)
    }

    fn admin_context(request_id: String) -> TrustedAuditContext {
        TrustedAuditContext::admin_control_plane(
            TrustedAdminActor::admin("01JZ000000000000000000000A"),
            TrustedAuditRequest::from_http(request_id, None, None, None),
        )
    }

    fn unique_email() -> String {
        format!("test-{}@example.com", ulid::Ulid::new())
    }

    fn unique_nick() -> String {
        ulid::Ulid::new().to_string()
    }

    async fn insert_outbox_relay_event(svc: &Service, request_id: &str) -> String {
        svc.create_user(
            Profile::new(unique_email(), unique_nick()),
            CredentialType::Password,
            "POSTGRES_RELAY_TEST_PASSWORD_HASH",
            admin_context(request_id.to_string()),
        )
        .await
        .expect("create relay test event");

        sqlx::query_scalar(
            "SELECT event_id FROM outbox_events
             WHERE payload::jsonb -> 'request' ->> 'request_id' = $1
             ORDER BY seq DESC
             LIMIT 1",
        )
        .bind(request_id)
        .fetch_one(&svc.pool)
        .await
        .expect("read relay test event id")
    }

    // ── Atomicity ────────────────────────────────────────────────────────────

    /// A rolled-back transaction must not leave any outbox row behind.
    #[tokio::test]
    #[ignore = "requires postgres (run with: cargo test -p db --features postgres -- --ignored)"]
    async fn rollback_leaves_no_audit_outbox_row() {
        let svc = test_service().await;

        let email = unique_email();
        let req_ok = ulid::Ulid::new().to_string();
        let req_fail = ulid::Ulid::new().to_string();

        // First create succeeds — establishes the email that will trigger a constraint violation.
        let ctx_ok = admin_context(req_ok.clone());
        let password_secret = "POSTGRES_CREATE_PASSWORD_SECRET";
        let nickname = unique_nick();
        svc.create_user(
            Profile::new(email.clone(), nickname.clone()),
            CredentialType::Password,
            password_secret,
            ctx_ok,
        )
        .await
        .expect("first create should succeed");

        let payload: String = sqlx::query_scalar(
            "SELECT payload FROM outbox_events
             WHERE payload::jsonb -> 'request' ->> 'request_id' = $1",
        )
        .bind(&req_ok)
        .fetch_one(&svc.pool)
        .await
        .expect("read successful create event");
        let event: JsonValue = serde_json::from_str(&payload).expect("valid audit payload");
        assert_eq!(event["action"], "user.created");
        assert_eq!(event["scope"]["kind"], "control_plane");
        assert_eq!(event["actor"]["type"], "admin");
        assert_eq!(
            event["after"],
            serde_json::json!({ "email": email, "nickname": nickname })
        );
        assert_eq!(event["metadata"]["source"], "admin_api");
        assert!(!payload.contains(password_secret));

        // Second create with the same email — unique constraint fires, tx rolls back.
        let ctx_fail = admin_context(req_fail.clone());
        let result = svc
            .create_user(
                Profile::new(email.clone(), unique_nick()),
                CredentialType::Password,
                "pw",
                ctx_fail,
            )
            .await;
        assert!(result.is_err(), "duplicate email should fail");

        // The failed transaction must have written zero outbox rows.
        let count = count_outbox_by_request_id(&svc, &req_fail).await;
        assert_eq!(count, 0, "rolled-back tx must not leak an outbox row");
    }

    /// `create_super_admin_user` writes exactly two events (user + super_admin)
    /// inside a single transaction.
    #[tokio::test]
    #[ignore = "requires postgres (run with: cargo test -p db --features postgres -- --ignored)"]
    async fn super_admin_create_writes_two_audit_outbox_rows() {
        let svc = test_service().await;

        let actor_id = format!("bootstrap-test:{}", ulid::Ulid::new());
        let ctx = TrustedAuditContext::background(
            TrustedAuditBoundary::control_plane(),
            TrustedBackgroundActor::system(Some(actor_id.clone())),
        );

        let password_secret = "POSTGRES_BOOTSTRAP_PASSWORD_SECRET";
        svc.create_super_admin_user(
            Profile::new(unique_email(), unique_nick()),
            CredentialType::Password,
            password_secret,
            ctx,
        )
        .await
        .expect("super_admin create should succeed");

        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM outbox_events
             WHERE payload::jsonb -> 'actor' ->> 'id' = $1",
        )
        .bind(&actor_id)
        .fetch_one(&svc.pool)
        .await
        .expect("count bootstrap events");
        assert_eq!(
            count, 2,
            "create_super_admin_user must write exactly 2 outbox rows"
        );

        let payloads: Vec<String> = sqlx::query_scalar(
            "SELECT payload FROM outbox_events
             WHERE payload::jsonb -> 'actor' ->> 'id' = $1
             ORDER BY seq",
        )
        .bind(&actor_id)
        .fetch_all(&svc.pool)
        .await
        .expect("read bootstrap events");
        let events: Vec<JsonValue> = payloads
            .iter()
            .map(|payload| serde_json::from_str(payload).expect("valid bootstrap payload"))
            .collect();
        assert_ne!(events[0]["event_id"], events[1]["event_id"]);
        assert_eq!(events[0]["occurred_at"], events[1]["occurred_at"]);
        assert!(
            events
                .iter()
                .all(|event| event["scope"]["kind"] == "control_plane")
        );
        assert!(events.iter().any(|event| event["action"] == "user.created"));
        assert!(
            events
                .iter()
                .any(|event| event["action"] == "super_admin.granted")
        );
        assert!(
            payloads
                .iter()
                .all(|payload| !payload.contains(password_secret))
        );
    }

    #[tokio::test]
    #[ignore = "requires postgres (run with: cargo test -p db --features postgres -- --ignored)"]
    async fn audit_validation_failure_rolls_back_postgres_business_mutation() {
        let svc = test_service().await;
        let request_id = ulid::Ulid::new().to_string();
        let context = TrustedAuditContext::admin_control_plane(
            TrustedAdminActor::admin("01JZ000000000000000000000A"),
            TrustedAuditRequest::from_http(request_id.clone(), None, None, Some("x".repeat(513))),
        );
        let email = unique_email();

        let result = svc
            .create_user(
                Profile::new(email.clone(), unique_nick()),
                CredentialType::Password,
                "POSTGRES_ROLLBACK_PASSWORD_SECRET",
                context,
            )
            .await;
        assert!(
            result
                .expect_err("invalid event must fail")
                .to_string()
                .contains("request.user_agent exceeds 512")
        );
        assert!(
            svc.get_user_by_username(&email)
                .await
                .expect("query user")
                .is_none()
        );
        assert_eq!(count_outbox_by_request_id(&svc, &request_id).await, 0);
    }

    #[tokio::test]
    #[ignore = "requires postgres (run with: cargo test -p db --features postgres -- --ignored)"]
    async fn audit_a5_postgres_producers_use_outbox_semantics_and_exclude_secrets() {
        let svc = test_service().await;
        let request_id = ulid::Ulid::new().to_string();
        let context = admin_context(request_id.clone());
        let client_id = format!("audit-client-{}", ulid::Ulid::new());
        let oauth_hash = "POSTGRES_OAUTH_SECRET_HASH";
        let rotated_hash = "POSTGRES_ROTATED_OAUTH_SECRET_HASH";
        let mut client = OAuthClient::new(
            client_id.clone(),
            Some(oauth_hash.to_string()),
            "Postgres Audit Client".to_string(),
            Some("POSTGRES_OAUTH_DESCRIPTION_SECRET".to_string()),
            "client_secret_basic".to_string(),
            vec!["client_credentials".to_string()],
            vec!["code".to_string()],
            vec!["https://client.example/callback".to_string()],
            vec!["openid".to_string()],
            vec!["gateway".to_string()],
            serde_json::json!({ "secret": "POSTGRES_OAUTH_ATTRS_SECRET" }),
        );
        client = svc
            .create_oauth_client(client, context.clone())
            .await
            .expect("create OAuth client");
        client.name = "Postgres Audit Client Updated".to_string();
        svc.update_oauth_client(client, context.clone())
            .await
            .expect("update OAuth client");
        svc.set_oauth_client_enabled(&client_id, false, context.clone())
            .await
            .expect("disable OAuth client");
        svc.update_oauth_client_secret_hash(&client_id, Some(rotated_hash), context.clone())
            .await
            .expect("rotate OAuth secret");

        let user = svc
            .create_user(
                Profile::new(unique_email(), unique_nick()),
                CredentialType::Password,
                "POSTGRES_A5_USER_PASSWORD_HASH",
                context.clone(),
            )
            .await
            .expect("create key owner");
        let api_hash = format!("POSTGRES_API_KEY_HASH_{}", ulid::Ulid::new());
        let api_key = svc
            .create_user_api_key(
                &user.id,
                &api_hash,
                "postgres-key",
                Some(serde_json::json!({ "secret": "POSTGRES_API_ATTRS_SECRET" })),
                None,
                context.clone(),
            )
            .await
            .expect("create API key");
        let mut changed_key = api_key.clone();
        changed_key.attrs = serde_json::json!({ "secret": "POSTGRES_UPDATED_API_ATTRS_SECRET" });
        svc.update_api_key(changed_key, context.clone())
            .await
            .expect("update API key");
        svc.revoke_api_key(&api_key.id, context.clone())
            .await
            .expect("revoke API key");

        let admin_hash = format!("POSTGRES_ADMIN_KEY_HASH_{}", ulid::Ulid::new());
        let admin_key = svc
            .create_admin_key(
                &admin_hash,
                Some("postgres-admin".to_string()),
                vec!["users".to_string()],
                context.clone(),
            )
            .await
            .expect("create admin key");
        let mut changed_admin_key = admin_key.clone();
        changed_admin_key.set_permissions(vec!["users".to_string(), "api_keys".to_string()]);
        svc.update_admin_key(changed_admin_key, context.clone())
            .await
            .expect("update admin key");
        svc.revoke_admin_key(&admin_key.id, context.clone())
            .await
            .expect("revoke admin key");

        let account = svc
            .create_service_account(
                "postgres-audit-worker",
                Some("POSTGRES_SERVICE_ACCOUNT_DESCRIPTION_SECRET"),
                None,
                context.clone(),
            )
            .await
            .expect("create service account");
        svc.update_service_account(
            &account.id,
            "postgres-audit-worker-updated",
            Some("POSTGRES_UPDATED_SERVICE_ACCOUNT_DESCRIPTION_SECRET"),
            context.clone(),
        )
        .await
        .expect("update service account");

        let payloads: Vec<String> = sqlx::query_scalar(
            "SELECT payload FROM outbox_events
             WHERE payload::jsonb -> 'request' ->> 'request_id' = $1
             ORDER BY seq",
        )
        .bind(&request_id)
        .fetch_all(&svc.pool)
        .await
        .expect("read A5 payloads");
        let events: Vec<JsonValue> = payloads
            .iter()
            .map(|payload| serde_json::from_str(payload).expect("valid A5 payload"))
            .collect();
        for action in [
            "oauth_client.created",
            "oauth_client.updated",
            "oauth_client.disabled",
            "oauth_client.secret_rotated",
            "api_key.created",
            "api_key.updated",
            "api_key.revoked",
            "admin_key.created",
            "admin_key.permissions_updated",
            "admin_key.revoked",
            "service_account.created",
            "service_account.updated",
        ] {
            assert!(
                events.iter().any(|event| event["action"] == action),
                "missing action {action}"
            );
        }
        assert!(events.iter().all(|event| {
            !matches!(
                event["resource"]["type"].as_str(),
                Some("oauth_client" | "api_key" | "admin_key" | "service_account")
            ) || event["scope"]["kind"] == "control_plane"
        }));
        let serialized = payloads.join("\n");
        for prohibited in [
            oauth_hash,
            rotated_hash,
            api_hash.as_str(),
            admin_hash.as_str(),
            "POSTGRES_OAUTH_DESCRIPTION_SECRET",
            "POSTGRES_OAUTH_ATTRS_SECRET",
            "POSTGRES_API_ATTRS_SECRET",
            "POSTGRES_UPDATED_API_ATTRS_SECRET",
            "POSTGRES_SERVICE_ACCOUNT_DESCRIPTION_SECRET",
            "POSTGRES_UPDATED_SERVICE_ACCOUNT_DESCRIPTION_SECRET",
        ] {
            assert!(!serialized.contains(prohibited), "leaked {prohibited}");
        }
    }

    #[tokio::test]
    #[ignore = "requires postgres (run with: cargo test -p db --features postgres -- --ignored)"]
    async fn audit_a5_postgres_invalid_event_rolls_back_secret_creating_mutation() {
        let svc = test_service().await;
        let request_id = ulid::Ulid::new().to_string();
        let client_id = format!("rollback-client-{}", ulid::Ulid::new());
        let context = TrustedAuditContext::admin_control_plane(
            TrustedAdminActor::admin("01JZ000000000000000000000A"),
            TrustedAuditRequest::from_http(request_id.clone(), None, None, Some("x".repeat(513))),
        );
        let client = OAuthClient::new(
            client_id.clone(),
            Some("POSTGRES_ROLLBACK_OAUTH_HASH_SECRET".to_string()),
            "Rollback Client".to_string(),
            None,
            "client_secret_basic".to_string(),
            vec!["client_credentials".to_string()],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec!["gateway".to_string()],
            serde_json::json!({}),
        );

        assert!(svc.create_oauth_client(client, context).await.is_err());
        assert!(
            svc.get_oauth_client_by_client_id(&client_id)
                .await
                .expect("query rolled-back client")
                .is_none()
        );
        assert_eq!(count_outbox_by_request_id(&svc, &request_id).await, 0);
    }

    // ── Relay claim / publish / mark cycle ───────────────────────────────────

    /// Serializes the claim-based tests: cargo runs tests in parallel, and two
    /// concurrent claimers would steal (and publish) each other's rows, making
    /// presence assertions flaky. Insert-only tests don't need this.
    static CLAIM_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// A claimed row is absent from subsequent claims after `commit_published`.
    #[tokio::test]
    #[ignore = "requires postgres (run with: cargo test -p db --features postgres -- --ignored)"]
    async fn relay_claim_publish_mark_cycle() {
        let _guard = CLAIM_LOCK.lock().await;
        let svc = test_service().await;

        let req_id = ulid::Ulid::new().to_string();
        let event_id = insert_outbox_relay_event(&svc, &req_id).await;

        // Claim with a limit large enough to swallow any backlog left behind by
        // the insert-only tests — our row must be present.
        let claim = svc
            .claim_unpublished_outbox_events(100_000)
            .await
            .expect("first claim failed");
        assert!(
            claim.rows().iter().any(|row| row.event_id == event_id),
            "the outbox row written above must appear in the claim"
        );

        // Mark all claimed rows as published.
        claim
            .commit_published()
            .await
            .expect("commit_published failed");

        // A fresh claim must not return the same row (published_at IS NOW set).
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM outbox_events
             WHERE event_id = $1 AND published_at IS NULL",
        )
        .bind(&event_id)
        .fetch_one(&svc.pool)
        .await
        .unwrap_or(0);
        assert_eq!(
            count, 0,
            "after commit_published the row must not be in the unpublished set"
        );
    }

    /// Two concurrent open claims must not overlap — `FOR UPDATE SKIP LOCKED`
    /// ensures each relay node claims a disjoint set of rows.
    #[tokio::test]
    #[ignore = "requires postgres (run with: cargo test -p db --features postgres -- --ignored)"]
    async fn concurrent_claims_are_disjoint() {
        let _guard = CLAIM_LOCK.lock().await;
        let svc = test_service().await;

        // Insert 4 unpublished raw outbox rows.
        for _ in 0..4 {
            insert_outbox_relay_event(&svc, &ulid::Ulid::new().to_string()).await;
        }

        // Open claim1 (limit 2) — keeps its transaction open.
        let claim1 = svc
            .claim_unpublished_outbox_events(2)
            .await
            .expect("claim1 failed");
        // Open claim2 (limit 2) — must skip rows locked by claim1.
        let claim2 = svc
            .claim_unpublished_outbox_events(2)
            .await
            .expect("claim2 failed");

        let seqs1: std::collections::HashSet<i64> = claim1.rows().iter().map(|r| r.seq).collect();
        let seqs2: std::collections::HashSet<i64> = claim2.rows().iter().map(|r| r.seq).collect();

        let overlap: Vec<_> = seqs1.intersection(&seqs2).collect();
        assert!(
            overlap.is_empty(),
            "concurrent claims must not claim the same rows; overlap: {:?}",
            overlap
        );

        // Clean up: mark both batches published so they don't linger.
        claim1.commit_published().await.ok();
        claim2.commit_published().await.ok();
    }
}
