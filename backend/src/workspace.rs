use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub owner_id: String,
    #[serde(rename = "isPersonal")]
    pub is_personal: bool,
    #[serde(rename = "deletedAt", skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<DateTime<Utc>>,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct WorkspaceMember {
    #[serde(rename = "workspaceId")]
    pub workspace_id: String,
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(rename = "joinedAt")]
    pub joined_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceWithMemberCount {
    pub id: String,
    pub name: String,
    pub slug: String,
    #[serde(rename = "ownerId")]
    pub owner_id: String,
    #[serde(rename = "isPersonal")]
    pub is_personal: bool,
    #[serde(rename = "memberCount")]
    pub member_count: i64,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMemberWithInfo {
    #[serde(rename = "userId")]
    pub user_id: String,
    pub username: String,
    #[serde(rename = "joinedAt")]
    pub joined_at: DateTime<Utc>,
    #[serde(rename = "isOwner")]
    pub is_owner: bool,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceResponse {
    pub id: String,
    pub name: String,
    pub slug: String,
    #[serde(rename = "ownerId")]
    pub owner_id: String,
    #[serde(rename = "isPersonal")]
    pub is_personal: bool,
}

#[derive(Debug, Serialize)]
pub struct CurrentWorkspaceResponse {
    pub id: String,
    pub name: String,
    pub slug: String,
    #[serde(rename = "isPersonal")]
    pub is_personal: bool,
}

pub const WORKSPACE_NAME_MIN_LEN: usize = 3;
pub const WORKSPACE_NAME_MAX_LEN: usize = 50;
pub const WORKSPACE_SLUG_MIN_LEN: usize = 3;
pub const WORKSPACE_SLUG_MAX_LEN: usize = 63;
pub const LEGACY_SHARED_WORKSPACE_NAME: &str = "Migrated Workspace";

pub fn validate_workspace_name(name: &str) -> Result<String, String> {
    let name = name.trim().to_string();
    let char_count = name.chars().count();

    if name.is_empty() {
        return Err("工作空间名称不能为空".to_string());
    }

    if char_count < WORKSPACE_NAME_MIN_LEN {
        return Err(format!(
            "工作空间名称至少需要 {} 个字符",
            WORKSPACE_NAME_MIN_LEN
        ));
    }

    if char_count > WORKSPACE_NAME_MAX_LEN {
        return Err(format!(
            "工作空间名称不能超过 {} 个字符",
            WORKSPACE_NAME_MAX_LEN
        ));
    }

    Ok(name)
}

#[allow(dead_code)]
pub fn generate_deleted_workspace_name(original_name: &str, workspace_id: &str) -> String {
    format!("{}_deleted_{}", original_name, workspace_id)
}

pub fn make_personal_workspace_name(username: &str) -> String {
    format!("{}的个人空间", username)
}

pub fn make_legacy_shared_workspace_name() -> &'static str {
    LEGACY_SHARED_WORKSPACE_NAME
}

pub fn validate_workspace_slug(slug: &str) -> Result<String, String> {
    let slug = slug.trim().to_ascii_lowercase();
    let len = slug.chars().count();

    if slug.is_empty() {
        return Err("工作空间 slug 不能为空".to_string());
    }
    if len < WORKSPACE_SLUG_MIN_LEN {
        return Err(format!(
            "工作空间 slug 至少需要 {} 个字符",
            WORKSPACE_SLUG_MIN_LEN
        ));
    }
    if len > WORKSPACE_SLUG_MAX_LEN {
        return Err(format!(
            "工作空间 slug 不能超过 {} 个字符",
            WORKSPACE_SLUG_MAX_LEN
        ));
    }
    if slug.starts_with('-') || slug.ends_with('-') {
        return Err("工作空间 slug 不能以连字符开头或结尾".to_string());
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err("工作空间 slug 仅支持小写字母、数字和连字符".to_string());
    }
    Ok(slug)
}

pub fn slugify_workspace_name(name: &str) -> String {
    let mut out = String::new();
    let mut last_is_dash = false;

    for ch in name.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            last_is_dash = false;
        } else if (ch.is_whitespace() || c == '-' || c == '_') && !last_is_dash {
            out.push('-');
            last_is_dash = true;
        }
    }

    let trimmed = out.trim_matches('-').to_string();
    if trimmed.len() > WORKSPACE_SLUG_MAX_LEN {
        trimmed[..WORKSPACE_SLUG_MAX_LEN]
            .trim_end_matches('-')
            .to_string()
    } else {
        trimmed
    }
}

pub fn fallback_workspace_slug_from_id(workspace_id: &str) -> String {
    let suffix = &workspace_id[..workspace_id.len().min(8)];
    format!("ws-{suffix}")
}

pub fn workspace_slug_base_from_name_or_id(name: &str, workspace_id: &str) -> String {
    let from_name = slugify_workspace_name(name);
    if validate_workspace_slug(&from_name).is_ok() {
        return from_name;
    }

    let from_id = fallback_workspace_slug_from_id(workspace_id);
    if validate_workspace_slug(&from_id).is_ok() {
        return from_id;
    }

    "workspace-default".to_string()
}

