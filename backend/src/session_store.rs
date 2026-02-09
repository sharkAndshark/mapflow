use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_sessions::{
    session::{Id, Record},
    session_store::Error,
    SessionStore,
};

#[derive(Debug, Clone)]
pub struct DuckDBStore {
    conn: Arc<Mutex<duckdb::Connection>>,
}

impl DuckDBStore {
    pub fn new(conn: Arc<Mutex<duckdb::Connection>>) -> Self {
        Self { conn }
    }

    fn record_to_json(record: &Record) -> Result<String, Error> {
        serde_json::to_string(&record.data)
            .map_err(|e| Error::Backend(format!("Failed to serialize session data: {}", e)))
    }

    fn json_to_record(
        id: &str,
        data: &str,
        expiry_date: chrono::DateTime<chrono::Utc>,
    ) -> Result<Record, Error> {
        let data: HashMap<String, serde_json::Value> = serde_json::from_str(data)
            .map_err(|e| Error::Backend(format!("Failed to deserialize session data: {}", e)))?;

        let expiry_date_timestamp = expiry_date.timestamp();
        let expiry_date = time::OffsetDateTime::from_unix_timestamp(expiry_date_timestamp)
            .map_err(|e| Error::Backend(format!("Invalid expiry date: {}", e)))?;

        let id = id
            .parse()
            .map_err(|e| Error::Backend(format!("Invalid session ID: {}", e)))?;

        Ok(Record {
            id,
            data,
            expiry_date,
        })
    }
}

#[async_trait]
impl SessionStore for DuckDBStore {
    async fn save(&self, session_record: &Record) -> Result<(), Error> {
        let conn = self.conn.lock().await;
        let id = session_record.id.to_string();
        let data = Self::record_to_json(session_record)?;

        let expiry_timestamp = session_record.expiry_date.unix_timestamp();
        let expiry_date = chrono::DateTime::from_timestamp(expiry_timestamp, 0)
            .ok_or_else(|| Error::Backend("Invalid expiry timestamp".to_string()))?;

        conn.execute(
            "INSERT OR REPLACE INTO sessions (id, data, expiry_date) VALUES (?, ?, ?)",
            duckdb::params![id, data, expiry_date],
        )
        .map_err(|e| Error::Backend(format!("Failed to save session: {}", e)))?;

        Ok(())
    }

    async fn load(&self, session_id: &Id) -> Result<Option<Record>, Error> {
        let conn = self.conn.lock().await;
        let id = session_id.to_string();

        let mut stmt = conn
            .prepare("SELECT id, data, expiry_date FROM sessions WHERE id = ?")
            .map_err(|e| Error::Backend(format!("Failed to prepare load query: {}", e)))?;

        let mut rows = stmt
            .query(duckdb::params![id])
            .map_err(|e| Error::Backend(format!("Failed to load session: {}", e)))?;

        if let Some(row) = rows
            .next()
            .map_err(|e| Error::Backend(format!("Failed to read session row: {}", e)))?
        {
            let id: String = row
                .get(0)
                .map_err(|e| Error::Backend(format!("Failed to read session ID: {}", e)))?;
            let data: String = row
                .get(1)
                .map_err(|e| Error::Backend(format!("Failed to read session data: {}", e)))?;
            let expiry_date: chrono::DateTime<chrono::Utc> = row
                .get(2)
                .map_err(|e| Error::Backend(format!("Failed to read expiry date: {}", e)))?;

            let now = chrono::Utc::now();
            if expiry_date < now {
                return Ok(None);
            }

            let record = Self::json_to_record(&id, &data, expiry_date)?;
            Ok(Some(record))
        } else {
            Ok(None)
        }
    }

    async fn delete(&self, session_id: &Id) -> Result<(), Error> {
        let conn = self.conn.lock().await;
        let id = session_id.to_string();

        conn.execute("DELETE FROM sessions WHERE id = ?", duckdb::params![id])
            .map_err(|e| Error::Backend(format!("Failed to delete session: {}", e)))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::init_database;
    use tempfile::TempDir;

    async fn create_test_store() -> (DuckDBStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = Arc::new(Mutex::new(init_database(&db_path)));
        let store = DuckDBStore::new(conn);
        (store, temp_dir)
    }

    fn create_test_record() -> Record {
        let mut data = HashMap::new();
        data.insert("user_id".to_string(), serde_json::json!("123"));
        data.insert("username".to_string(), serde_json::json!("test_user"));
        data.insert("role".to_string(), serde_json::json!("admin"));

        let id = Id::default();
        let expiry_date = time::OffsetDateTime::now_utc() + time::Duration::hours(24);

        Record {
            id,
            data,
            expiry_date,
        }
    }

    #[tokio::test]
    async fn test_save_and_load_session() {
        let (store, _temp_dir) = create_test_store().await;
        let record = create_test_record();

        store.save(&record).await.unwrap();

        let loaded = store.load(&record.id).await.unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.id, record.id);
        assert_eq!(loaded.data, record.data);
    }

    #[tokio::test]
    async fn test_load_nonexistent_session() {
        let (store, _temp_dir) = create_test_store().await;
        let id = Id::default();

        let loaded = store.load(&id).await.unwrap();
        assert_eq!(loaded, None);
    }

    #[tokio::test]
    async fn test_update_session() {
        let (store, _temp_dir) = create_test_store().await;
        let mut record = create_test_record();

        store.save(&record).await.unwrap();

        record
            .data
            .insert("new_key".to_string(), serde_json::json!("new_value"));
        store.save(&record).await.unwrap();

        let loaded = store.load(&record.id).await.unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.id, record.id);
        assert_eq!(
            loaded.data.get("new_key"),
            Some(&serde_json::json!("new_value"))
        );
    }

    #[tokio::test]
    async fn test_delete_session() {
        let (store, _temp_dir) = create_test_store().await;
        let record = create_test_record();

        store.save(&record).await.unwrap();
        store.delete(&record.id).await.unwrap();

        let loaded = store.load(&record.id).await.unwrap();
        assert_eq!(loaded, None);
    }

    #[tokio::test]
    async fn test_complex_session_data() {
        let (store, _temp_dir) = create_test_store().await;

        let mut data = HashMap::new();
        data.insert("string".to_string(), serde_json::json!("test"));
        data.insert("number".to_string(), serde_json::json!(42));
        data.insert("bool".to_string(), serde_json::json!(true));
        data.insert("null".to_string(), serde_json::json!(null));
        data.insert("array".to_string(), serde_json::json!([1, 2, 3]));
        data.insert("object".to_string(), serde_json::json!({"nested": "value"}));

        let id = Id::default();
        let expiry_date = time::OffsetDateTime::now_utc() + time::Duration::hours(1);
        let record = Record {
            id,
            data: data.clone(),
            expiry_date,
        };

        store.save(&record).await.unwrap();

        let loaded = store.load(&record.id).await.unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.id, record.id);
        assert_eq!(loaded.data, data);
    }

    #[tokio::test]
    async fn test_expired_session_returns_none() {
        let (store, _temp_dir) = create_test_store().await;

        let mut data = HashMap::new();
        data.insert("user_id".to_string(), serde_json::json!("123"));

        let id = Id::default();
        let expiry_date = time::OffsetDateTime::now_utc() - time::Duration::hours(1);

        let record = Record {
            id,
            data,
            expiry_date,
        };

        store.save(&record).await.unwrap();

        let loaded = store.load(&record.id).await.unwrap();
        assert_eq!(loaded, None, "Expired session should return None");
    }
}
