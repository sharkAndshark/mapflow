use axum_login::{AuthUser, AuthnBackend, UserId};
use duckdb::OptionalExt;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    #[serde(skip)]
    pub password_hash: String,
    pub role: String,
}

impl AuthUser for User {
    type Id = String;

    fn id(&self) -> Self::Id {
        self.id.clone()
    }

    fn session_auth_hash(&self) -> &[u8] {
        self.password_hash.as_bytes()
    }
}

#[derive(Clone)]
pub struct AuthBackend {
    db: Arc<Mutex<duckdb::Connection>>,
}

impl AuthBackend {
    pub fn new(db: Arc<Mutex<duckdb::Connection>>) -> Self {
        Self { db }
    }
}

#[derive(Debug)]
pub enum AuthError {
    Database(String),
    UserNotFound,
    InvalidCredentials,
    PasswordHash(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::Database(msg) => write!(f, "Database error: {}", msg),
            AuthError::UserNotFound => write!(f, "User not found"),
            AuthError::InvalidCredentials => write!(f, "Invalid credentials"),
            AuthError::PasswordHash(msg) => write!(f, "Password hashing error: {}", msg),
        }
    }
}

impl std::error::Error for AuthError {}

impl AuthnBackend for AuthBackend {
    type User = User;
    type Error = AuthError;
    type Credentials = (String, String); // (username, password)

    async fn authenticate(
        &self,
        creds: (String, String),
    ) -> Result<Option<Self::User>, Self::Error> {
        let (username, password) = creds;
        let conn = self.db.lock().await;

        let mut stmt = conn
            .prepare("SELECT id, username, password_hash, role FROM users WHERE username = ?")
            .map_err(|e| AuthError::Database(e.to_string()))?;

        let user_result = stmt
            .query_row(duckdb::params![username], |row| {
                Ok(User {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    password_hash: row.get(2)?,
                    role: row.get(3)?,
                })
            })
            .optional()
            .map_err(|e: duckdb::Error| AuthError::Database(e.to_string()))?;

        if let Some(user) = user_result {
            let is_valid = crate::password::verify_password(&password, &user.password_hash)
                .map_err(|e| AuthError::PasswordHash(e.to_string()))?;

            if is_valid {
                Ok(Some(user))
            } else {
                Err(AuthError::InvalidCredentials)
            }
        } else {
            use std::sync::OnceLock;

            static DUMMY_HASH: OnceLock<String> = OnceLock::new();

            let dummy_hash = DUMMY_HASH.get_or_init(|| {
                crate::password::hash_password("dummy_password_for_timing_attack").unwrap_or_else(
                    |_| "$2b$12$0000000000000000000000000000000000000000000000000000".to_string(),
                )
            });

            // Timing attack mitigation: always execute verify_password to equalize response time
            // The result is intentionally discarded since we always return InvalidCredentials
            let _ = crate::password::verify_password(&password, dummy_hash);
            Err(AuthError::InvalidCredentials)
        }
    }