pub async fn ensure_test_mode_workspace(db: &Arc<Mutex<duckdb::Connection>>) -> Option<String> {
    let conn = db.lock().await;

    let workspace_id: Option<String> = conn
        .query_row(
            "SELECT id FROM workspaces WHERE is_personal = true AND deleted_at IS NULL LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok()
        .flatten();

    if let Some(wid) = workspace_id {
        drop(conn);
        return Some(wid);
    }

    let existing_user_id: Option<String> = conn
        .query_row("SELECT id FROM users LIMIT 1", [], |row| row.get(0))
        .ok()
        .flatten();

    let user_id = match existing_user_id {
        Some(uid) => uid,
        None => {
            let new_user_id = uuid::Uuid::new_v4().to_string();
            let _ = conn.execute(
                "INSERT INTO users (id, username, password_hash, role, current_workspace_id, created_at) VALUES (?, ?, '', 'user', NULL, CURRENT_TIMESTAMP)",
                duckdb::params![&new_user_id, format!("test_user_{}", &new_user_id[..8])],
            );
            new_user_id
        }
    };

    let new_workspace_id = uuid::Uuid::new_v4().to_string();
    let workspace_name = "Test Workspace".to_string();
    let workspace_slug = workspace_slug_base_from_name_or_id(&workspace_name, &new_workspace_id);

    let _ = conn.execute(
        "INSERT INTO workspaces (id, name, slug, owner_id, is_personal, created_at) VALUES (?, ?, ?, ?, true, CURRENT_TIMESTAMP)",
        duckdb::params![&new_workspace_id, &workspace_name, &workspace_slug, &user_id],
    );

    let _ = conn.execute(
        "INSERT INTO workspace_members (workspace_id, user_id, joined_at) VALUES (?, ?, CURRENT_TIMESTAMP)",
        duckdb::params![&new_workspace_id, &user_id],
    );

    drop(conn);
    Some(new_workspace_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_workspace_name_rejects_empty() {
        assert!(validate_workspace_name("").is_err());
        assert!(validate_workspace_name("   ").is_err());
    }

    #[test]
    fn validate_workspace_name_rejects_too_short() {
        assert!(validate_workspace_name("ab").is_err());
        assert!(validate_workspace_name("abc").is_ok());
    }

    #[test]
    fn validate_workspace_name_rejects_too_long() {
        let long_name = "a".repeat(51);
        assert!(validate_workspace_name(&long_name).is_err());

        let max_name = "a".repeat(50);
        assert!(validate_workspace_name(&max_name).is_ok());
    }

    #[test]
    fn validate_workspace_name_trims_whitespace() {
        let result = validate_workspace_name("  valid name  ");
        assert_eq!(result.unwrap(), "valid name");
    }

    #[test]
    fn generate_deleted_workspace_name_format() {
        let result = generate_deleted_workspace_name("myworkspace", "ws-123");
        assert_eq!(result, "myworkspace_deleted_ws-123");
    }

    #[test]
    fn make_personal_workspace_name_format() {
        let result = make_personal_workspace_name("alice");
        assert_eq!(result, "alice的个人空间");
    }

    #[test]
    fn make_legacy_shared_workspace_name_format() {
        assert_eq!(
            make_legacy_shared_workspace_name(),
            LEGACY_SHARED_WORKSPACE_NAME
        );
    }

    #[test]
    fn validate_workspace_name_counts_characters_not_bytes() {
        let valid_name = "工作空间".repeat(12);
        assert_eq!(valid_name.chars().count(), 48);
        assert!(validate_workspace_name(&valid_name).is_ok());

        let too_long_name = "测".repeat(51);
        assert_eq!(too_long_name.chars().count(), 51);
        assert!(validate_workspace_name(&too_long_name).is_err());
    }

    #[test]
    fn validate_workspace_slug_rejects_invalid_chars() {
        assert_eq!(validate_workspace_slug("Hello").unwrap(), "hello");
        assert!(validate_workspace_slug("abc_def").is_err());
        assert!(validate_workspace_slug("-abc").is_err());
        assert!(validate_workspace_slug("abc-").is_err());
    }

    #[test]
    fn validate_workspace_slug_accepts_lowercase_dash() {
        assert_eq!(
            validate_workspace_slug("team-alpha-01").unwrap(),
            "team-alpha-01"
        );
    }

    #[test]
    fn slugify_workspace_name_normalizes_to_kebab_case() {
        assert_eq!(slugify_workspace_name("Team Alpha 01"), "team-alpha-01");
        assert_eq!(slugify_workspace_name("  Team___Beta  "), "team-beta");
        assert_eq!(slugify_workspace_name("中文空间"), "");
    }

    #[test]
    fn fallback_workspace_slug_from_id_prefixes_ws() {
        assert_eq!(
            fallback_workspace_slug_from_id("abcdef123456"),
            "ws-abcdef12"
        );
    }

    #[test]
    fn workspace_slug_base_prefers_name_then_id() {
        assert_eq!(
            workspace_slug_base_from_name_or_id("Team One", "abcdef123456"),
            "team-one"
        );
        assert_eq!(
            workspace_slug_base_from_name_or_id("中文空间", "abcdef123456"),
            "ws-abcdef12"
        );
    }
}
