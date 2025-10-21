# Webapp Performance Fixes - Summary

## Issues Identified and Fixed

### 1. ✅ NetworkStatus Event Listener Leak (CRITICAL)
**File**: `webapp/src/components/common.rs:279-308`

**Problem**:
- Effect created global `online`/`offline` event listeners using manual Closure management
- Closures were `.forget()`-ed, causing permanent memory leaks
- Each time the component mounted, new listeners accumulated
- Listeners never cleaned up, causing exponential performance degradation

**Fix**:
- Replaced manual Closure management with `window_event_listener` from Leptos
- `window_event_listener` automatically cleans up listeners when component unmounts
- Removed Effect wrapper - listeners are now set up directly in component body

```rust
// BEFORE: Manual Closure with .forget() - LEAKS MEMORY
Effect::new(move |_| {
    let callback = Closure::wrap(Box::new(move || { ... }));
    window.add_event_listener_with_callback("online", ...);
    callback.forget();  // ❌ LEAKED
});

// AFTER: Automatic cleanup
use leptos::prelude::window_event_listener;
window_event_listener(leptos::ev::online, move |_| {
    set_online.set(true);
    web_sys::console::log_1(&"📡 Network connection restored".into());
});
```

---

### 2. ✅ KeyboardContext Global Listener in Constructor (CRITICAL)
**Files**:
- `webapp/src/components/keyboard.rs:14-101`
- `webapp/src/components/mod.rs:20`
- `webapp/src/main.rs:10,69`

**Problem**:
- Global keydown listener added in `KeyboardContext::new()` constructor
- Listener was `.forget()`-ed and never removed
- Every time context was recreated, new global listener accumulated
- **Every keystroke fired ALL accumulated listeners**, causing severe input lag
- Especially noticeable during typing in login/register forms

**Fix**:
1. Removed `setup_listener()` call from `new()` constructor
2. Created new `KeyboardListener` component that uses `window_event_listener`
3. Added `<KeyboardListener />` to main App component
4. Listener now has proper cleanup when app unmounts

```rust
// BEFORE: Listener in constructor - ACCUMULATES
impl KeyboardContext {
    pub fn new() -> Self {
        let ctx = Self { ... };
        ctx.setup_listener();  // ❌ Adds listener every time
        ctx
    }
}

// AFTER: Listener in separate component
#[component]
pub fn KeyboardListener() -> impl IntoView {
    use leptos::prelude::window_event_listener;
    let kb_ctx = use_context::<KeyboardContext>().expect(...);

    window_event_listener(leptos::ev::keydown, move |e| {
        // ... handler logic
    });

    view! {}
}
```

---

### 3. ✅ ConnectionStatus Inefficient Subscriptions (HIGH PRIORITY)
**Files**:
- `webapp/src/state.rs:250-275,365-379`
- `webapp/src/components/common.rs:221-258`

**Problem**:
- All `get_*_signal()` methods called `self.games.get()` in reactive contexts
- Created subscriptions to the **entire games HashMap**
- Components re-rendered whenever ANY game was added/removed/modified
- ConnectionStatus in nav bar re-rendered on every games change
- Unnecessary re-renders even when no game was active (e.g., login page)

**Fix**:
- Changed all `get_*_signal()` methods to use `get_untracked()`
- Prevents subscribing to the HashMap - only subscribes to returned signal
- Components now only re-render when their specific game's signal changes

```rust
// BEFORE: Subscribes to entire HashMap
pub fn get_game_signal(&self, game_id: Uuid) -> Option<RwSignal<...>> {
    self.games.get().get(&game_id).map(|g| g.state)  // ❌ Subscribes to games
}

// AFTER: No subscription to HashMap
pub fn get_game_signal(&self, game_id: Uuid) -> Option<RwSignal<...>> {
    self.games.get_untracked().get(&game_id).map(|g| g.state)  // ✅ No subscription
}
```

**Methods Updated**:
- `get_game_signal()`
- `get_chat_signal()`
- `get_event_log_signal()`
- `get_websocket_connected_signal()`
- `get_move_events()`
- `get_conflict_events()`

---

## Files Changed

1. **webapp/src/components/common.rs**
   - Fixed NetworkStatus event listener leak

2. **webapp/src/components/keyboard.rs**
   - Removed `setup_listener()` from constructor
   - Added `KeyboardListener` component
   - Removed unused import

3. **webapp/src/components/mod.rs**
   - Exported `KeyboardListener`

4. **webapp/src/main.rs**
   - Imported `KeyboardListener`
   - Added `<KeyboardListener />` to app root

5. **webapp/src/state.rs**
   - Updated 6 methods to use `get_untracked()`
   - Added comments explaining the optimization

---

## Impact

### Before Fixes:
- ❌ Input lag during typing (especially in login/register forms)
- ❌ Hangs and hitches throughout the UI
- ❌ Performance degradation over time as listeners accumulated
- ❌ Excessive re-renders in nav bar and components
- ❌ Every keystroke checked entire shortcuts HashMap multiple times
- ❌ Network status listeners leaked indefinitely

### After Fixes:
- ✅ No event listener leaks - all use automatic cleanup
- ✅ Keyboard listener created exactly once
- ✅ Components only subscribe to signals they actually use
- ✅ No unnecessary subscriptions to HashMap changes
- ✅ Drastically reduced re-render frequency
- ✅ Input fields should be responsive and lag-free

---

## Testing Recommendations

1. **Verify listener cleanup**:
   ```javascript
   // In browser console, check event listeners
   getEventListeners(window)
   // Should see only one 'keydown', one 'online', one 'offline'
   ```

2. **Test input responsiveness**:
   - Type rapidly in login form
   - Should be no lag or dropped characters
   - Input should feel instant

3. **Monitor performance**:
   - Use browser DevTools Performance tab
   - Record typing in forms
   - JavaScript execution should be minimal

4. **Check re-render frequency**:
   - Add `console.log` to ConnectionStatus
   - Navigate between pages
   - Should only log when connection actually changes, not on every navigation

---

## Compilation Status

✅ All code compiles successfully
- 13 warnings about unused code (not related to fixes)
- No errors
- Ready for testing

---

## Additional Observations

### Toast Component
The toast component has minor inefficiencies (forgotten 10ms timeouts for entrance animations), but these are negligible compared to the critical issues fixed. The auto-dismiss pattern could be optimized to cancel timeouts on manual dismissal, but this is very low priority.

### Modal Component
The ModalContainer already uses `window_event_listener` for Escape key handling (fixed in previous work), so it has proper cleanup.

### Future Considerations
- Consider adding metrics/logging to track event listener count
- Could add development-mode warnings if multiple listeners detected
- May want to profile other components for similar issues
