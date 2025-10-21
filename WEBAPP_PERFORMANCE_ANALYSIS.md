# Webapp Performance Issues - Root Cause Analysis

## Executive Summary

The webapp has **severe performance issues** causing hangs and hitches throughout the UI, especially during input interactions. Analysis reveals multiple **catastrophic event listener leaks** and **inefficient reactive patterns** that compound to create pervasive performance degradation.

---

## 🔴 CRITICAL ISSUES

### 1. **NetworkStatus: Event Listener Leak (CATASTROPHIC)**
**File**: `webapp/src/components/common.rs:279-323`

```rust
#[component]
pub fn NetworkStatus() -> impl IntoView {
    let (is_online, set_is_online) = signal(true);

    Effect::new(move |_| {
        let window = web_sys::window().expect("window should exist");

        let online_callback = Closure::wrap(Box::new(move || {
            set_is_online.set(true);
        }) as Box<dyn Fn()>);

        let offline_callback = Closure::wrap(Box::new(move || {
            set_is_online.set(false);
        }) as Box<dyn Fn()>);

        let _ = window
            .add_event_listener_with_callback("online", online_callback.as_ref().unchecked_ref());
        let _ = window
            .add_event_listener_with_callback("offline", offline_callback.as_ref().unchecked_ref());

        // Leak closures to keep them alive
        online_callback.forget();  // ❌ LEAKED - NEVER CLEANED UP
        offline_callback.forget(); // ❌ LEAKED - NEVER CLEANED UP
    });
```

**Problem**:
- Effect creates global `online`/`offline` event listeners with `.forget()`
- Listeners are **NEVER removed**
- If NetworkStatus component is ever recreated (route changes, parent re-renders), **new listeners accumulate**
- Each listener fires on every network event, triggering signal updates
- NetworkStatus is rendered in main.rs:70 inside the Router - could be recreated on route changes

**Impact**: Exponential performance degradation as listeners accumulate

---

### 2. **KeyboardContext: Global Event Listener in Constructor**
**File**: `webapp/src/components/keyboard.rs:14-77`

```rust
impl KeyboardContext {
    pub fn new() -> Self {
        let ctx = Self {
            shortcuts: RwSignal::new(HashMap::new()),
            help_visible: RwSignal::new(false),
        };

        // Setup global keyboard listener
        ctx.setup_listener();  // ❌ ADDS LISTENER IN CONSTRUCTOR
        ctx
    }

    fn setup_listener(&self) {
        // ... create Closure
        let _ = document.add_event_listener_with_callback("keydown", callback.as_ref().unchecked_ref());
        callback.forget();  // ❌ LEAKED - NEVER CLEANED UP
    }
}
```

**Problem**:
- Global keydown listener added in `new()` constructor
- Listener is `.forget()`-ed and **never removed**
- Every time `KeyboardContext::new()` is called, a new global listener is added
- Even though wrapped in `StoredValue` in main.rs, if App component function runs multiple times (HMR, errors, framework edge cases), listeners accumulate
- **Every keystroke fires ALL accumulated listeners**, checking shortcuts HashMap

**Impact**:
- Explains lag during typing in login form
- Gets worse over time or across navigation
- Each listener checks entire shortcuts HashMap on every keystroke

---

### 3. **ConnectionStatus: Inefficient Signal Access Pattern**
**File**: `webapp/src/components/common.rs:221-258`

```rust
view! {
    <div class="connection-status">
        {move || {
            let active_game_id = app_state.active_game_id.get();  // Subscribe
            let connected = active_game_id
                .and_then(|game_id| app_state.get_websocket_connected_signal(game_id))
                .map(|sig| sig.get())
                .unwrap_or(false);
```

Combined with `state.rs:266-271`:
```rust
pub fn get_websocket_connected_signal(&self, game_id: Uuid) -> Option<RwSignal<bool>> {
    self.games
        .get()  // ❌ CREATES SUBSCRIPTION TO ENTIRE GAMES HASHMAP
        .get(&game_id)
        .map(|g| g.websocket_connected)
}
```

**Problem**:
- `get_websocket_connected_signal()` calls `self.games.get()` in reactive context
- Creates subscription to the **entire `games` HashMap**
- ConnectionStatus re-runs whenever ANY game is added/removed/modified
- ConnectionStatus is in the nav bar (main.rs:98) - rendered on **every page**
- Unnecessary re-renders even when no game is active (like on login page)

**Impact**:
- Excessive re-renders across all pages
- Especially bad when games HashMap changes frequently
- Creates layout thrash in the nav bar

---

### 4. **Modal: Event Listener on Every Render**
**File**: `webapp/src/components/modal.rs:141-147`

```rust
#[component]
pub fn ModalContainer() -> impl IntoView {
    let modal_ctx = use_context::<ModalContext>().expect("ModalContext should be provided");

    let modal_ctx_for_listener = modal_ctx.clone();
    window_event_listener(leptos::ev::keydown, move |e: web_sys::KeyboardEvent| {
        if modal_ctx_for_listener.is_open.get() && e.key() == "Escape" {
            modal_ctx_for_listener.close();
        }
    });
```

**Problem**:
- This code is in the component body, so it runs every time ModalContainer renders
- `window_event_listener` should auto-cleanup, but if component re-renders frequently, could still create temporary performance issues
- The listener checks `modal_ctx_for_listener.is_open.get()` on **every Escape press**, even when modal is closed

**Impact**:
- Moderate - depends on re-render frequency
- Adds overhead to every Escape keypress

---

### 5. **Toast: Forgotten Timeouts**
**File**: `webapp/src/components/toast.rs:121-126`

