use anyhow::Result;
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

pub async fn connect(url: &str) -> Result<SqlitePool> {
    if let Some(path) = url.strip_prefix("sqlite://") {
        let file = path.split('?').next().unwrap_or(path);
        if let Some(parent) = std::path::Path::new(file).parent() {
            if !parent.as_os_str().is_empty() { tokio::fs::create_dir_all(parent).await?; }
        }
    }
    let pool = SqlitePoolOptions::new().max_connections(8).connect(url).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
