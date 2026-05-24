use crate::pipeline::{handle_ptt_press, handle_ptt_release, resolve_pending_dialog_selection};
use crate::state::{
    emit_processing, record_automation_event, AutomationEvent, ChatMessage, ConsoleError,
    ProcessingState,
};
use crate::{commands, AppState};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const AUTOMATION_ADDR: &str = "127.0.0.1:9877";

#[derive(Clone)]
struct AutomationState {
    app: AppHandle,
}

#[derive(Serialize)]
struct ErrorResponse {
    ok: bool,
    error: String,
}

struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn timeout(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::REQUEST_TIMEOUT,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                ok: false,
                error: self.message,
            }),
        )
            .into_response()
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

#[derive(Serialize)]
struct OkResponse {
    ok: bool,
}

#[derive(Serialize)]
struct TextAcceptedResponse {
    ok: bool,
    accepted: bool,
}

#[derive(Deserialize)]
struct TextRequest {
    text: String,
}

#[derive(Deserialize)]
struct DialogAnswerRequest {
    selected: String,
}

#[derive(Serialize)]
struct DialogAnswerResponse {
    ok: bool,
    selected: String,
}

#[derive(Deserialize)]
struct WaitRequest {
    condition: String,
    #[serde(default = "default_wait_timeout_ms")]
    timeout_ms: u64,
}

#[derive(Serialize)]
struct WaitResponse {
    ok: bool,
    condition: String,
    met: bool,
}

#[derive(Serialize)]
struct AutomationStateResponse {
    ok: bool,
    app_mode: String,
    dictation_active: bool,
    dictation_buffer: String,
    processing_stage: String,
    processing_text: String,
    is_recording: bool,
    message_count: usize,
    recent_messages: Vec<ChatMessage>,
    panel: Option<AutomationPanelState>,
    dialog: Option<AutomationDialogState>,
    tts_enabled: bool,
    llm_model: String,
}

#[derive(Serialize)]
struct AutomationPanelState {
    title: String,
    content_chars: usize,
}

#[derive(Serialize)]
struct AutomationDialogState {
    question: String,
    options: Vec<String>,
}

#[derive(Serialize)]
struct EventsResponse {
    ok: bool,
    events: Vec<AutomationEvent>,
}

#[derive(Serialize)]
struct ConsoleErrorsResponse {
    ok: bool,
    errors: Vec<ConsoleError>,
}

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = AutomationState { app: app.clone() };
        let router = Router::new()
            .route("/state", get(get_state))
            .route("/events", get(get_events))
            .route("/console/errors", get(get_console_errors))
            .route("/text", post(post_text))
            .route("/ptt/press", post(post_ptt_press))
            .route("/ptt/release", post(post_ptt_release))
            .route("/ptt/cancel", post(post_ptt_cancel))
            .route("/dialog/answer", post(post_dialog_answer))
            .route("/panel/close", post(post_panel_close))
            .route("/wait", post(post_wait))
            .route("/quit", post(post_quit))
            .with_state(state);

        let listener = match tokio::net::TcpListener::bind(AUTOMATION_ADDR).await {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("Automation API could not bind {}: {}", AUTOMATION_ADDR, err);
                return;
            }
        };

        record_automation_event(&app, "automation.started", AUTOMATION_ADDR);

        if let Err(err) = axum::serve(listener, router).await {
            eprintln!("Automation API stopped: {}", err);
        }
    });
}

async fn get_state(State(state): State<AutomationState>) -> ApiResult<AutomationStateResponse> {
    Ok(Json(snapshot_state(&state.app)))
}

async fn get_events(State(state): State<AutomationState>) -> ApiResult<EventsResponse> {
    let app_state = state.app.state::<AppState>();
    let events = app_state.automation_events.lock().unwrap().clone();
    Ok(Json(EventsResponse { ok: true, events }))
}

async fn get_console_errors(
    State(state): State<AutomationState>,
) -> ApiResult<ConsoleErrorsResponse> {
    let app_state = state.app.state::<AppState>();
    let errors = app_state.console_errors.lock().unwrap().clone();
    Ok(Json(ConsoleErrorsResponse { ok: true, errors }))
}

async fn post_text(
    State(state): State<AutomationState>,
    Json(req): Json<TextRequest>,
) -> ApiResult<TextAcceptedResponse> {
    let text = req.text.trim().to_string();
    if text.is_empty() {
        return Err(ApiError::bad_request("text must not be empty"));
    }

    commands::submit_text(state.app.clone(), text.clone()).map_err(ApiError::bad_request)?;
    record_automation_event(&state.app, "automation.text", truncate_for_log(&text, 120));
    Ok(Json(TextAcceptedResponse {
        ok: true,
        accepted: true,
    }))
}

async fn post_ptt_press(State(state): State<AutomationState>) -> ApiResult<OkResponse> {
    handle_ptt_press(&state.app);
    record_automation_event(&state.app, "automation.ptt_press", "PTT pressed");
    Ok(Json(OkResponse { ok: true }))
}

