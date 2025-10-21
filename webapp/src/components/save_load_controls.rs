// SaveLoadControls component - handles game save/load functionality
use crate::{components::use_modal, state::AppState};
use automatafl_api_types::SnapshotInfo;
use leptos::prelude::*;
use uuid::Uuid;

#[component]
pub fn SaveLoadControls(game_id: Uuid) -> impl IntoView {
    let app_state = use_context::<AppState>().expect("AppState should be provided");
    let modal = use_modal();

    let (selected_snapshot, set_selected_snapshot) = signal(Option::<usize>::None);

    // Save game action with automatic toasts
    let app_state_for_save = app_state.clone();
    let save_action = crate::helpers::use_toast_action(
        move |gid: &Uuid| {
            let app_state = app_state_for_save.clone();
            let gid = *gid;
            async move {
                let client = app_state.get_api_client();
                client.save_game(gid).await
            }
        },
        Some("Game saved successfully!".to_string()),
        Some("Save failed".to_string()),
    );

    // Load game action with automatic toasts
    let app_state_for_load = app_state.clone();
    let load_action = crate::helpers::use_toast_action(
        move |(gid, idx): &(Uuid, usize)| {
            let app_state = app_state_for_load.clone();
            let gid = *gid;
            let idx = *idx;
            async move {
                let client = app_state.get_api_client();
                client
                    .load_game(gid, idx)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }
        },
        Some("Game loaded successfully!".to_string()),
        Some("Load failed".to_string()),
    );

    // Load snapshots resource - auto-refreshes when save_action completes
    let app_state_for_snapshots = app_state.clone();
    let snapshots_resource = LocalResource::new(move || {
        // Depend on save_action.version() to auto-refresh when saves complete
        save_action.version().get();

        let app_state = app_state_for_snapshots.clone();
        async move {
            let client = app_state.get_api_client();
            client.list_snapshots(game_id).await
        }
    });

    // Clear selected snapshot when load completes
    Effect::new(move |_| {
        load_action.version().track();
        if load_action
            .value()
            .with(|v| v.as_ref().is_some_and(|r| r.is_ok()))
        {
            set_selected_snapshot.set(None);
        }
    });

    view! {
        <div class="save-load-controls">
            <h3>"Save & Load Game"</h3>

            <div class="save-section">
                <button
                    class="button button-primary"
                    on:click=move |_| {
                        let _ = save_action.dispatch(game_id);
                    }
                    disabled=move || save_action.pending().get()
                >
                    {move || {
                        if save_action.pending().get() { "Saving..." } else { "Save Current State" }
                    }}
                </button>
            </div>

            <div class="load-section">
                <h4>"Load Previous State"</h4>
                <div class="snapshots-list">
                    <Suspense fallback=move || {
                        view! { <div class="loading">"Loading snapshots..."</div> }
                    }>
                        {move || {
                            snapshots_resource
                                .get()
                                .map(|result| {
                                    match result {
                                        Ok(response) => {
                                            if response.snapshots.is_empty() {
                                                view! {
                                                    <p class="no-snapshots">"No saved states available"</p>
                                                }
                                                    .into_any()
                                            } else {
                                                response
                                                    .snapshots
                                                    .into_iter()
                                                    .enumerate()
                                                    .map(|(idx, snapshot)| {
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
                                                    })
                                                    .collect_view()
                                                    .into_any()
                                            }
                                        }
                                        Err(e) => {
                                            view! {
                                                <p class="error">
                                                    "Failed to load snapshots: " {e.to_string()}
                                                </p>
                                            }
                                                .into_any()
                                        }
                                    }
                                })
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
    load_action: Action<(Uuid, usize), Result<(), String>>,
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
                {move || {
                    (selected_snapshot.get() == Some(idx))
                        .then(|| {
                            let modal_for_button = modal.clone();
                            view! {
                                <button
                                    class="button button-small button-primary"
                                    on:click=move |_| {
                                        modal_for_button
                                            .confirm(
                                                "Load Snapshot?",
                                                "Loading this snapshot will replace the current game state. Continue?",
                                                move || {
                                                    load_action.dispatch((game_id, idx));
                                                },
                                            );
                                    }
                                    disabled=move || load_action.pending().get()
                                >
                                    {move || {
                                        if load_action.pending().get() {
                                            "Loading..."
                                        } else {
                                            "Load"
                                        }
                                    }}
                                </button>
                            }
                        })
                }}
            </div>
        </div>
    }
}
