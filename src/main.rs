use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Mutex;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tower_http::services::ServeDir;
use uuid::Uuid;

#[derive(Deserialize)]
struct Config {
    bind: String,
    port: u16,
    db_path: String,
}

struct AppState {
    db: Mutex<Connection>,
}

#[derive(Serialize)]
struct Stats {
    visitors: u64,
    thumbs_up: u64,
    thumbs_down: u64,
    user_vote: Option<String>,
}

#[derive(Deserialize)]
struct VoteRequest {
    vote: String,
}

fn init_db(conn: &Connection) {
    println!("init_db: enter");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS visitors (
            id TEXT PRIMARY KEY,
            fingerprint TEXT NOT NULL,
            vote TEXT,
            first_seen TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )
    .expect("failed to initialize database");
    println!("init_db: exit");
}

fn visitor_id(headers: &HeaderMap) -> (Option<String>, String) {
    println!("visitor_id: enter");

    // Check for existing cookie
    let cookie_id = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|c| {
                let c = c.trim();
                c.strip_prefix("nw_id=").map(|id| id.to_string())
            })
        });

    // Build fingerprint from IP + User-Agent
    let ua = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");
    let ip = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");
    let mut hasher = Sha256::new();
    hasher.update(ip.as_bytes());
    hasher.update(ua.as_bytes());
    let fingerprint = format!("{:x}", hasher.finalize());

    println!("visitor_id: exit cookie_id={cookie_id:?}, fingerprint={fingerprint:?}");
    (cookie_id, fingerprint)
}

fn ensure_visitor(conn: &Connection, cookie_id: &Option<String>, fingerprint: &str) -> String {
    // If we have a cookie ID, check if it exists in the DB
    if let Some(id) = cookie_id {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM visitors WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .unwrap_or(false);
        if exists {
            return id.clone();
        }
    }

    // Check if fingerprint already exists
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM visitors WHERE fingerprint = ?1",
            [fingerprint],
            |row| row.get(0),
        )
        .ok();
    if let Some(id) = existing {
        return id;
    }

    // New visitor
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO visitors (id, fingerprint) VALUES (?1, ?2)",
        (&id, fingerprint),
    )
    .expect("failed to insert visitor");
    id
}

async fn get_stats(
    State(state): State<std::sync::Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let (cookie_id, fingerprint) = visitor_id(&headers);
    let db = state.db.lock().unwrap();

    let vid = ensure_visitor(&db, &cookie_id, &fingerprint);

    let visitors: u64 = db
        .query_row("SELECT COUNT(*) FROM visitors", [], |row| row.get(0))
        .unwrap_or(0);
    let thumbs_up: u64 = db
        .query_row(
            "SELECT COUNT(*) FROM visitors WHERE vote = 'up'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let thumbs_down: u64 = db
        .query_row(
            "SELECT COUNT(*) FROM visitors WHERE vote = 'down'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let user_vote: Option<String> = db
        .query_row("SELECT vote FROM visitors WHERE id = ?1", [&vid], |row| {
            row.get(0)
        })
        .unwrap_or(None);

    let cookie = format!("nw_id={vid}; Path=/; Max-Age=31536000; SameSite=Lax");
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(SET_COOKIE, cookie.parse().unwrap());

    (
        resp_headers,
        Json(Stats {
            visitors,
            thumbs_up,
            thumbs_down,
            user_vote,
        }),
    )
}

async fn post_vote(
    State(state): State<std::sync::Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<VoteRequest>,
) -> impl IntoResponse {
    println!("post_vote: enter");
    if body.vote != "up" && body.vote != "down" {
        return (StatusCode::BAD_REQUEST, "invalid vote").into_response();
    }

    let (cookie_id, fingerprint) = visitor_id(&headers);
    let db = state.db.lock().unwrap();
    let vid = ensure_visitor(&db, &cookie_id, &fingerprint);

    // Get current vote
    let current: Option<String> = db
        .query_row("SELECT vote FROM visitors WHERE id = ?1", [&vid], |row| {
            row.get(0)
        })
        .unwrap_or(None);

    // Toggle: same vote again removes it, different vote switches
    let new_vote: Option<&str> = if current.as_deref() == Some(&body.vote) {
        None
    } else {
        Some(&body.vote)
    };

    db.execute(
        "UPDATE visitors SET vote = ?1 WHERE id = ?2",
        (new_vote, &vid),
    )
    .expect("failed to update vote");
    println!("post_vote: new_vote={new_vote:?}");

    let cookie = format!("nw_id={vid}; Path=/; Max-Age=31536000; SameSite=Lax");
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(SET_COOKIE, cookie.parse().unwrap());

    // Return updated stats
    let visitors: u64 = db
        .query_row("SELECT COUNT(*) FROM visitors", [], |row| row.get(0))
        .unwrap_or(0);
    let thumbs_up: u64 = db
        .query_row(
            "SELECT COUNT(*) FROM visitors WHERE vote = 'up'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let thumbs_down: u64 = db
        .query_row(
            "SELECT COUNT(*) FROM visitors WHERE vote = 'down'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    println!("post_vote: exit visitors={visitors:?}, thumbs_up={thumbs_up:?}, thumbs_down={thumbs_down:?}");
    (
        resp_headers,
        Json(Stats {
            visitors,
            thumbs_up,
            thumbs_down,
            user_vote: new_vote.map(|s| s.to_string()),
        }),
    )
        .into_response()
}

#[tokio::main]
async fn main() {
    println!("main: enter");

    let config_str =
        fs::read_to_string("site/config.toml").expect("failed to read site/config.toml");
    let config: Config = toml::from_str(&config_str).expect("failed to parse config");
    println!("main: config_str={config_str:?}");

    // Ensure DB directory exists
    if let Some(parent) = PathBuf::from(&config.db_path).parent() {
        fs::create_dir_all(parent).expect("failed to create db directory");
    }

    let conn = Connection::open(&config.db_path).expect("failed to open database");
    init_db(&conn);

    let state = std::sync::Arc::new(AppState {
        db: Mutex::new(conn),
    });

    let app = Router::new()
        .route("/api/stats", get(get_stats))
        .route("/api/vote", post(post_vote))
        .fallback_service(ServeDir::new("site"))
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", config.bind, config.port)
        .parse()
        .expect("invalid bind address");
    println!("Listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind");
    axum::serve(listener, app).await.expect("server error");

    println!("main: exit");
}
