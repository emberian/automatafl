//! Remote HTTP client for connecting `automatafl-client` to a live `automatafl-server` (`R5` / `R6`).

use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use automatafl_server::api::{
    ActionResponse, ApiError, CreateGameResponse, JoinGameResponse, ReplayExport,
};
use automatafl_server::bot::BotStrategy;
use automatafl_server::matchmaking::{SeekRequest, SeekStatus};
use automatafl_server::protocol::{Coord, GameConfig, Seat};
use automatafl_server::session::{GameView, LobbyGameSummary};

#[derive(Clone, Debug)]
pub struct RemoteClient {
    host_port: String,
}

impl RemoteClient {
    pub fn new(base_url: &str) -> Result<Self, String> {
        let stripped = base_url
            .trim()
            .strip_prefix("http://")
            .unwrap_or(base_url.trim())
            .trim_end_matches('/');
        if stripped.is_empty() {
            return Err("empty server URL".to_string());
        }
        let host_port = if stripped.contains(':') {
            stripped.to_string()
        } else {
            format!("{stripped}:80")
        };
        Ok(Self { host_port })
    }

    pub fn host_port(&self) -> &str {
        &self.host_port
    }

    async fn request_json<B: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<&B>,
    ) -> Result<R, String> {
        let mut stream = TcpStream::connect(&self.host_port)
            .await
            .map_err(|e| format!("failed to connect to {}: {e}", self.host_port))?;

        let payload = match body {
            Some(b) => serde_json::to_vec(b).map_err(|e| format!("serialize error: {e}"))?,
            None => Vec::new(),
        };

        let mut req = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
            self.host_port
        );
        if let Some(tok) = token {
            req.push_str(&format!("Authorization: Bearer {tok}\r\n"));
        }
        if body.is_some() {
            req.push_str("Content-Type: application/json\r\n");
            req.push_str(&format!("Content-Length: {}\r\n", payload.len()));
        } else {
            req.push_str("Content-Length: 0\r\n");
        }
        req.push_str("\r\n");

        stream
            .write_all(req.as_bytes())
            .await
            .map_err(|e| format!("write headers error: {e}"))?;
        if !payload.is_empty() {
            stream
                .write_all(&payload)
                .await
                .map_err(|e| format!("write body error: {e}"))?;
        }
        stream
            .flush()
            .await
            .map_err(|e| format!("flush error: {e}"))?;

        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .await
            .map_err(|e| format!("read response error: {e}"))?;

        let (status, resp_body) = parse_http_response(&raw)?;
        if !(200..300).contains(&status) {
            if let Ok(api_err) = serde_json::from_slice::<ApiError>(&resp_body) {
                return Err(format!("HTTP {status}: {}", api_err.error));
            }
            return Err(format!(
                "HTTP {status}: {}",
                String::from_utf8_lossy(&resp_body)
            ));
        }

        serde_json::from_slice(&resp_body).map_err(|e| {
            format!(
                "failed to decode response JSON ({e}): {}",
                String::from_utf8_lossy(&resp_body)
            )
        })
    }

    pub async fn list_games(&self) -> Result<Vec<LobbyGameSummary>, String> {
        self.request_json::<(), Vec<LobbyGameSummary>>("GET", "/api/games", None, None)
            .await
    }

    pub async fn create_game(
        &self,
        config: GameConfig,
        vs_bot: Option<BotStrategy>,
        creator_seat: Seat,
        player_name: Option<String>,
    ) -> Result<CreateGameResponse, String> {
        let body = serde_json::json!({
            "config": config,
            "vs_bot": vs_bot,
            "creator_seat": creator_seat,
            "player_name": player_name,
        });
        self.request_json("POST", "/api/games", None, Some(&body))
            .await
    }

    pub async fn join_game(
        &self,
        game_id: &str,
        preferred_seat: Option<Seat>,
        player_name: Option<String>,
    ) -> Result<JoinGameResponse, String> {
        let body = serde_json::json!({
            "preferred_seat": preferred_seat,
            "player_name": player_name,
        });
        self.request_json(
            "POST",
            &format!("/api/games/{game_id}/join"),
            None,
            Some(&body),
        )
        .await
    }

    pub async fn get_view(&self, game_id: &str, token: Option<&str>) -> Result<GameView, String> {
        self.request_json::<(), GameView>("GET", &format!("/api/games/{game_id}"), token, None)
            .await
    }

    pub async fn select_piece(
        &self,
        game_id: &str,
        token: &str,
        coord: Option<Coord>,
    ) -> Result<GameView, String> {
        let body = serde_json::json!({ "coord": coord });
        let resp: ActionResponse = self
            .request_json(
                "POST",
                &format!("/api/games/{game_id}/select"),
                Some(token),
                Some(&body),
            )
            .await?;
        Ok(resp.view)
    }

    pub async fn submit_move(
        &self,
        game_id: &str,
        token: &str,
        from: Coord,
        to: Coord,
    ) -> Result<GameView, String> {
        self.submit_move_with_revision(game_id, token, from, to, None)
            .await
    }

    pub async fn submit_move_with_revision(
        &self,
        game_id: &str,
        token: &str,
        from: Coord,
        to: Coord,
        expected_revision: Option<u64>,
    ) -> Result<GameView, String> {
        let body = serde_json::json!({
            "from": from,
            "to": to,
            "expected_revision": expected_revision,
        });
        let resp: ActionResponse = self
            .request_json(
                "POST",
                &format!("/api/games/{game_id}/move"),
                Some(token),
                Some(&body),
            )
            .await?;
        Ok(resp.view)
    }

    pub fn compute_commitment(
        game_id: &str,
        seat: Seat,
        turn_no: u32,
        round_step: u32,
        from: Coord,
        to: Coord,
        nonce: &str,
    ) -> String {
        automatafl_server::session::GameSession::compute_commitment(
            game_id,
            turn_no as u64,
            round_step as usize,
            seat,
            from,
            to,
            nonce,
        )
    }

    pub async fn commit_blind_hash(
        &self,
        game_id: &str,
        token: &str,
        commitment_hash: &str,
    ) -> Result<GameView, String> {
        let body = serde_json::json!({ "commitment_hash": commitment_hash });
        let resp: ActionResponse = self
            .request_json(
                "POST",
                &format!("/api/games/{game_id}/commit"),
                Some(token),
                Some(&body),
            )
            .await?;
        Ok(resp.view)
    }

    pub async fn reveal_move(
        &self,
        game_id: &str,
        token: &str,
        blind_details: Option<(Coord, Coord, String)>,
    ) -> Result<GameView, String> {
        let body = match blind_details {
            Some((from, to, nonce)) => {
                serde_json::json!({ "from": from, "to": to, "nonce": nonce })
            }
            None => serde_json::json!({}),
        };
        let resp: ActionResponse = self
            .request_json(
                "POST",
                &format!("/api/games/{game_id}/reveal"),
                Some(token),
                Some(&body),
            )
            .await?;
        Ok(resp.view)
    }

    pub async fn resolve_round(&self, game_id: &str, token: Option<&str>) -> Result<GameView, String> {
        let body = serde_json::json!({});
        let resp: ActionResponse = self
            .request_json(
                "POST",
                &format!("/api/games/{game_id}/resolve"),
                token,
                Some(&body),
            )
            .await?;
        Ok(resp.view)
    }

    pub async fn resign(&self, game_id: &str, token: &str) -> Result<GameView, String> {
        let body = serde_json::json!({});
        let resp: ActionResponse = self
            .request_json(
                "POST",
                &format!("/api/games/{game_id}/resign"),
                Some(token),
                Some(&body),
            )
            .await?;
        Ok(resp.view)
    }

    pub async fn export_replay(&self, game_id: &str) -> Result<ReplayExport, String> {
        self.request_json::<(), ReplayExport>(
            "GET",
            &format!("/api/games/{game_id}/replay"),
            None,
            None,
        )
        .await
    }

    pub async fn seek_matchmaking(&self, req: &SeekRequest) -> Result<SeekStatus, String> {
        self.request_json("POST", "/api/matchmaking/seek", None, Some(req))
            .await
    }

    pub async fn matchmaking_status(&self, ticket_id: &str) -> Result<SeekStatus, String> {
        self.request_json::<(), SeekStatus>(
            "GET",
            &format!("/api/matchmaking/status/{ticket_id}"),
            None,
            None,
        )
        .await
    }

    pub async fn health_check(&self) -> Result<bool, String> {
        let val: serde_json::Value = self
            .request_json::<(), serde_json::Value>("GET", "/api/health", None, None)
            .await?;
        Ok(val.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    pub async fn subscribe_events(
        &self,
        game_id: &str,
        token: Option<&str>,
    ) -> Result<SseSubscription, String> {
        let mut stream = TcpStream::connect(&self.host_port)
            .await
            .map_err(|e| format!("failed to connect SSE to {}: {e}", self.host_port))?;

        let mut req = format!(
            "GET /api/games/{game_id}/events HTTP/1.1\r\nHost: {}\r\nAccept: text/event-stream\r\nConnection: keep-alive\r\n",
            self.host_port
        );
        if let Some(tok) = token {
            req.push_str(&format!("Authorization: Bearer {tok}\r\n"));
        }
        req.push_str("\r\n");

        stream
            .write_all(req.as_bytes())
            .await
            .map_err(|e| format!("write SSE headers error: {e}"))?;
        stream
            .flush()
            .await
            .map_err(|e| format!("flush SSE error: {e}"))?;

        let mut reader = tokio::io::BufReader::new(stream);
        let mut status_line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut status_line)
            .await
            .map_err(|e| format!("read SSE status line error: {e}"))?;
        let status_code: u16 = status_line
            .split_whitespace()
            .nth(1)
            .ok_or_else(|| format!("malformed SSE status line: {status_line}"))?
            .parse()
            .map_err(|e| format!("invalid SSE status code: {e}"))?;
        if status_code != 200 {
            return Err(format!("SSE HTTP {status_code}: {status_line}"));
        }
        let mut is_chunked = false;
        loop {
            let mut line = String::new();
            let n = tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
                .await
                .map_err(|e| format!("read SSE header error: {e}"))?;
            if n == 0 || line == "\r\n" || line == "\n" {
                break;
            }
            let lower = line.to_ascii_lowercase();
            if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
                is_chunked = true;
            }
        }
        Ok(SseSubscription { reader, is_chunked })
    }
}