async fn post_ptt_release(State(state): State<AutomationState>) -> ApiResult<OkResponse> {
    handle_ptt_release(&state.app);
    record_automation_event(&state.app, "automation.ptt_release", "PTT released");
    Ok(Json(OkResponse { ok: true }))
}

async fn post_ptt_cancel(State(state): State<AutomationState>) -> ApiResult<OkResponse> {
    {
        let app_state = state.app.state::<AppState>();
        *app_state.is_recording.lock().unwrap() = false;
        app_state.recorded_samples.lock().unwrap().clear();
    }
    let _ = emit_processing(
        &state.app,
        ProcessingState {
            stage: "idle".to_string(),
            text: String::new(),
        },
    );
    record_automation_event(&state.app, "automation.ptt_cancel", "PTT cancelled");
    Ok(Json(OkResponse { ok: true }))
}

async fn post_dialog_answer(
    State(state): State<AutomationState>,
    Json(req): Json<DialogAnswerRequest>,
) -> ApiResult<DialogAnswerResponse> {
    let selected = resolve_pending_dialog_selection(&state.app, &req.selected)
        .map_err(ApiError::bad_request)?;
    record_automation_event(
        &state.app,
        "automation.dialog_answer",
        truncate_for_log(&selected, 120),
    );
    Ok(Json(DialogAnswerResponse { ok: true, selected }))
}

async fn post_panel_close(State(state): State<AutomationState>) -> ApiResult<OkResponse> {
    {
        let app_state = state.app.state::<AppState>();
        app_state.ui_state.lock().unwrap().panel = None;
    }
    if let Some(window) = state.app.get_webview_window("panel") {
        let _ = window.close();
    }
    record_automation_event(&state.app, "automation.panel_close", "Panel closed");
    Ok(Json(OkResponse { ok: true }))
}

async fn post_wait(
    State(state): State<AutomationState>,
    Json(req): Json<WaitRequest>,
) -> ApiResult<WaitResponse> {
    let timeout_ms = req.timeout_ms.clamp(1, 60_000);
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);

    loop {
        if condition_met(&state.app, &req.condition) {
            return Ok(Json(WaitResponse {
                ok: true,
                condition: req.condition,
                met: true,
            }));
        }
        if Instant::now() >= deadline {
            return Err(ApiError::timeout(format!(
                "condition '{}' was not met within {}ms",
                req.condition, timeout_ms
            )));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn post_quit(State(state): State<AutomationState>) -> ApiResult<OkResponse> {
    record_automation_event(&state.app, "automation.quit", "Quit requested");
    let app = state.app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        app.exit(0);
    });
    Ok(Json(OkResponse { ok: true }))
}

fn snapshot_state(app: &AppHandle) -> AutomationStateResponse {
    let state = app.state::<AppState>();
    let processing = state.processing.lock().unwrap().clone();
    let (message_count, recent_messages) = {
        let messages = state.messages.lock().unwrap();
        let recent_messages = messages.iter().rev().take(10).cloned().collect::<Vec<_>>();
        (
            messages.len(),
            recent_messages.into_iter().rev().collect::<Vec<_>>(),
        )
    };
    let panel = state
        .ui_state
        .lock()
        .unwrap()
        .panel
        .as_ref()
        .map(|panel| AutomationPanelState {
            title: panel.title.clone(),
            content_chars: panel.content.chars().count(),
        });
    let dialog =
        state
            .pending_dialog
            .lock()
            .unwrap()
            .as_ref()
            .map(|dialog| AutomationDialogState {
                question: dialog.question.clone(),
                options: dialog
                    .options
                    .iter()
                    .map(|option| option.label.clone())
                    .collect(),
            });
    let config = state.config.lock().unwrap();
    let tts_enabled = config.tts_enabled;
    let llm_model = config.llm_model.clone();
    drop(config);
    let is_recording = *state.is_recording.lock().unwrap();
    let app_mode = state.app_mode.lock().unwrap().to_string();
    let dictation_active = *state.dictation_active.lock().unwrap();
    let dictation_buffer = state.dictation_buffer.lock().unwrap().clone();

    AutomationStateResponse {
        ok: true,
        app_mode,
        dictation_active,
        dictation_buffer,
        processing_stage: processing.stage,
        processing_text: processing.text,
        is_recording,
        message_count,
        recent_messages,
        panel,
        dialog,
        tts_enabled,
        llm_model,
    }
}

fn condition_met(app: &AppHandle, condition: &str) -> bool {
    let state = app.state::<AppState>();
    match condition {
        "idle" => state.processing.lock().unwrap().stage == "idle",
        "recording" => *state.is_recording.lock().unwrap(),
        "dialog.shown" => state.pending_dialog.lock().unwrap().is_some(),
        "panel.open" => state.ui_state.lock().unwrap().panel.is_some(),
        "messages.changed" => !state.messages.lock().unwrap().is_empty(),
        _ => false,
    }
}

fn default_wait_timeout_ms() -> u64 {
    10_000
}

fn truncate_for_log(text: &str, max_chars: usize) -> String {
    let mut truncated = text.chars().take(max_chars).collect::<String>();
    if text.chars().count() > max_chars {
        truncated.push_str("...");
    }
    truncated
}