    async fn get_user(&self, user_id: &UserId<Self>) -> Result<Option<Self::User>, Self::Error> {
        let conn = self.db.lock().await;

        let mut stmt = conn
            .prepare("SELECT id, username, password_hash, role FROM users WHERE id = ?")
            .map_err(|e| AuthError::Database(e.to_string()))?;

        let user_result = stmt
            .query_row(duckdb::params![user_id], |row| {
                Ok(User {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    password_hash: row.get(2)?,
                    role: row.get(3)?,
                })
            })
            .optional()
            .map_err(|e: duckdb::Error| AuthError::Database(e.to_string()))?;

        Ok(user_result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{init_database, password::hash_password};
    use std::sync::Arc;
    use tempfile::TempDir;

    async fn create_test_backend() -> (AuthBackend, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = Arc::new(Mutex::new(init_database(&db_path)));
        let backend = AuthBackend::new(conn);
        (backend, temp_dir)
    }

    async fn create_test_user(backend: &AuthBackend, username: &str, password: &str, role: &str) {
        let conn = backend.db.lock().await;
        let user_id = uuid::Uuid::new_v4().to_string();
        let password_hash = hash_password(password).unwrap();

        conn.execute(
            "INSERT INTO users (id, username, password_hash, role, created_at) VALUES (?, ?, ?, ?, CURRENT_TIMESTAMP)",
            duckdb::params![user_id, username, password_hash, role],
        ).unwrap();
    }

    #[tokio::test]
    async fn test_authenticate_success() {
        let (backend, _temp_dir) = create_test_backend().await;
        create_test_user(&backend, "testuser", "Test123!@#", "admin").await;

        let result = backend
            .authenticate(("testuser".to_string(), "Test123!@#".to_string()))
            .await
            .unwrap();

        assert!(result.is_some());
        let user = result.unwrap();
        assert_eq!(user.username, "testuser");
        assert_eq!(user.role, "admin");
    }

    #[tokio::test]
    async fn test_authenticate_wrong_password() {
        let (backend, _temp_dir) = create_test_backend().await;
        create_test_user(&backend, "testuser", "Test123!@#", "admin").await;

        let result = backend
            .authenticate(("testuser".to_string(), "WrongPassword123!".to_string()))
            .await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AuthError::InvalidCredentials));
    }

    #[tokio::test]
    async fn test_authenticate_nonexistent_user() {
        let (backend, _temp_dir) = create_test_backend().await;

        let result = backend
            .authenticate(("nonexistent".to_string(), "Test123!@#".to_string()))
            .await;

        // Should return InvalidCredentials (not Ok(None)) for timing attack mitigation
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AuthError::InvalidCredentials));
    }

    #[tokio::test]
    async fn test_get_user() {
        let (backend, _temp_dir) = create_test_backend().await;
        create_test_user(&backend, "testuser", "Test123!@#", "admin").await;

        let conn = backend.db.lock().await;
        let user_id: String = conn
            .query_row(
                "SELECT id FROM users WHERE username = ?",
                duckdb::params!["testuser"],
                |row| row.get(0),
            )
            .unwrap();
        drop(conn);

        let user = backend.get_user(&user_id).await.unwrap().unwrap();

        assert_eq!(user.username, "testuser");
        assert_eq!(user.role, "admin");
    }

    #[tokio::test]
    async fn test_get_nonexistent_user() {
        let (backend, _temp_dir) = create_test_backend().await;

        let result = backend
            .get_user(&"nonexistent-id".to_string())
            .await
            .unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_timing_attack_mitigation() {
        let (backend, _temp_dir) = create_test_backend().await;
        create_test_user(&backend, "existinguser", "Test123!@#", "admin").await;

        // Timing attack mitigation ensures that both scenarios execute the same code path:
        // 1. User exists but wrong password -> executes verify_password with real hash
        // 2. User doesn't exist -> executes verify_password with dummy hash
        // Both should return the same error type, making timing attacks impractical

        let result_wrong_password = backend
            .authenticate(("existinguser".to_string(), "WrongPassword123!".to_string()))
            .await;

        let result_nonexistent = backend
            .authenticate(("nonexistent".to_string(), "Test123!@#".to_string()))
            .await;

        // Both scenarios should return InvalidCredentials
        assert!(
            matches!(result_wrong_password, Err(AuthError::InvalidCredentials)),
            "Expected InvalidCredentials for wrong password, got {:?}",
            result_wrong_password
        );

        assert!(
            matches!(result_nonexistent, Err(AuthError::InvalidCredentials)),
            "Expected InvalidCredentials for nonexistent user, got {:?}",
            result_nonexistent
        );

        // The implementation ensures both paths execute verify_password:
        // - Real user: verify_password with stored hash
        // - Nonexistent user: verify_password with dummy hash
        // This equalizes response times, preventing timing-based user enumeration
    }
}
