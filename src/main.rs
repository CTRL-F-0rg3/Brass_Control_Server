use async_trait::async_trait;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get},
    Json, Router,
};
use russh::server::{Auth, Config, Handler, Server as RusshServer, Session};
use russh::{ChannelId, CryptoVec};
use russh_keys::key;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tower_http::cors::Any;

// --- MODEL DANYCH REST API ---

#[derive(Serialize, Deserialize, Clone)]
pub struct RepoInfo {
    pub name: String,
    pub object_count: usize,
    pub size_bytes: u64,
}

#[derive(Deserialize)]
pub struct CreateRepoRequest {
    pub name: String,
}

#[derive(Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub message: String,
    pub data: Option<T>,
}

// --- SILNIK ZARZĄDZANIA SERWEREM ---

pub struct AdminEngine {
    pub repos_root: PathBuf,
}

impl AdminEngine {
    pub fn new(repos_root: PathBuf) -> Self {
        Self { repos_root }
    }

    pub async fn create_repository(&self, name: &str) -> Result<String, String> {
        let repo_path = self.repos_root.join(name);
        if repo_path.exists() {
            return Err(format!("Repozytorium '{}' już istnieje.", name));
        }

        fs::create_dir_all(repo_path.join("objects"))
        .await
        .map_err(|e| e.to_string())?;
        fs::create_dir_all(repo_path.join("refs"))
        .await
        .map_err(|e| e.to_string())?;

        Ok(format!("Utworzono repozytorium bare: {}", name))
    }

    pub async fn list_repositories(&self) -> Result<Vec<RepoInfo>, String> {
        let mut entries = fs::read_dir(&self.repos_root)
        .await
        .map_err(|e| e.to_string())?;
        let mut result = Vec::new();

        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                let name = entry.file_name().to_string_lossy().to_string();
                let path = entry.path();

                let (obj_count, size) = Self::get_repo_stats(&path).await;
                result.push(RepoInfo {
                    name,
                    object_count: obj_count,
                    size_bytes: size,
                });
            }
        }
        Ok(result)
    }

    pub async fn delete_repository(&self, name: &str) -> Result<String, String> {
        let repo_path = self.repos_root.join(name);
        if !repo_path.exists() {
            return Err(format!("Repozytorium '{}' nie istnieje.", name));
        }

        fs::remove_dir_all(repo_path)
        .await
        .map_err(|e| e.to_string())?;
        Ok(format!("Usunięto repozytorium: {}", name))
    }

    async fn get_repo_stats(path: &PathBuf) -> (usize, u64) {
        let mut count = 0;
        let mut total_bytes = 0;

        let obj_dir = path.join("objects");
        if let Ok(mut entries) = fs::read_dir(obj_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Ok(meta) = entry.metadata().await {
                    if meta.is_file() {
                        count += 1;
                        total_bytes += meta.len();
                    }
                }
            }
        }
        (count, total_bytes)
    }
}

pub type SharedState = Arc<Mutex<AdminEngine>>;

// --- ROUTERY HTTP / REST API ---

async fn api_list_repos(State(state): State<SharedState>) -> impl IntoResponse {
    let engine = state.lock().await;
    match engine.list_repositories().await {
        Ok(repos) => (
            StatusCode::OK,
            Json(ApiResponse {
                success: true,
                message: "Pobrano listę repozytoriów".to_string(),
                 data: Some(repos),
            }),
        ),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse {
                success: false,
                message: err,
                data: None,
            }),
        ),
    }
}

async fn api_create_repo(
    State(state): State<SharedState>,
                         Json(payload): Json<CreateRepoRequest>,
) -> impl IntoResponse {
    let engine = state.lock().await;
    match engine.create_repository(&payload.name).await {
        Ok(msg) => (
            StatusCode::CREATED,
            Json(ApiResponse::<()> {
                success: true,
                message: msg,
                data: None,
            }),
        ),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::<()> {
                success: false,
                message: err,
                data: None,
            }),
        ),
    }
}

async fn api_delete_repo(
    State(state): State<SharedState>,
                         Path(name): Path<String>,
) -> impl IntoResponse {
    let engine = state.lock().await;
    match engine.delete_repository(&name).await {
        Ok(msg) => (
            StatusCode::OK,
            Json(ApiResponse::<()> {
                success: true,
                message: msg,
                data: None,
            }),
        ),
        Err(err) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::<()> {
                success: false,
                message: err,
                data: None,
            }),
        ),
    }
}

// --- SERWER SSH ---

#[derive(Clone)]
struct ServerApp {
    state: SharedState,
}

impl RusshServer for ServerApp {
    type Handler = SessionApp;

    fn new_client(&mut self, _peer_addr: Option<std::net::SocketAddr>) -> Self::Handler {
        SessionApp {
            state: self.state.clone(),
            exec_command: None,
        }
    }
}

struct SessionApp {
    state: SharedState,
    exec_command: Option<String>,
}

#[async_trait]
impl Handler for SessionApp {
    type Error = russh::Error;

    async fn auth_publickey(
        &mut self,
        _user: &str,
        _key: &key::PublicKey,
    ) -> Result<Auth, Self::Error> {
        Ok(Auth::Accept)
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        name: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let cmd = String::from_utf8_lossy(name).trim().to_string();
        self.exec_command = Some(cmd.clone());

        if cmd.starts_with("brass-admin repo list") {
            let engine = self.state.lock().await;
            let repos = engine.list_repositories().await.unwrap_or_default();
            let mut out = String::from("REPOSITORIES:\n");
            for r in repos {
                out.push_str(&format!(
                    " - {} (objects: {}, size: {} B)\n",
                                      r.name, r.object_count, r.size_bytes
                ));
            }
            session.data(channel, CryptoVec::from_slice(out.as_bytes()));
            session.exit_status_request(channel, 0);
            session.close(channel);
        }
        Ok(())
    }
}

// --- MAIN ---

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let repos_dir = std::env::current_dir()?.join("server_repositories");
    tokio::fs::create_dir_all(&repos_dir).await?;

    let state = Arc::new(Mutex::new(AdminEngine::new(repos_dir)));

    // 1. Uruchomienie REST API HTTP (Port 8080)
    let state_http = state.clone();
    tokio::spawn(async move {
        let app = Router::new()
        .route("/api/v1/repos", get(api_list_repos).post(api_create_repo))
        .route("/api/v1/repos/:name", delete(api_delete_repo))
        .layer(tower_http::cors::CorsLayer::new().allow_origin(Any))
        .with_state(state_http);

        let listener = TcpListener::bind("0.0.0.0:8080").await.unwrap();
        println!("[REST API] Publiczny interfejs HTTP nasłuchuje na http://0.0.0.0:8080");
        axum::serve(listener, app).await.unwrap();
    });

    // 2. Uruchomienie Serwera SSH (Port 2222)
    let host_key = key::KeyPair::generate_ed25519().unwrap();
    let config = Config {
        inactivity_timeout: Some(std::time::Duration::from_secs(3600)),
        keys: vec![host_key],
        ..Default::default()
    };

    let ssh_listener = TcpListener::bind("0.0.0.0:2222").await?;
    println!("[SSH] Usługa transmisji Brass Control nasłuchuje na porcie 2222");

    let mut server = ServerApp { state };
    let config = Arc::new(config);

    while let Ok((stream, peer_addr)) = ssh_listener.accept().await {
        let config = config.clone();
        let session_handler = server.new_client(Some(peer_addr));
        tokio::spawn(async move {
            let _ = russh::server::run_stream(config, stream, session_handler).await;
        });
    }

    Ok(())
}
