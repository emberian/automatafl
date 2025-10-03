// SaveLoadControls component - handles game save/load functionality
use crate::{
    components::{use_modal, use_toast},
    state::AppState,
};
use automatafl_api_types::SnapshotInfo;
use leptos::prelude::*;
use uuid::Uuid;

#[component]
pub fn SaveLoadControls(game_id: Uuid) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let toast = use_toast();
    let modal = use_modal();

    let (snapshots, set_snapshots) = signal(Vec::<SnapshotInfo>::new());
    let (selected_snapshot, set_selected_snapshot) = signal(Option::<usize>::None);

    // Save game action
    let app_state_for_save = app_state.clone();
    let save_action = Action::new_local(move |gid: &Uuid| {
        let app_state = app_state_for_save.clone();
        let gid = *gid;
        async move {
            let client = app_state.get_api_client();
            client.save_game(gid).await
        }
    });

    // Load snapshots action
    let app_state_for_load_snapshots = app_state.clone();
    let load_snapshots_action = Action::new_local(move |gid: &Uuid| {
        let app_state = app_state_for_load_snapshots.clone();
        let gid = *gid;
        async move {
            let client = app_state.get_api_client();
            client.list_snapshots(gid).await.map_err(|e| e.to_string())
        }
    });

    // Load game action
    let app_state_for_load = app_state.clone();
    let load_action = Action::new_local(move |(gid, idx): &(Uuid, usize)| {
        let app_state = app_state_for_load.clone();
        let gid = *gid;
        let idx = *idx;
        async move {
            let client = app_state.get_api_client();
            client.load_game(gid, idx).await
        }
    });

    // Handle save results
    let toast_save = toast.clone();
    Effect::new(move |_| {
        if let Some(result) = save_action.value().get() {
            match result {
                Ok(_) => {
                    toast_save.success("Game saved successfully!");
                    // Refresh snapshots list
                    load_snapshots_action.dispatch(game_id);
                }
                Err(e) => {
                    toast_save.error(format!("Save failed: {}", e));
                }
            }
        }
    });

    // Handle load snapshots results
    let toast_snapshots = toast.clone();
    Effect::new(move |_| {
        if let Some(result) = load_snapshots_action.value().get() {
            match result {
                Ok(response) => {
                    set_snapshots.set(response.snapshots);
                }
                Err(e) => {
                    toast_snapshots.error(format!("Failed to load snapshots: {}", e));
                }
            }
        }
    });

    // Handle load game results
    let toast_load = toast.clone();
    Effect::new(move |_| {
        if let Some(result) = load_action.value().get() {
            match result {
                Ok(_) => {
                    toast_load.success("Game loaded successfully!");
                    set_selected_snapshot.set(None);
                }
                Err(e) => {
                    toast_load.error(format!("Load failed: {}", e));
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
                            let snaps = snapshots.get();
                            if snaps.is_empty() {
                                view! {
                                    <p class="no-snapshots">"No saved states available"</p>
                                }.into_any()
                            } else {
                                snaps.into_iter().enumerate().map(|(idx, snapshot)| {
                                    view! {
                                        <SnapshotItem
                                            idx=idx
                                            snapshot=snapshot
                                            selected_snapshot=selected_snapshot
                                            set_selected_snapshot=set_selected_snapshot
                                            load_action=load_action
                                            modal=modal.clone()
                                            game_id=game_id
                                        />
                                    }
                                }).collect_view().into_any()
                            }
                        }}
                    </Suspense>
                </div>
            </div>
        </div>
    }
}

// Snapshot item component
#[component]
fn SnapshotItem(
    idx: usize,
    snapshot: SnapshotInfo,
    selected_snapshot: ReadSignal<Option<usize>>,
    set_selected_snapshot: WriteSignal<Option<usize>>,
    load_action: Action<(Uuid, usize), Result<(), automatafl_backend_client::ClientError>>,
    modal: crate::components::ModalContext,
    game_id: Uuid,
) -> impl IntoView {
    let timestamp = chrono::DateTime::from_timestamp(snapshot.timestamp as i64, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    view! {
        <div class=move || {
            let is_selected = selected_snapshot.get() == Some(idx);
            format!("snapshot-item {}", if is_selected { "selected" } else { "" })
        }>
            <div class="snapshot-info">
                <span class="snapshot-index">"#" {idx}</span>
                <span class="snapshot-time">{timestamp}</span>
            </div>
            <div class="snapshot-actions">
                <button
                    class="button button-small"
                    on:click=move |_| {
                        if selected_snapshot.get() == Some(idx) {
                            set_selected_snapshot.set(None);
                        } else {
                            set_selected_snapshot.set(Some(idx));
                        }
                    }
                >
                    {move || {
                        let is_selected = selected_snapshot.get() == Some(idx);
                        if is_selected { "Deselect" } else { "Select" }
                    }}
                </button>
                {move || (selected_snapshot.get() == Some(idx)).then(|| {
                    let modal_for_button = modal.clone();
                    view! {
                        <button
                            class="button button-small button-primary"
                            on:click=move |_| {
                                modal_for_button.confirm(
                                    "Load Snapshot?",
                                    "Loading this snapshot will replace the current game state. Continue?",
                                    move || { load_action.dispatch((game_id, idx)); }
                                );
                            }
                            disabled=move || load_action.pending().get()
                        >
                            {move || if load_action.pending().get() { "Loading..." } else { "Load" }}
                        </button>
                    }
                })}
            </div>
        </div>
    }
}