```rust
Effect::new(move |_| {
    gloo_timers::callback::Timeout::new(10, move || {
        set_is_visible.set(true);
    })
    .forget();  // ❌ TIMEOUT LEAKS
});
```

**Problem**:
- Every toast creates a forgotten timeout
- Timeout fires even if toast is removed
- Tries to set signal after component is gone (should be safe in Leptos, but wasteful)

**Impact**:
- Minor - just 10ms timeouts
- Could accumulate with many toasts

---

## 🟡 MODERATE ISSUES

### 6. **Multiple Signal Access Methods Create Redundant Subscriptions**
**File**: `webapp/src/state.rs:250-271`

Methods like `get_game_signal()`, `get_chat_signal()`, `get_websocket_connected_signal()` all call `self.games.get()`:

```rust
pub fn get_game_signal(&self, game_id: Uuid) -> Option<RwSignal<Option<GameStateResponse>>> {
    self.games.get().get(&game_id).map(|g| g.state)  // Creates subscription to entire HashMap
}
```

**Problem**:
- Each method call creates a subscription to the entire `games` HashMap
- Components subscribe to games changes even when they only care about one game
- HashMap changes trigger re-runs of all components using these methods

**Impact**:
- Unnecessary re-renders
- Could use `.with_untracked()` or `.get_untracked()` since we only want the inner signal

---

### 7. **AppState Validation on Every Startup**
**File**: `webapp/src/state.rs:147-179`

```rust
// Validate session on startup if we have one
if session_token.is_some() {
    app_state.validate_session();  // Spawns async task, hits localStorage
}

pub fn validate_session(&self) {
    // ...
    leptos::task::spawn_local(async move {
        match client.get_matchmaking_status().await {  // API call
            Ok(_) => { /* ... */ }
            Err(_) => {
                app_state.logout();  // Updates multiple signals, hits localStorage
            }
        }
    });
}
```

**Problem**:
- Runs on every app startup
- If API is slow or fails, delays app initialization
- `logout()` updates multiple signals and accesses localStorage
- No loading state shown to user

**Impact**:
- Initial load delays
- localStorage access can be slow in some browsers
- Multiple signal updates trigger dependent effects

---

## 🔧 RECOMMENDED FIXES

### Fix 1: NetworkStatus - Use window_event_listener
```rust
#[component]
pub fn NetworkStatus() -> impl IntoView {
    use leptos::prelude::window_event_listener;
    let (is_online, set_is_online) = signal(true);

    // Auto-cleanup when component unmounts
    let set_online = set_is_online.clone();
    window_event_listener(leptos::ev::online, move |_| {
        set_online.set(true);
        web_sys::console::log_1(&"📡 Network connection restored".into());
    });

    let set_offline = set_is_online.clone();
    window_event_listener(leptos::ev::offline, move |_| {
        set_offline.set(false);
        web_sys::console::warn_1(&"📡 Network connection lost".into());
    });

    // ... view
}
```

### Fix 2: KeyboardContext - Move Listener to Component
```rust
// Remove setup_listener() from new()
impl KeyboardContext {
    pub fn new() -> Self {
        Self {
            shortcuts: RwSignal::new(HashMap::new()),
            help_visible: RwSignal::new(false),
        }
    }
}

// Create separate component for the listener
#[component]
pub fn KeyboardListener() -> impl IntoView {
    use leptos::prelude::window_event_listener;
    let kb_ctx = use_context::<KeyboardContext>().expect("KeyboardContext should be provided");

    let shortcuts_clone = kb_ctx.shortcuts;
    let help_visible = kb_ctx.help_visible;

    window_event_listener(leptos::ev::keydown, move |e: web_sys::KeyboardEvent| {
        // ... handler logic
    });

    view! {}
}

// Add <KeyboardListener /> to main.rs alongside KeyboardShortcutsHelp
```

### Fix 3: ConnectionStatus - Use get_untracked
```rust
pub fn get_websocket_connected_signal(&self, game_id: Uuid) -> Option<RwSignal<bool>> {
    self.games
        .get_untracked()  // ✅ NO SUBSCRIPTION - we only want the signal
        .get(&game_id)
        .map(|g| g.websocket_connected)
}
```

Apply same pattern to all get_*_signal methods in state.rs.

### Fix 4: Toast - Proper Cleanup
```rust
Effect::new(move |_| {
    let timeout = gloo_timers::callback::Timeout::new(10, move || {
        set_is_visible.set(true);
    });

    // Return cleanup function
    move || {
        timeout.cancel();
    }
});
```

---

## TESTING PLAN

1. **Verify listener accumulation**:
   - Add console.log to each listener setup
   - Navigate between pages
   - Check how many listeners fire on each event

2. **Profile performance**:
   - Use browser devtools Performance tab
   - Record typing in login form
   - Look for excessive JavaScript execution

3. **Monitor signal subscriptions**:
   - Add logging to signal .get() calls
   - Watch for unexpected re-runs

4. **Test fixes incrementally**:
   - Fix NetworkStatus first (most obvious)
   - Fix KeyboardContext second (affects typing)
   - Fix ConnectionStatus third (affects all pages)
   - Verify each fix before proceeding

---

## PRIORITY

1. 🔴 **IMMEDIATE**: NetworkStatus event listener leak
2. 🔴 **IMMEDIATE**: KeyboardContext global listener in constructor
3. 🟡 **HIGH**: ConnectionStatus inefficient subscriptions
4. 🟡 **MEDIUM**: All get_*_signal methods using .get() instead of .get_untracked()
5. 🟢 **LOW**: Toast timeout cleanup
6. 🟢 **LOW**: Modal listener optimization