pub struct SseSubscription {
    reader: tokio::io::BufReader<TcpStream>,
    is_chunked: bool,
}

impl SseSubscription {
    pub async fn next_view(&mut self) -> Result<GameView, String> {
        use tokio::io::AsyncBufReadExt;
        loop {
            let mut line = String::new();
            let n = self
                .reader
                .read_line(&mut line)
                .await
                .map_err(|e| format!("SSE read error: {e}"))?;
            if n == 0 {
                return Err("SSE stream closed".to_string());
            }
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with(':') {
                continue;
            }
            if self.is_chunked && usize::from_str_radix(trimmed, 16).is_ok() {
                continue;
            }
            if let Some(data) = trimmed.strip_prefix("data:") {
                let json_str = data.trim();
                if !json_str.is_empty() {
                    let view: GameView = serde_json::from_str(json_str)
                        .map_err(|e| format!("SSE JSON decode error ({e}): {json_str}"))?;
                    return Ok(view);
                }
            }
        }
    }
}

fn parse_http_response(raw: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| "malformed HTTP response: missing header terminator".to_string())?;
    let header_str = String::from_utf8_lossy(&raw[..header_end]);
    let mut lines = header_str.lines();
    let status_line = lines.next().ok_or_else(|| "empty HTTP status line".to_string())?;
    let status_code: u16 = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| format!("malformed status line: {status_line}"))?
        .parse()
        .map_err(|e| format!("invalid status code in {status_line}: {e}"))?;

    let is_chunked = lines.any(|l| {
        let lower = l.to_ascii_lowercase();
        lower.starts_with("transfer-encoding:") && lower.contains("chunked")
    });

    let body_slice = &raw[header_end + 4..];
    if is_chunked {
        Ok((status_code, decode_chunked(body_slice)))
    } else {
        Ok((status_code, body_slice.to_vec()))
    }
}

fn decode_chunked(mut input: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(pos) = input.windows(2).position(|w| w == b"\r\n") {
        let len_str = String::from_utf8_lossy(&input[..pos]);
        let hex_part = len_str.split(';').next().unwrap_or("").trim();
        let Ok(chunk_len) = usize::from_str_radix(hex_part, 16) else {
            break;
        };
        if chunk_len == 0 {
            break;
        }
        let start = pos + 2;
        if start + chunk_len > input.len() {
            out.extend_from_slice(&input[start..]);
            break;
        }
        out.extend_from_slice(&input[start..start + chunk_len]);
        let next = start + chunk_len + 2;
        if next > input.len() {
            break;
        }
        input = &input[next..];
    }
    out
}
