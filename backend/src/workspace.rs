use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
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
    #[serde(rename = "ownerId")]
    pub owner_id: String,
    #[serde(rename = "isPersonal")]
    pub is_personal: bool,
}

#[derive(Debug, Serialize)]
pub struct CurrentWorkspaceResponse {
    pub id: String,
    pub name: String,
    #[serde(rename = "isPersonal")]
    pub is_personal: bool,
}

pub const WORKSPACE_NAME_MIN_LEN: usize = 3;
pub const WORKSPACE_NAME_MAX_LEN: usize = 50;
pub const LEGACY_SHARED_WORKSPACE_NAME: &str = "Migrated Workspace";

pub fn validate_workspace_name(name: &str) -> Result<String, String> {
    let name = name.trim().to_string();

    if name.is_empty() {
        return Err("工作空间名称不能为空".to_string());
    }

    if name.len() < WORKSPACE_NAME_MIN_LEN {
        return Err(format!(
            "工作空间名称至少需要 {} 个字符",
            WORKSPACE_NAME_MIN_LEN
        ));
    }

    if name.len() > WORKSPACE_NAME_MAX_LEN {
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
}
