// SaveLoadControls component - handles game save/load functionality
use crate::{api::ApiClient, state::AppState, components::{use_toast, use_modal}};
use leptos::prelude::*;
use uuid::Uuid;
use automatafl_api_types::SnapshotInfo;

#[component]
pub fn SaveLoadControls(game_id: Uuid) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let toast = use_toast();
    let modal = use_modal();
    
    let (snapshots, set_snapshots) = signal(Vec::<SnapshotInfo>::new());
    let (selected_snapshot, set_selected_snapshot) = signal(Option::<usize>::None);
    
    let api_base_url = app_state.api_base_url.clone();
    
    // Save game action
    let api_base_url_for_save = api_base_url.clone();
    let save_action = Action::new_local(move |gid: &Uuid| {
        let gid = *gid;
        let base_url = api_base_url_for_save.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.save_game(gid).await
        }
    });
    
    // Load snapshots action
    let api_base_url_for_snapshots = api_base_url.clone();
    let load_snapshots_action = Action::new_local(move |gid: &Uuid| {
        let gid = *gid;
        let base_url = api_base_url_for_snapshots.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.list_snapshots(gid).await.map_err(|e| e.to_string())
        }
    });
    
    // Load game action
    let api_base_url_for_load = api_base_url.clone();
    let load_action = Action::new_local(move |(gid, idx): &(Uuid, usize)| {
        let gid = *gid;
        let idx = *idx;
        let base_url = api_base_url_for_load.clone();
        async move {
            let client = ApiClient::new(base_url);
            client.load_game(gid, idx).await
        }
    });
    
    // Handle save results
    Effect::new(move |_| {
        if let Some(result) = save_action.value().get() {
            match result {
                Ok(_) => {
                    toast.success("Game saved successfully!");
                    // Refresh snapshots list
                    load_snapshots_action.dispatch(game_id);
                }
                Err(e) => {
                    toast.error(format!("Save failed: {}", e));
                }
            }
        }
    });
    
    // Handle load snapshots results
    Effect::new(move |_| {
        if let Some(result) = load_snapshots_action.value().get() {
            match result {
                Ok(response) => {
                    set_snapshots.set(response.snapshots);
                }
                Err(e) => {
                    toast.error(format!("Failed to load snapshots: {}", e));
                }
            }
        }
    });
    
    // Handle load game results
    Effect::new(move |_| {
        if let Some(result) = load_action.value().get() {
            match result {
                Ok(_) => {
                    toast.success("Game loaded successfully!");
                    set_selected_snapshot.set(None);
                }
                Err(e) => {
                    toast.error(format!("Load failed: {}", e));
                }
            }
        }
    });
    
    // Load snapshots on mount
    Effect::new(move |_| {
        let _ = load_snapshots_action.dispatch(game_id);
    });
    
    view! {
        <div class="save-load-controls">
            <h3>"Save & Load Game"</h3>
            
            <div class="save-section">
                <button
                    class="button button-primary"
                    on:click=move |_| { let _ = save_action.dispatch(game_id); }
                    disabled=move || save_action.pending().get()
                >
                    {move || if save_action.pending().get() { "Saving..." } else { "Save Current State" }}
                </button>
            </div>
            
            <div class="load-section">
                <h4>"Load Previous State"</h4>
                <div class="snapshots-list">
                    <Suspense fallback=move || view! {
                        <div class="loading">"Loading snapshots..."</div>
                    }>
                        {move || {
                            let modal = modal.clone();
                            let snaps = snapshots.get();
                            if snaps.is_empty() {
                                view! {
                                    <p class="no-snapshots">"No saved states available"</p>
                                }.into_any()
                            } else {
                                snaps.into_iter().enumerate().map(|(idx, snapshot)| {
                                    let modal = modal.clone();
                                    let timestamp = chrono::DateTime::from_timestamp(snapshot.timestamp as i64, 0)
                                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                                        .unwrap_or_else(|| "Unknown".to_string());

                                    let is_selected = selected_snapshot.get() == Some(idx);

                                    view! {
                                        <div class=format!("snapshot-item {}", if is_selected { "selected" } else { "" })>
                                            <div class="snapshot-info">
                                                <span class="snapshot-index">"#" {idx}</span>
                                                <span class="snapshot-time">{timestamp}</span>
                                            </div>
                                            <div class="snapshot-actions">
                                                <button
                                                    class="button button-small"
                                                    on:click=move |_| {
                                                        if is_selected {
                                                            set_selected_snapshot.set(None);
                                                        } else {
                                                            set_selected_snapshot.set(Some(idx));
                                                        }
                                                    }
                                                >
                                                    {if is_selected { "Deselect" } else { "Select" }}
                                                </button>
                                                <Show when=move || is_selected>
                                                    <button
                                                        class="button button-small button-primary"
                                                        on:click=move |_| {
                                                            modal.confirm(
                                                                "Load Snapshot?",
                                                                "Loading this snapshot will replace the current game state. Continue?",
                                                                move || { load_action.dispatch((game_id, idx)); }
                                                            );
                                                        }
                                                        disabled=move || load_action.pending().get()
                                                    >
                                                        {move || if load_action.pending().get() { "Loading..." } else { "Load" }}
                                                    </button>
                                                </Show>
                                            </div>
                                        </div>
                                    }
                                }).collect_view()
                            }
                        }}
                    </Suspense>
                </div>
            </div>
        </div>
    }
}

// (types now imported from automatafl_api_types at top)

