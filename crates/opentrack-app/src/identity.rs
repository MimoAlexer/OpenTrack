use super::*;
use argon2::{
    Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version,
    password_hash::SaltString,
};
use opentrack_domain::{
    Login, Session, Setup, User, bounded_text, normalize_email, validate_password,
    validate_team_key,
};
use sqlx::Row;

impl App {
    pub async fn setup_required(&self) -> Result<bool> {
        Ok(!store::user_exists(&self.pool).await.map_err(db_error)?)
    }
    pub(crate) async fn password_work(
        &self,
        password: String,
        existing: Option<String>,
    ) -> Result<String> {
        let permit = self
            .hashes
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::RateLimited)?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let params = Params::new(65536, 3, 1, None).map_err(|_| Error::Internal)?;
            let engine = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
            if let Some(hash) = existing {
                let parsed = PasswordHash::new(&hash).map_err(|_| Error::Internal)?;
                engine
                    .verify_password(password.as_bytes(), &parsed)
                    .map_err(|_| Error::Unauthorized)?;
                Ok(hash)
            } else {
                let salt = SaltString::generate(&mut OsRng);
                engine
                    .hash_password(password.as_bytes(), &salt)
                    .map(|p| p.to_string())
                    .map_err(|_| Error::Internal)
            }
        })
        .await
        .map_err(|_| Error::Internal)?
    }
    pub async fn setup(&self, req: Setup) -> Result<(Session, String)> {
        use subtle::ConstantTimeEq;
        if !bool::from(
            self.setup_token_hash
                .as_bytes()
                .ct_eq(digest(req.setup_token.as_bytes()).as_bytes()),
        ) {
            return Err(Error::Forbidden);
        }
        let email = normalize_email(&req.email)?;
        validate_password(&req.password)?;
        let name = bounded_text(&req.display_name, "Name", 100)?;
        let workspace_name = bounded_text(&req.workspace_name, "Workspace name", 100)?;
        let team_name = bounded_text(&req.team_name, "Team name", 100)?;
        let team_key = validate_team_key(&req.team_key)?;
        if !self.setup_required().await? {
            return Err(Error::Conflict("Setup has already been completed".into()));
        }
        let hash = self.password_work(req.password, None).await?;
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        sqlx::query("SELECT pg_advisory_xact_lock(1869636978)")
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users)")
            .fetch_one(&mut *tx)
            .await
            .map_err(db_error)?;
        if exists {
            return Err(Error::Conflict("Setup has already been completed".into()));
        }
        let user = User {
            id: Uuid::new_v4(),
            email,
            display_name: name,
        };
        let workspace = Uuid::new_v4();
        let organization = Uuid::new_v4();
        let team = Uuid::new_v4();
        store::context(&mut tx, user.id, Some(workspace))
            .await
            .map_err(db_error)?;
        sqlx::query("INSERT INTO users(id,email,display_name,password_hash) VALUES($1,$2,$3,$4)")
            .bind(user.id)
            .bind(&user.email)
            .bind(&user.display_name)
            .bind(hash)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        sqlx::query("INSERT INTO organizations(id,name,created_by) VALUES($1,$2,$3)")
            .bind(organization)
            .bind(&workspace_name)
            .bind(user.id)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        sqlx::query(
            "INSERT INTO organization_members(organization_id,user_id,role) VALUES($1,$2,'owner')",
        )
        .bind(organization)
        .bind(user.id)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        sqlx::query("INSERT INTO workspaces(id,organization_id,name,slug) VALUES($1,$2,$3,$4)")
            .bind(workspace)
            .bind(organization)
            .bind(&workspace_name)
            .bind(format!("workspace-{}", &workspace.to_string()[..8]))
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        sqlx::query(
            "INSERT INTO workspace_members(workspace_id,user_id,role) VALUES($1,$2,'owner')",
        )
        .bind(workspace)
        .bind(user.id)
        .execute(&mut *tx)
        .await
        .map_err(db_error)?;
        sqlx::query("INSERT INTO workspace_auth_guards(workspace_id) VALUES($1)")
            .bind(workspace)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        sqlx::query("INSERT INTO workspace_stream_heads(workspace_id) VALUES($1)")
            .bind(workspace)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        sqlx::query("INSERT INTO teams(id,workspace_id,name,key) VALUES($1,$2,$3,$4)")
            .bind(team)
            .bind(workspace)
            .bind(team_name)
            .bind(team_key)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        sqlx::query("INSERT INTO team_prefixes(workspace_id,prefix,team_id) SELECT workspace_id,key,id FROM teams WHERE workspace_id=$1 AND id=$2").bind(workspace).bind(team).execute(&mut *tx).await.map_err(db_error)?;
        sqlx::query("INSERT INTO team_members(workspace_id,team_id,user_id,role) VALUES($1,$2,$3,'maintainer')").bind(workspace).bind(team).bind(user.id).execute(&mut *tx).await.map_err(db_error)?;
        for (position, (name, category, color)) in [
            ("Backlog", "backlog", "gray"),
            ("Todo", "unstarted", "slate"),
            ("In progress", "started", "amber"),
            ("Done", "completed", "green"),
            ("Canceled", "canceled", "gray"),
        ]
        .iter()
        .enumerate()
        {
            sqlx::query("INSERT INTO workflow_statuses(id,workspace_id,team_id,name,category,color,position,is_default) VALUES($1,$2,$3,$4,$5,$6,$7,$8)").bind(Uuid::new_v4()).bind(workspace).bind(team).bind(name).bind(category).bind(color).bind(position as i32).bind(position==0).execute(&mut *tx).await.map_err(db_error)?;
        }
        store::identity_audit(
            &mut tx,
            workspace,
            user.id,
            workspace,
            "instance.setup",
            Uuid::new_v4(),
        )
        .await
        .map_err(db_error)?;
        let (_, token, csrf) = crate::sessions::mint_session(&mut tx, user.id).await?;
        let workspaces = store::workspaces(&mut tx, user.id)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok((
            Session {
                audience: SessionAudience::Web,
                user,
                csrf_token: csrf,
                workspaces,
            },
            token,
        ))
    }
    pub async fn login(&self, req: Login) -> Result<(Session, String)> {
        self.login_with_previous(req, None).await
    }
    pub async fn login_with_previous(
        &self,
        req: Login,
        previous_token: Option<&str>,
    ) -> Result<(Session, String)> {
        let email = normalize_email(&req.email).map_err(|_| Error::Unauthorized)?;
        if req.password.len() > 1024 {
            return Err(Error::Unauthorized);
        }
        let account_hash = digest(email.as_bytes());
        // API admission bounds new attempts. Expire old account buckets in small
        // indexed batches so random account names cannot accumulate indefinitely.
        sqlx::query("DELETE FROM login_attempts WHERE account_hash IN (SELECT account_hash FROM login_attempts WHERE window_start<now()-interval '5 minutes' ORDER BY window_start,account_hash LIMIT 100)")
            .execute(&self.pool).await.map_err(db_error)?;

        let attempts:i32=sqlx::query_scalar("INSERT INTO login_attempts(account_hash,attempts) VALUES($1,1) ON CONFLICT(account_hash) DO UPDATE SET attempts=CASE WHEN login_attempts.window_start<now()-interval '5 minutes' THEN 1 ELSE login_attempts.attempts+1 END,window_start=CASE WHEN login_attempts.window_start<now()-interval '5 minutes' THEN now() ELSE login_attempts.window_start END RETURNING attempts").bind(&account_hash).fetch_one(&self.pool).await.map_err(db_error)?;
        if attempts > 5 {
            return Err(Error::RateLimited);
        }
        let record=sqlx::query("SELECT id,email,display_name,password_hash FROM users WHERE email=$1 AND disabled_at IS NULL").bind(email).fetch_optional(&self.pool).await.map_err(db_error)?;
        let Some(row) = record else {
            self.password_work(req.password, None).await?;
            return Err(Error::Unauthorized);
        };
        let password_hash: String = row.try_get("password_hash").map_err(db_error)?;
        let verified_hash = self
            .password_work(req.password, Some(password_hash))
            .await?;
        let user_id: Uuid = row.try_get("id").map_err(db_error)?;
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        store::context_exclusive(&mut tx, user_id, None)
            .await
            .map_err(db_error)?;
        // Password work never holds a transaction. A concurrent password change
        // or disable must still win before we mint or revoke any session.
        let row = sqlx::query("SELECT id,email,display_name FROM users WHERE id=$1 AND password_hash=$2 AND disabled_at IS NULL")
            .bind(user_id).bind(verified_hash).fetch_optional(&mut *tx).await.map_err(db_error)?.ok_or(Error::Unauthorized)?;
        let user = User {
            id: row.try_get("id").map_err(db_error)?,
            email: row.try_get("email").map_err(db_error)?,
            display_name: row.try_get("display_name").map_err(db_error)?,
        };
        sqlx::query("DELETE FROM login_attempts WHERE account_hash=$1")
            .bind(account_hash)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        let (new_id, token, csrf) = crate::sessions::mint_session(&mut tx, user.id).await?;
        if let Some(previous) = previous_token.filter(|token| token.len() == 64) {
            crate::sessions::rotate_previous(
                &mut tx,
                user.id,
                &digest(previous.as_bytes()),
                new_id,
                "session.login_rotated",
            )
            .await?;
        }
        let workspaces = store::workspaces(&mut tx, user.id)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok((
            Session {
                audience: SessionAudience::Web,
                user,
                csrf_token: csrf,
                workspaces,
            },
            token,
        ))
    }
    pub async fn session(&self, actor: &Actor) -> Result<Session> {
        let mut tx = store::begin(&self.pool).await.map_err(db_error)?;
        store::context(&mut tx, actor.user.id, None)
            .await
            .map_err(db_error)?;
        if !store::active_session(&mut tx, actor)
            .await
            .map_err(db_error)?
        {
            return Err(Error::Unauthorized);
        }
        let workspaces = store::workspaces(&mut tx, actor.user.id)
            .await
            .map_err(db_error)?;
        tx.commit().await.map_err(db_error)?;
        Ok(Session {
            audience: actor.audience,
            user: actor.user.clone(),
            csrf_token: actor.csrf_token.clone(),
            workspaces,
        })
    }
}
