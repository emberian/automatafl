//! Responsive Web / Desktop / Mobile UI for Automatafl (`R6`).
//!
//! Self-contained zero-build HTML5 + CSS Grid + SVG + EventSource (SSE) single-page
//! interface inspired by Lichess.
//!
//! Client-side offloading features:
//! - **Zero-round-trip piece selection & target/sightline/forecast calculation**:
//!   Clicking a piece computes rook-line `legal_targets`, `executable_targets` (respecting
//!   conflict marks `×`, cooldown squares `❄️`, and inclusive path blocking), sightline
//!   raycasts, automaton pulse preview, and counterfactual single-move forecasts in 0ms
//!   directly in the browser with zero network requests.
//! - **Client-Side Local AI (Zero Server Load)**:
//!   Full in-browser simultaneous-move rules engine + Web Worker AI bot search (`decoupled_uct`,
//!   `regret_matching`, `minimax`, `tactical`, `novice`) so single-player games against AI bots
//!   consume zero server CPU and zero server RAM, plus optional local AI move assistance on
//!   any table.
//! - **Compact Wire Rendering**:
//!   Renders the board directly from `board.cells`, `marks`, `goals`, and `cooldown_squares`
//!   without needing 121 serialized `CellView` objects over the network.

pub const INDEX_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Automatafl — Verified Simultaneous-Move Platform</title>
<style>
  :root {
    --bg: #161512;
    --panel: #262421;
    --panel-alt: #302e2c;
    --border: #3d3934;
    --text: #bababa;
    --text-bright: #ffffff;
    --accent: #629924;
    --accent-hover: #73b12b;
    --amber: #d59120;
    --danger: #cc3333;
    --frost: #4da6ff;
    --seat-a: #3893e8;
    --seat-b: #e68a00;
    --board-light: #2b2926;
    --board-dark: #22201d;
    --board-sight: #333028;
    --repulsor: #d9534f;
    --attractor: #428bca;
    --automaton: #f0ad4e;
  }
  * { box-sizing: border-box; }
  body {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
    background: var(--bg);
    color: var(--text);
    line-height: 1.45;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.75rem 1.25rem;
    background: var(--panel);
    border-bottom: 1px solid var(--border);
  }
  .brand {
    display: flex;
    align-items: baseline;
    gap: 0.75rem;
    text-decoration: none;
    color: var(--text-bright);
    font-weight: 700;
    font-size: 1.2rem;
    letter-spacing: 0.02em;
  }
  .brand span.sub {
    font-size: 0.78rem;
    font-weight: 400;
    color: var(--text);
    background: var(--panel-alt);
    padding: 0.15rem 0.5rem;
    border-radius: 4px;
  }
  .container {
    max-width: 1320px;
    margin: 0 auto;
    padding: 1rem;
  }
  .lobby-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 1.25rem;
  }
  .game-grid {
    display: grid;
    grid-template-columns: minmax(320px, 680px) 1fr;
    gap: 1.25rem;
    align-items: start;
  }
  @media (max-width: 960px) {
    .lobby-grid, .game-grid { grid-template-columns: 1fr; }
  }
  .card {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 1rem;
    margin-bottom: 1rem;
  }
  .card h2, .card h3 {
    margin-top: 0;
    color: var(--text-bright);
    font-size: 1.05rem;
    border-bottom: 1px solid var(--border);
    padding-bottom: 0.5rem;
  }
  .form-row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.75rem;
    margin-bottom: 0.75rem;
  }
  label {
    display: block;
    font-size: 0.82rem;
    margin-bottom: 0.25rem;
    color: var(--text);
  }
  select, input[type="text"], input[type="number"] {
    width: 100%;
    padding: 0.45rem 0.6rem;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text-bright);
    font-size: 0.9rem;
  }
  .checkbox-row {
    display: flex;
    gap: 1rem;
    flex-wrap: wrap;
    margin: 0.6rem 0;
  }
  .checkbox-row label {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    cursor: pointer;
    color: var(--text-bright);
  }
  button, .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 0.4rem;
    padding: 0.5rem 0.9rem;
    background: var(--accent);
    color: #fff;
    border: none;
    border-radius: 4px;
    font-weight: 600;
    font-size: 0.9rem;
    cursor: pointer;
    text-decoration: none;
  }
  button:hover, .btn:hover { background: var(--accent-hover); }
  button.secondary, .btn.secondary {
    background: var(--panel-alt);
    border: 1px solid var(--border);
    color: var(--text-bright);
  }
  button.danger { background: var(--danger); }
  button:disabled { opacity: 0.45; cursor: not-allowed; }
  .board-wrap {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.75rem;
  }
  .board-grid {
    display: grid;
    gap: 2px;
    background: #111;
    border: 2px solid var(--border);
    aspect-ratio: 1 / 1;
    width: 100%;
    max-width: 640px;
    margin: 0 auto;
    user-select: none;
  }
  .sq {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--board-light);
    cursor: pointer;
    font-weight: 700;
    transition: background 0.08s ease;
  }
  .sq.dark { background: var(--board-dark); }
  .sq.sightline { background: var(--board-sight); }
  .sq:hover { outline: 2px solid #888; z-index: 2; }
  .sq.selected {
    background: #3d5223 !important;
    outline: 2px solid #96df3a;
    z-index: 3;
  }
  .sq.target-exec::after {
    content: "";
    width: 30%;
    height: 30%;
    border-radius: 50%;
    background: rgba(115, 195, 45, 0.85);
    box-shadow: 0 0 6px rgba(115, 195, 45, 0.9);
  }
  .sq.target-blocked::after {
    content: "";
    width: 28%;
    height: 28%;
    border-radius: 50%;
    border: 2px dashed var(--amber);
    background: rgba(213, 145, 32, 0.25);
  }
  .sq.marked {
    background: rgba(204, 51, 51, 0.28) !important;
    box-shadow: inset 0 0 0 2px var(--danger);
  }
  .sq.cooldown {
    box-shadow: inset 0 0 0 2px var(--frost);
    background: rgba(77, 166, 255, 0.14);
  }
  .sq.last-move-src {
    background: rgba(213, 145, 32, 0.18);
  }
  .sq.last-move-dst {
    background: rgba(213, 145, 32, 0.28);
    box-shadow: inset 0 0 0 2px rgba(213, 145, 32, 0.65);
  }
  .sq.last-auto-from {
    box-shadow: inset 0 0 0 2px rgba(240, 173, 78, 0.55);
  }
  .mark-x {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #ff5555;
    font-size: 1.55rem;
    font-weight: 900;
    pointer-events: none;
    text-shadow: 0 0 4px #000;
  }
  .cooldown-badge {
    position: absolute;
    top: 1px;
    right: 2px;
    font-size: 0.68rem;
    line-height: 1;
    pointer-events: none;
    z-index: 2;
    filter: drop-shadow(0 0 2px #000);
  }
  .goal-badge {
    position: absolute;
    top: 2px;
    left: 3px;
    font-size: 0.65rem;
    font-weight: 800;
    padding: 0 3px;
    border-radius: 2px;
    line-height: 1.1;
  }
  .goal-a { background: rgba(56, 147, 232, 0.25); color: #6cb8ff; border: 1px solid var(--seat-a); }
  .goal-b { background: rgba(230, 138, 0, 0.25); color: #ffb84d; border: 1px solid var(--seat-b); }
  .coord-label {
    position: absolute;
    bottom: 1px;
    right: 2px;
    font-size: 0.56rem;
    color: #666;
    font-weight: 400;
    pointer-events: none;
  }
  .piece {
    width: 74%;
    height: 74%;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 0.95rem;
    font-weight: 800;
    color: #fff;
    box-shadow: 0 2px 5px rgba(0,0,0,0.55);
    z-index: 1;
  }
  .piece-repulsor {
    background: radial-gradient(circle at 32% 32%, #ff7672, var(--repulsor));
    border: 2px solid #ffb3b0;
  }
  .piece-attractor {
    background: radial-gradient(circle at 32% 32%, #6eb4f7, var(--attractor));
    border: 2px solid #b5dcff;
  }
  .piece-automaton {
    background: radial-gradient(circle at 32% 32%, #ffe082, var(--automaton));
    border: 2px solid #fff3cd;
    color: #1c1608;
    border-radius: 22%;
  }
  .own-move-src { box-shadow: inset 0 0 0 3px #6cb8ff !important; }
  .own-move-dst { box-shadow: inset 0 0 0 3px #73c32d !important; }
  .badge {
    display: inline-block;
    padding: 0.18rem 0.5rem;
    border-radius: 4px;
    font-size: 0.78rem;
    font-weight: 600;
    background: var(--panel-alt);
    color: var(--text-bright);
    margin-right: 0.35rem;
    margin-bottom: 0.25rem;
  }
  .badge.fog-sealed { background: #3b2e58; color: #d6bfff; border: 1px solid #6e54a3; }
  .badge.fog-locked { background: #264a36; color: #98f5be; border: 1px solid #3c7d57; }
  .badge.clash { background: #5a1e1e; color: #ff9e9e; border: 1px solid var(--danger); }
  .badge.frost { background: #193854; color: #8cd0ff; border: 1px solid var(--frost); }
  .seat-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.65rem 0.85rem;
    border-radius: 5px;
    background: var(--panel-alt);
    margin-bottom: 0.6rem;
    border-left: 4px solid var(--border);
  }
  .seat-card.seat-0 { border-left-color: var(--seat-a); }
  .seat-card.seat-1 { border-left-color: var(--seat-b); }
  .table-list {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.88rem;
  }
  .table-list th, .table-list td {
    text-align: left;
    padding: 0.5rem;
    border-bottom: 1px solid var(--border);
  }
  .mono { font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace; }
  .status-banner {
    padding: 0.65rem 0.85rem;
    border-radius: 5px;
    margin-bottom: 0.75rem;
    font-weight: 600;
    background: var(--panel-alt);
    color: var(--text-bright);
    border: 1px solid var(--border);
  }
  .history-log {
    max-height: 210px;
    overflow-y: auto;
    font-size: 0.82rem;
    background: var(--bg);
    padding: 0.5rem;
    border-radius: 4px;
    border: 1px solid var(--border);
  }
  .history-entry {
    padding: 0.22rem 0.35rem;
    border-bottom: 1px solid #262421;
  }
  .history-entry:last-child { border-bottom: none; }
  .nav-right {
    display: flex;
    align-items: center;
    gap: 0.65rem;
    flex-wrap: wrap;
  }
  .nav-pill {
    display: inline-flex;
    align-items: center;
    gap: 0.55rem;
    padding: 0.36rem 0.7rem;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 5px;
    font-size: 0.78rem;
    color: var(--text);
    text-decoration: none;
    transition: border-color 0.15s ease;
  }
  .nav-pill:hover {
    border-color: var(--accent);
    color: var(--text-bright);
  }
  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #629924;
    box-shadow: 0 0 6px #629924;
    display: inline-block;
  }
  .status-dot.warn {
    background: var(--amber);
    box-shadow: 0 0 6px var(--amber);
  }
  .status-dot.crit {
    background: var(--danger);
    box-shadow: 0 0 6px var(--danger);
  }
  .kpi-grid {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 1rem;
    margin-bottom: 1rem;
  }
  .spark-grid {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 1rem;
    margin-bottom: 1rem;
  }
  @media (max-width: 980px) {
    .kpi-grid { grid-template-columns: repeat(2, 1fr); }
    .spark-grid { grid-template-columns: 1fr; }
  }
  @media (max-width: 560px) {
    .kpi-grid { grid-template-columns: 1fr; }
  }
  .kpi-card {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.9rem 1rem;
    border-top: 3px solid var(--accent);
  }
  .kpi-card.blue { border-top-color: var(--seat-a); }
  .kpi-card.amber { border-top-color: var(--amber); }
  .kpi-card.frost { border-top-color: var(--frost); }
  .kpi-title {
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text);
    margin-bottom: 0.25rem;
  }
  .kpi-val {
    font-size: 1.65rem;
    font-weight: 800;
    color: var(--text-bright);
    line-height: 1.15;
  }
  .kpi-sub {
    font-size: 0.76rem;
    color: var(--text);
    margin-top: 0.35rem;
  }
  .gauge-row {
    margin-bottom: 0.8rem;
  }
  .gauge-head {
    display: flex;
    justify-content: space-between;
    font-size: 0.82rem;
    margin-bottom: 0.25rem;
  }
  .gauge-bar-bg {
    width: 100%;
    height: 9px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 5px;
    overflow: hidden;
  }
  .gauge-bar-fill {
    height: 100%;
    background: linear-gradient(90deg, #629924, #84c832);
    width: 0%;
    transition: width 0.25s ease;
  }
  .gauge-bar-fill.warn {
    background: linear-gradient(90deg, #d59120, #f0ad4e);
  }
  .gauge-bar-fill.crit {
    background: linear-gradient(90deg, #cc3333, #ff5555);
  }
  .spark-box {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.4rem;
    height: 112px;
    position: relative;
  }
  .spark-svg {
    width: 100%;
    height: 100%;
    display: block;
  }
  .stat-kv-grid {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 0.55rem;
    margin-bottom: 0.75rem;
  }
  .stat-kv {
    background: var(--panel-alt);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.45rem 0.65rem;
  }
  .stat-kv .k {
    font-size: 0.72rem;
    color: var(--text);
  }
  .stat-kv .v {
    font-size: 1.05rem;
    font-weight: 700;
    color: var(--text-bright);
  }
  .trace-kind {
    display: inline-block;
    padding: 0.1rem 0.42rem;
    border-radius: 3px;
    font-size: 0.72rem;
    font-weight: 700;
  }
  .trace-kind.GAME_CREATE { background: #1e3a56; color: #8cd0ff; }
  .trace-kind.SEAT_JOIN { background: #264a36; color: #98f5be; }
  .trace-kind.ROUND_SEAL { background: #3b2e58; color: #d6bfff; }
  .trace-kind.TURN_RESOLVE { background: #4a3818; color: #ffd280; }
  .trace-kind.GAME_FINISH { background: #5a1e1e; color: #ff9e9e; }
  .trace-kind.MATCH_PAIRED { background: #1f4c4c; color: #8ff5f5; }
</style>
</head>
<body>
<header>
  <a href="/" class="brand">
    AUTOMATAFL
    <span class="sub">Lean 4 Verified Simultaneous-Move Platform</span>
  </a>
  <div class="nav-right">
    <a href="/status" id="nav-status-pill" class="nav-pill mono" title="Live System Load, Network RTT Ping &amp; Lean 4 FFI Lag (Click for Dashboard)">
      <span id="nav-status-dot" class="status-dot"></span>
      <span id="nav-ping-txt">PING --ms</span>
      <span style="color:#555;">|</span>
      <span id="nav-srv-txt">SRV --µs</span>
      <span style="color:#555;">|</span>
      <span id="nav-ffi-txt">FFI --µs</span>
      <span style="color:#555;">|</span>
      <span id="nav-ram-txt">RAM --MiB</span>
    </a>
    <a href="/status" class="btn secondary">Status</a>
    <a href="/" class="btn secondary">Lobby</a>
  </div>
</header>

<div class="container">
  <div id="lobby-view" style="display:none;">
    <div class="lobby-grid">
      <div>
        <div class="card">
          <h2>Quick Play vs Simultaneous-Move AI Bot</h2>
          <div class="form-row">
            <div>
              <label>AI Bot Engine</label>
              <select id="bot-strategy">
                <option value="decoupled_uct" selected>Decoupled UCT (Simultaneous MCTS)</option>
                <option value="regret_matching">Regret Matching (Game-Theoretic Mixed Equilibrium)</option>
                <option value="minimax">Simultaneous Minimax (Maximin Security)</option>
                <option value="tactical">Tactical 1-Ply Lookahead</option>
                <option value="novice">Novice (Randomized Legal)</option>
              </select>
            </div>
            <div>
              <label>Your Seat</label>
              <select id="bot-human-seat">
                <option value="A" selected>Seat A (North Goals: (0,0) &amp; (N-1,0))</option>
                <option value="B">Seat B (South Goals: (0,N-1) &amp; (N-1,N-1))</option>
              </select>
            </div>
          </div>
          <div class="form-row">
            <div style="grid-column: 1 / -1;">
              <label>Execution Mode (Client Offloading)</label>
              <select id="bot-host-mode">
                <option value="client_local" selected>Client-Side Local AI (Zero Server Load — Runs in Browser Web Worker)</option>
                <option value="server_bot">Server-Hosted Bot Table (Shareable / Spectatable URL)</option>
              </select>
            </div>
          </div>
          <button onclick="startBotGame()">Play vs AI Bot Now</button>
        </div>

        <div class="card">
          <h2>Matchmaking Queue (Fog-of-War Live Pairing)</h2>
          <p style="font-size:0.86rem;margin-top:0;">
            Enter the queue with the selected board preset and variant settings below. You will be paired automatically as soon as a compatible opponent joins.
          </p>
          <div style="display:flex;gap:0.6rem;align-items:center;">
            <input type="text" id="mm-name" placeholder="Your handle (optional)" style="max-width:220px;">
            <button id="mm-btn" onclick="toggleMatchmaking()">Join Matchmaking Queue</button>
          </div>
          <div id="mm-status" style="margin-top:0.6rem;font-size:0.86rem;color:#98f5be;"></div>
        </div>

        <div class="card">
          <h2>Variant &amp; Rule Configurator (R2 Generalizations)</h2>
          <div class="form-row">
            <div>
              <label>Board Preset</label>
              <select id="cfg-preset">
                <option value="classic11x11" selected>11×11 Classic</option>
                <option value="tactical9x9">9×9 Tactical</option>
                <option value="blitz7x7">7×7 Blitz</option>
                <option value="mini5x5">5×5 Mini Skirmish</option>
              </select>
            </div>
            <div>
              <label>Tie-Break Mode</label>
              <select id="cfg-tiebreak">
                <option value="column" selected>Column Priority (Canonical)</option>
                <option value="row">Row Priority</option>
                <option value="freeze">Freeze on Tie (Automaton Stays Put)</option>
                <option value="momentum_same">Momentum (Prefer Same Axis)</option>
                <option value="momentum_orth">Momentum (Prefer Orthogonal Axis)</option>
              </select>
            </div>
          </div>
          <div class="form-row">
            <div>
              <label>Confluence / Collision Mode</label>
              <select id="cfg-confluence">
                <option value="conflict" selected>Conflict (Canonical Mark × &amp; Re-Enter)</option>
                <option value="bunch_before">Bunch-Before (Stop 1 Cell Short, No Re-Entry)</option>
                <option value="annihilate">Annihilate (Colliding Movers Destroy Each Other)</option>
              </select>
            </div>
            <div>
              <label>Commit-Reveal Protocol</label>
              <select id="cfg-commit">
                <option value="auto_reveal_on_both_sealed" selected>Auto-Reveal When Both Seats Seal</option>
                <option value="explicit_commit_reveal">Explicit Commit &amp; Reveal Phases</option>
              </select>
            </div>
          </div>
          <div class="form-row">
            <div>
              <label>Piece Cooldown Turns (❄️)</label>
              <input type="number" id="cfg-cooldown" value="0" min="0" max="5">
            </div>
            <div>
              <label>Automata Count (@)</label>
              <select id="cfg-automata">
                <option value="1" selected>1 Automaton (Canonical)</option>
                <option value="2">2 Automata (Dual Pulse)</option>
              </select>
            </div>
          </div>
          <div class="form-row">
            <div style="grid-column: 1 / -1;">
              <label>Open Table Handle</label>
              <input type="text" id="open-name" placeholder="Host name (optional)">
            </div>
          </div>
          <div class="checkbox-row">
            <label><input type="checkbox" id="cfg-swaps"> Allow 2-Cycle Swaps</label>
            <label><input type="checkbox" id="cfg-railgun"> Railgun Fling (Automaton Slides to Obstacle)</label>
          </div>
          <button onclick="createOpenTable()">Create Open Human Table</button>
        </div>
      </div>

      <div>
        <div class="card">
          <div style="display:flex;justify-content:space-between;align-items:center;">
            <h2 style="border:none;margin:0;padding:0;">Active Tables &amp; Live Games</h2>
            <button class="secondary" onclick="loadLobby()">Refresh</button>
          </div>
          <hr style="border:none;border-top:1px solid var(--border);margin:0.6rem 0;">
          <table class="table-list">
            <thead>
              <tr>
                <th>Game</th>
                <th>Variant</th>
                <th>Phase / Turn</th>
                <th>Seats</th>
                <th>Action</th>
              </tr>
            </thead>
            <tbody id="lobby-tables">
              <tr><td colspan="5">Loading tables...</td></tr>
            </tbody>
          </table>
        </div>

        <div class="card">
          <h3>Canonical Simultaneous-Move Rules &amp; Fog of War</h3>
          <ul style="font-size:0.85rem;padding-left:1.2rem;margin:0;">
            <li><strong>Simultaneous Sealed Moves:</strong> Both players secretly choose a rook-line move for any <strong>Repulsor (R)</strong> or <strong>Attractor (A)</strong>. Moves remain hidden under fog of war until both seats have sealed.</li>
            <li><strong>Zero-Round-Trip Client Selection:</strong> Piece selection, legal/executable target raycasts, sightlines, and single-move forecasts run locally in your browser in 0ms.</li>
            <li><strong>Conflict Re-Entry (×):</strong> In canonical <em>Conflict</em> mode, if both seats pick the same source (fork), move onto an unmoving piece (collide), or target the same destination (merge), those squares are marked <strong>×</strong> for the rest of the turn, non-clashing moves lock in, and clashing seats resubmit!</li>
            <li><strong>Automaton Pulse (@):</strong> Once moves execute, the Automaton scans N/E/S/W rook lines: Repulsors push away, Attractors pull toward. Stepping onto a goal corner wins!</li>
          </ul>
        </div>
      </div>
    </div>
  </div>

  <div id="game-view" style="display:none;">
    <div class="game-grid">
      <div class="board-wrap">
        <div id="status-banner" class="status-banner">Loading game state...</div>
        <div id="board-grid" class="board-grid"></div>
        <div style="display:flex;justify-content:space-between;align-items:center;margin-top:0.65rem;font-size:0.8rem;flex-wrap:wrap;gap:0.4rem;">
          <div>
            <span class="badge" style="color:#ff9995;">● R = Repulsor (Push)</span>
            <span class="badge" style="color:#99ccff;">● A = Attractor (Pull)</span>
            <span class="badge" style="color:#ffe082;">■ @ = Automaton</span>
            <span class="badge clash">× = Marked Conflict</span>
            <span class="badge frost">❄️ = Cooldown</span>
          </div>
          <div id="viewer-badge" class="mono"></div>
        </div>
      </div>

      <div>
        <div class="card">
          <h3>Fog-of-War Round Status</h3>
          <div id="seat-0-card" class="seat-card seat-0"></div>
          <div id="seat-1-card" class="seat-card seat-1"></div>

          <div id="action-controls" style="display:flex;gap:0.5rem;flex-wrap:wrap;margin-top:0.75rem;">
            <button id="btn-reveal" onclick="triggerReveal()" style="display:none;">Reveal Move</button>
            <button id="btn-resolve" onclick="triggerResolve()" style="display:none;">Resolve Round</button>
            <button id="btn-clear-sel" class="secondary" onclick="clearSelection()" style="display:none;">Clear Selection</button>
            <button id="btn-local-ai-move" class="secondary" onclick="triggerLocalAiAssist()" style="display:none;" title="Compute and seal a move locally in your browser">Local AI Move</button>
            <button id="btn-join-seat" onclick="joinOpenSeat()" style="display:none;">Join Open Seat</button>
            <button id="btn-rematch" onclick="rematchGame()" style="display:none;">Rematch</button>
            <button id="btn-resign" class="danger" onclick="resignGame()" style="display:none;">Resign</button>
          </div>
          <div id="own-forecast" style="margin-top:0.6rem;font-size:0.85rem;"></div>
        </div>

        <div class="card">
          <h3>Automaton Sense &amp; Goal Distance</h3>
          <div id="sense-panel" class="mono" style="font-size:0.84rem;"></div>
        </div>

        <div class="card">
          <h3>Turn History &amp; Conflict Log</h3>
          <div id="history-log" class="history-log mono">No completed turns yet.</div>
        </div>

        <div class="card">
          <h3>Variant Configuration &amp; Share Links</h3>
          <div id="variant-badges" style="margin-bottom:0.6rem;"></div>
          <div id="share-links" style="font-size:0.82rem;"></div>
        </div>
      </div>
    </div>
  </div>

  <div id="status-view" style="display:none;">
    <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:0.85rem;flex-wrap:wrap;gap:0.6rem;">
      <div>
        <h2 style="margin:0;color:var(--text-bright);font-size:1.25rem;">System Load, Network Lag &amp; Lean 4 FFI Telemetry</h2>
        <div style="font-size:0.82rem;color:var(--text);">
          Live 2s polling • Strict cgroup v2 &amp; ext4 loopback containment • Fog-of-war move coordinates strictly redacted
        </div>
      </div>
      <div style="display:flex;gap:0.5rem;align-items:center;flex-wrap:wrap;">
        <button class="secondary" onclick="pollServerStatus()">Refresh Now</button>
        <a href="/api/status" target="_blank" class="btn secondary mono">/api/status (JSON)</a>
        <a href="/metrics" target="_blank" class="btn secondary mono">/metrics (Prometheus)</a>
      </div>
    </div>

    <div class="kpi-grid">
      <div class="kpi-card">
        <div class="kpi-title">Client Network Ping (RTT)</div>
        <div id="st-kpi-ping" class="kpi-val mono">-- ms</div>
        <div id="st-kpi-ping-sub" class="kpi-sub mono">min --ms • avg --ms • jitter --ms</div>
      </div>
      <div class="kpi-card blue">
        <div class="kpi-title">Server HTTP Processing Lag</div>
        <div id="st-kpi-http" class="kpi-val mono">-- µs</div>
        <div id="st-kpi-http-sub" class="kpi-sub mono">p50 --µs • p95 --µs • p99 --µs</div>
      </div>
      <div class="kpi-card amber">
        <div class="kpi-title">Lean 4 Verified C FFI Oracle</div>
        <div id="st-kpi-ffi" class="kpi-val mono">-- µs</div>
        <div id="st-kpi-ffi-sub" class="kpi-sub mono">p95 --µs • max --µs • 0 errors</div>
      </div>
      <div class="kpi-card frost">
        <div class="kpi-title">Uptime &amp; Active Streams</div>
        <div id="st-kpi-uptime" class="kpi-val mono">--</div>
        <div id="st-kpi-uptime-sub" class="kpi-sub mono">SSE: 0 • Active Games: 0</div>
      </div>
    </div>

    <div class="spark-grid">
      <div class="card" style="margin-bottom:0;">
        <div style="display:flex;justify-content:space-between;align-items:baseline;margin-bottom:0.4rem;">
          <h3 style="border:none;padding:0;margin:0;font-size:0.92rem;">Client Network Ping (ms) — Last 60 Samples</h3>
          <span id="spark-ping-label" class="mono" style="font-size:0.78rem;color:#84c832;">-- ms</span>
        </div>
        <div class="spark-box">
          <svg id="spark-ping-svg" class="spark-svg" viewBox="0 0 360 96" preserveAspectRatio="none"></svg>
        </div>
      </div>

      <div class="card" style="margin-bottom:0;">
        <div style="display:flex;justify-content:space-between;align-items:baseline;margin-bottom:0.4rem;">
          <h3 style="border:none;padding:0;margin:0;font-size:0.92rem;">Server HTTP p95 &amp; Lean 4 FFI Avg Latency (µs) — 60s Window</h3>
          <span id="spark-lat-label" class="mono" style="font-size:0.78rem;color:#6cb8ff;">HTTP --µs / FFI --µs</span>
        </div>
        <div class="spark-box">
          <svg id="spark-lat-svg" class="spark-svg" viewBox="0 0 360 96" preserveAspectRatio="none"></svg>
        </div>
      </div>

      <div class="card" style="margin-bottom:0;">
        <div style="display:flex;justify-content:space-between;align-items:baseline;margin-bottom:0.4rem;">
          <h3 style="border:none;padding:0;margin:0;font-size:0.92rem;">Cgroup Memory RSS (MiB) vs 256 MiB Hard Cap — 60s Window</h3>
          <span id="spark-mem-label" class="mono" style="font-size:0.78rem;color:#f0ad4e;">-- MiB</span>
        </div>
        <div class="spark-box">
          <svg id="spark-mem-svg" class="spark-svg" viewBox="0 0 360 96" preserveAspectRatio="none"></svg>
        </div>
      </div>

      <div class="card" style="margin-bottom:0;">
        <div style="display:flex;justify-content:space-between;align-items:baseline;margin-bottom:0.4rem;">
          <h3 style="border:none;padding:0;margin:0;font-size:0.92rem;">HTTP Request Rate (req/s) &amp; Active SSE Streams — 60s Window</h3>
          <span id="spark-rps-label" class="mono" style="font-size:0.78rem;color:#98f5be;">-- req/s</span>
        </div>
        <div class="spark-box">
          <svg id="spark-rps-svg" class="spark-svg" viewBox="0 0 360 96" preserveAspectRatio="none"></svg>
        </div>
      </div>
    </div>

    <div class="lobby-grid">
      <div class="card">
        <h3>Resource Containment Gauges (<span id="st-cgroup-path" class="mono" style="font-size:0.82rem;color:#8cd0ff;">automatafl.slice</span>)</h3>

        <div class="gauge-row">
          <div class="gauge-head">
            <span><strong>Memory RSS</strong> (High: 192 MiB / Hard Cap: 256 MiB, Swap: 0)</span>
            <span id="gauge-mem-txt" class="mono">-- / 256.0 MiB (--%)</span>
          </div>
          <div class="gauge-bar-bg"><div id="gauge-mem-bar" class="gauge-bar-fill"></div></div>
          <div id="gauge-mem-sub" class="mono" style="font-size:0.74rem;color:var(--text);margin-top:0.2rem;">Peak RSS: -- MiB</div>
        </div>

        <div class="gauge-row">
          <div class="gauge-head">
            <span><strong>CPU Quota Utilization</strong> (Cap: 100% of 1 vCPU)</span>
            <span id="gauge-cpu-txt" class="mono">--% / 100%</span>
          </div>
          <div class="gauge-bar-bg"><div id="gauge-cpu-bar" class="gauge-bar-fill"></div></div>
          <div id="gauge-cpu-sub" class="mono" style="font-size:0.74rem;color:var(--text);margin-top:0.2rem;">Total CPU: --s • Host LoadAvg: -- / -- / --</div>
        </div>

        <div class="gauge-row">
          <div class="gauge-head">
            <span><strong>Dedicated Loopback Disk</strong> (<span class="mono">/var/lib/automatafl</span> 64 MiB ext4)</span>
            <span id="gauge-disk-txt" class="mono">-- / 55.0 MiB (--%)</span>
          </div>
          <div class="gauge-bar-bg"><div id="gauge-disk-bar" class="gauge-bar-fill"></div></div>
          <div id="gauge-disk-sub" class="mono" style="font-size:0.74rem;color:var(--text);margin-top:0.2rem;">Persisted Game Files: -- / 2048 (Auto-Pruned)</div>
        </div>

        <div class="gauge-row">
          <div class="gauge-head">
            <span><strong>In-Memory Session LRU Cache</strong> (Cap: 256 Active Games)</span>
            <span id="gauge-lru-txt" class="mono">-- / 256 (--%)</span>
          </div>
          <div class="gauge-bar-bg"><div id="gauge-lru-bar" class="gauge-bar-fill"></div></div>
          <div id="gauge-lru-sub" class="mono" style="font-size:0.74rem;color:var(--text);margin-top:0.2rem;">Saves: 0 • Loads: 0 • LRU Evictions: 0 • Disk Prunes: 0</div>
        </div>

        <div class="gauge-row" style="margin-bottom:0;">
          <div class="gauge-head">
            <span><strong>Open File Descriptors &amp; OS Tasks</strong> (NOFILE: 2048, TasksMax: 128)</span>
            <span id="gauge-fd-txt" class="mono">FDs: -- / 2048 • Tasks: -- / 128</span>
          </div>
          <div class="gauge-bar-bg"><div id="gauge-fd-bar" class="gauge-bar-fill"></div></div>
        </div>
      </div>

      <div class="card">
        <h3>Simultaneous-Move Lifecycle, AI Bot Engine &amp; Endpoint Breakdown</h3>
        <div class="stat-kv-grid">
          <div class="stat-kv"><div class="k">Games Created / Done</div><div id="st-games-cnt" class="v mono">0 / 0</div></div>
          <div class="stat-kv"><div class="k">Turns / Rounds Resolved</div><div id="st-turns-cnt" class="v mono">0 / 0</div></div>
          <div class="stat-kv"><div class="k">Conflict Clashes (×)</div><div id="st-clash-cnt" class="v mono" style="color:#ff9e9e;">0</div></div>
          <div class="stat-kv"><div class="k">Seat A / B Wins / Draw</div><div id="st-wins-cnt" class="v mono">0 / 0 / 0</div></div>
          <div class="stat-kv"><div class="k">Matchmaking Queue / Paired</div><div id="st-mm-cnt" class="v mono">0 / 0</div></div>
          <div class="stat-kv"><div class="k">SSE Connections / Events</div><div id="st-sse-cnt" class="v mono">0 / 0</div></div>
        </div>

        <h3 style="font-size:0.9rem;margin-top:0.85rem;">Server-Hosted AI Bot Search Telemetry (Client-Local AI = 0 Server Load)</h3>
        <div id="st-bot-summary" class="mono" style="font-size:0.8rem;margin-bottom:0.65rem;background:var(--bg);padding:0.55rem;border-radius:4px;border:1px solid var(--border);">
          Loading AI telemetry...
        </div>

        <h3 style="font-size:0.9rem;margin-top:0.85rem;">HTTP Endpoint Hit Distribution</h3>
        <div id="st-ep-grid" class="mono" style="font-size:0.78rem;display:grid;grid-template-columns:repeat(3,1fr);gap:0.35rem;">
        </div>
      </div>
    </div>

    <div class="card">
      <div style="display:flex;justify-content:space-between;align-items:center;flex-wrap:wrap;gap:0.5rem;">
        <h3 style="border:none;margin:0;padding:0;">Live Structured Event Trace (<span class="mono">journald</span> + Ring Buffer)</h3>
        <span style="font-size:0.76rem;color:#98f5be;" class="mono">🔒 Fog-of-War Sealed Coordinates Never Logged Before Turn Resolution</span>
      </div>
      <hr style="border:none;border-top:1px solid var(--border);margin:0.55rem 0;">
      <div style="max-height:320px;overflow-y:auto;">
        <table class="table-list mono" style="font-size:0.79rem;">
          <thead>
            <tr>
              <th>Seq</th>
              <th>Time (UTC)</th>
              <th>Kind</th>
              <th>Game</th>
              <th>Latency</th>
              <th>Sanitized Event Summary</th>
            </tr>
          </thead>
          <tbody id="st-trace-body">
            <tr><td colspan="6">Waiting for trace events...</td></tr>
          </tbody>
        </table>
      </div>
    </div>
  </div>
</div>

<script>
// ============================================================================
// Client-Side Verified Rules & Simultaneous-Move Engine (`ClientEngine`)
// Runs 100% in the browser for 0ms piece selection, target/sightline raycasts,
// single-move forecasts, and zero-server-load local AI games.
// Matches `oracle.rs` and `lean/Automatafl/Rules.lean` + `Variants.lean`.
// ============================================================================
const VAC = 0, REP = 1, ATT = 2, AUTO = 3;
const DIRS = [[0, -1], [1, 0], [0, 1], [-1, 0]]; // N, E, S, W

function coordEq(a, b) {
  return !!(a && b && a[0] === b[0] && a[1] === b[1]);
}
function coordIn(c, list) {
  return !!(c && list && list.some(x => x[0] === c[0] && x[1] === c[1]));
}
function inBounds(b, c) {
  return !!(c && c[0] >= 0 && c[1] >= 0 && c[0] < b.n && c[1] < b.n);
}
function cellAt(b, x, y) {
  if (x < 0 || y < 0 || x >= b.n || y >= b.n) return VAC;
  return b.cells[y * b.n + x];
}
function carAt(b, c) {
  return cellAt(b, c[0], c[1]) !== VAC;
}
function setCell(b, x, y, v) {
  if (x >= 0 && y >= 0 && x < b.n && y < b.n) {
    b.cells[y * b.n + x] = v;
  }
}
function cloneBoard(b) {
  return {
    n: b.n,
    cells: b.cells.slice(),
    auto: [b.auto[0], b.auto[1]],
    col_rule: b.col_rule,
    last_step_dir: b.last_step_dir ? [b.last_step_dir[0], b.last_step_dir[1]] : null,
    cooldowns: (b.cooldowns || []).map(cd => [[cd[0][0], cd[0][1]], cd[1]])
  };
}
function dedupCoords(cs) {
  const out = [];
  for (let i = 0; i < cs.length; i++) {
    if (!coordIn(cs[i], cs.slice(i + 1))) out.push([cs[i][0], cs[i][1]]);
  }
  return out;
}

function isOnCooldownClient(b, c, extraCooldownSquares) {
  if (extraCooldownSquares && coordIn(c, extraCooldownSquares)) return true;
  if (b.cooldowns && b.cooldowns.some(cd => cd[1] > 0 && coordEq(cd[0], c))) return true;
  return false;
}

function isSelectableClient(b, marks, cooldownSquares, c) {
  if (!inBounds(b, c)) return false;
  const p = cellAt(b, c[0], c[1]);
  if (p !== REP && p !== ATT) return false;
  if (coordEq(b.auto, c)) return false;
  if (coordIn(c, marks)) return false;
  if (cooldownSquares !== null && isOnCooldownClient(b, c, cooldownSquares)) return false;
  return true;
}

// Matches `oracle.rs::move_legal_b` (plus cooldown check on `frm` when cooldownSquares provided)
function moveLegalClient(b, marks, cooldownSquares, frm, to) {
  if (!inBounds(b, frm) || !inBounds(b, to)) return false;
  if (coordEq(frm, to)) return false;
  if (frm[0] !== to[0] && frm[1] !== to[1]) return false;
  if (coordEq(frm, b.auto)) return false;
  if (coordIn(frm, marks) || coordIn(to, marks)) return false;
  if (cooldownSquares && isOnCooldownClient(b, frm, cooldownSquares)) return false;
  return true;
}

function pathCells(frm, dst) {
  const dx = Math.sign(dst[0] - frm[0]);
  const dy = Math.sign(dst[1] - frm[1]);
  const steps = Math.max(Math.abs(dst[0] - frm[0]), Math.abs(dst[1] - frm[1]));
  const out = [];
  for (let k = 1; k <= steps; k++) {
    out.push([frm[0] + dx * k, frm[1] + dy * k]);
  }
  return out;
}

function blockedB(b, ms, m) {
  return pathCells(m.frm, m.to).some(c =>
    cellAt(b, c[0], c[1]) !== VAC && !ms.some(m2 => coordEq(m2.frm, c))
  );
}

// Inclusive path blocking (`Automatafl.blocked`): checks interior cells AND destination `to`.
function moveBlockedClient(b, lockedMoves, frm, to) {
  const all = [{ frm, to }, ...(lockedMoves || []).map(m => m.frm ? m : m.mv)];
  return blockedB(b, all, { frm, to });
}

function computeTargetsClient(b, marks, cooldownSquares, lockedMoves, src) {
  const legal = [];
  const executable = [];
  if (!src || !isSelectableClient(b, marks, cooldownSquares, src)) {
    return { legal, executable };
  }
  for (let y = 0; y < b.n; y++) {
    for (let x = 0; x < b.n; x++) {
      const dst = [x, y];
      if (moveLegalClient(b, marks, cooldownSquares, src, dst)) {
        legal.push(dst);
        if (!moveBlockedClient(b, lockedMoves, src, dst)) {
          executable.push(dst);
        }
      }
    }
  }
  return { legal, executable };
}

function raycastDirClient(b, from, step) {
  for (let dist = 1; dist <= b.n + 1; dist++) {
    const c = [from[0] + step[0] * dist, from[1] + step[1] * dist];
    if (!inBounds(b, c)) {
      return { what: VAC, dist };
    }
    const p = cellAt(b, c[0], c[1]);
    if (p !== VAC) {
      return { what: p, dist };
    }
  }
  return { what: VAC, dist: b.n + 1 };
}

function computeSightlinesClient(b) {
  const out = [];
  const steps = [[-1, 0], [1, 0], [0, -1], [0, 1]];
  for (const step of steps) {
    const r = raycastDirClient(b, b.auto, step);
    for (let k = 1; k <= r.dist; k++) {
      const c = [b.auto[0] + step[0] * k, b.auto[1] + step[1] * k];
      if (inBounds(b, c) && !coordIn(c, out)) {
        out.push(c);
      }
    }
  }
  return out;
}

// Matches `oracle.rs::evaluate_axis`
function evaluateAxisClient(pos, neg) {
  if (pos.what === ATT && neg.what === REP) {
    return pos.dist > 1
      ? { variant: 3, pos: true, att_dist: pos.dist, rep_dist: neg.dist }
      : { variant: 0, pos: false, att_dist: 0, rep_dist: 0 };
  }
  if (pos.what === REP && neg.what === ATT) {
    return neg.dist > 1
      ? { variant: 3, pos: false, att_dist: neg.dist, rep_dist: pos.dist }
      : { variant: 0, pos: false, att_dist: 0, rep_dist: 0 };
  }
  if (pos.what === REP && neg.what === REP) {
    return pos.dist !== neg.dist
      ? { variant: 2, pos: pos.dist > neg.dist, att_dist: 0, rep_dist: Math.min(pos.dist, neg.dist) }
      : { variant: 0, pos: false, att_dist: 0, rep_dist: 0 };
  }
  if (pos.what === REP && neg.what === VAC) {
    return neg.dist > 1
      ? { variant: 2, pos: false, att_dist: 0, rep_dist: pos.dist }
      : { variant: 0, pos: false, att_dist: 0, rep_dist: 0 };
  }
  if (pos.what === VAC && neg.what === REP) {
    return pos.dist > 1
      ? { variant: 2, pos: true, att_dist: 0, rep_dist: neg.dist }
      : { variant: 0, pos: false, att_dist: 0, rep_dist: 0 };
  }
  if (pos.what === ATT && neg.what === ATT) {
    return (pos.dist !== neg.dist && Math.min(pos.dist, neg.dist) > 1)
      ? { variant: 1, pos: pos.dist < neg.dist, att_dist: Math.min(pos.dist, neg.dist), rep_dist: 0 }
      : { variant: 0, pos: false, att_dist: 0, rep_dist: 0 };
  }
  if (pos.what === ATT && neg.what === VAC) {
    return pos.dist > 1
      ? { variant: 1, pos: true, att_dist: pos.dist, rep_dist: 0 }
      : { variant: 0, pos: false, att_dist: 0, rep_dist: 0 };
  }
  if (pos.what === VAC && neg.what === ATT) {
    return neg.dist > 1
      ? { variant: 1, pos: false, att_dist: neg.dist, rep_dist: 0 }
      : { variant: 0, pos: false, att_dist: 0, rep_dist: 0 };
  }
  return { variant: 0, pos: false, att_dist: 0, rep_dist: 0 };
}

// Matches `oracle.rs::decision_cmp`
function decisionCmpClient(a, b) {
  if (a.variant !== b.variant) return a.variant - b.variant;
  if (a.variant === 3) {
    if (a.att_dist !== b.att_dist) return b.att_dist - a.att_dist;
    return b.rep_dist - a.rep_dist;
  }
  if (a.variant === 2) return b.rep_dist - a.rep_dist;
  if (a.variant === 1) return b.att_dist - a.att_dist;
  return 0;
}

function deltaOfClient(d, base) {
  if (d.variant === 0) return [0, 0];
  return d.pos ? [base[0], base[1]] : [-base[0], -base[1]];
}

// Matches `oracle.rs::choose_offset_cfg`
function chooseOffsetCfgClient(xDec, yDec, tieBreak, lastStep) {
  const cmp = decisionCmpClient(xDec, yDec);
  if (cmp > 0) return deltaOfClient(xDec, [1, 0]);
  if (cmp < 0) return deltaOfClient(yDec, [0, 1]);
  const mode = (tieBreak && tieBreak.mode) ? tieBreak.mode : 'column';
  if (mode === 'column') return deltaOfClient(yDec, [0, 1]);
  if (mode === 'row') return deltaOfClient(xDec, [1, 0]);
  if (mode === 'freeze') return [0, 0];
  if (mode === 'momentum') {
    const same = tieBreak.prefer_same_axis !== false;
    if (lastStep && lastStep[0] !== 0) {
      return same ? deltaOfClient(xDec, [1, 0]) : deltaOfClient(yDec, [0, 1]);
    }
    if (lastStep && lastStep[1] !== 0) {
      return same ? deltaOfClient(yDec, [0, 1]) : deltaOfClient(xDec, [1, 0]);
    }
    return deltaOfClient(yDec, [0, 1]);
  }
  return deltaOfClient(yDec, [0, 1]);
}

function stepToClient(b, target) {
  const nxt = cloneBoard(b);
  if (inBounds(nxt, nxt.auto)) setCell(nxt, nxt.auto[0], nxt.auto[1], VAC);
  if (inBounds(nxt, target)) setCell(nxt, target[0], target[1], AUTO);
  nxt.auto = [target[0], target[1]];
  return nxt;
}

function flingLoopClient(off, fuel, b) {
  if (fuel <= 0 || (off[0] === 0 && off[1] === 0)) return cloneBoard(b);
  const target = [b.auto[0] + off[0], b.auto[1] + off[1]];
  if (inBounds(b, target) && cellAt(b, target[0], target[1]) === VAC) {
    const nxt = stepToClient(b, target);
    return flingLoopClient(off, fuel - 1, nxt);
  }
  return cloneBoard(b);
}

function automatonStepVariantClient(cfg, b, lastStep) {
  const xp = raycastDirClient(b, b.auto, [1, 0]);
  const xn = raycastDirClient(b, b.auto, [-1, 0]);
  const yp = raycastDirClient(b, b.auto, [0, 1]);
  const yn = raycastDirClient(b, b.auto, [0, -1]);
  const xDec = evaluateAxisClient(xp, xn);
  const yDec = evaluateAxisClient(yp, yn);
  const tb = (cfg && cfg.tie_break) ? cfg.tie_break : { mode: b.col_rule === false ? 'row' : 'column' };
  const off = chooseOffsetCfgClient(xDec, yDec, tb, lastStep);
  if (cfg && cfg.railgun_fling) {
    return flingLoopClient(off, b.n, b);
  }
  const target = [b.auto[0] + off[0], b.auto[1] + off[1]];
  if ((off[0] !== 0 || off[1] !== 0) && inBounds(b, target) && cellAt(b, target[0], target[1]) === VAC) {
    return stepToClient(b, target);
  }
  return cloneBoard(b);
}

function decisionWhyClient(d, axisX) {
  const dir = axisX ? (d.pos ? 'right' : 'left') : (d.pos ? 'down' : 'up');
  const back = axisX ? (d.pos ? 'left' : 'right') : (d.pos ? 'up' : 'down');
  const sqWord = n => n === 1 ? '1 square' : `${n} squares`;
  if (d.variant === 3) {
    return `an attractor ${sqWord(d.att_dist)} to its ${dir} and a repulsor ${sqWord(d.rep_dist)} to its ${back} (both send it ${dir})`;
  }
  if (d.variant === 2) {
    return `shoved ${dir}, away from a repulsor ${sqWord(Math.max(1, d.rep_dist))} to its ${back}`;
  }
  if (d.variant === 1) {
    return `pulled ${dir}, toward an attractor ${sqWord(Math.max(1, d.att_dist))} to its ${dir}`;
  }
  return 'nothing pulls it on this axis';
}

// Matches `oracle.rs::sense_reading_with_config`
function senseAtOriginClient(b, origin, config, lastStep) {
  const xp = raycastDirClient(b, origin, [1, 0]);
  const xn = raycastDirClient(b, origin, [-1, 0]);
  const yp = raycastDirClient(b, origin, [0, 1]);
  const yn = raycastDirClient(b, origin, [0, -1]);
  const xDec = evaluateAxisClient(xp, xn);
  const yDec = evaluateAxisClient(yp, yn);
  const tb = (config && config.tie_break) ? config.tie_break : { mode: b.col_rule === false ? 'row' : 'column' };
  const effLast = lastStep || b.last_step_dir || null;
  const off = chooseOffsetCfgClient(xDec, yDec, tb, effLast);
  const firstTarget = [origin[0] + off[0], origin[1] + off[1]];
  const moves = (off[0] !== 0 || off[1] !== 0) && inBounds(b, firstTarget) && cellAt(b, firstTarget[0], firstTarget[1]) === VAC;
  if (!moves) {
    let why = 'the two axes pull equally hard, so it holds';
    const dw = off[0] > 0 ? 'right' : off[0] < 0 ? 'left' : off[1] > 0 ? 'down' : off[1] < 0 ? 'up' : 'nowhere';
    if (off[0] !== 0 || off[1] !== 0) {
      why = !inBounds(b, firstTarget)
        ? `it leans ${dw} but that is off the board, so it holds`
        : `it leans ${dw} but a piece is standing there, so it holds`;
    }
    return { rays: [xp, xn, yp, yn], xDec, yDec, offset: [0, 0], target: [origin[0], origin[1]], why };
  }
  const after = automatonStepVariantClient(config, b, effLast);
  const axisX = off[0] !== 0;
  const dec = axisX ? xDec : yDec;
  return { rays: [xp, xn, yp, yn], xDec, yDec, offset: off, target: [after.auto[0], after.auto[1]], why: decisionWhyClient(dec, axisX) };
}

function goalDistanceClient(auto, goals, seatIdx) {
  let best = null;
  for (const g of (goals || [])) {
    if (g[1] === seatIdx) {
      const d = Math.abs(auto[0] - g[0][0]) + Math.abs(auto[1] - g[0][1]);
      if (best === null || d < best) best = d;
    }
  }
  return best;
}

// ============================================================================
// Simultaneous Move Chain & Confluence Solver (matches `oracle.rs` 1-to-1)
// ============================================================================
function forkAt(ms, s) {
  return ms.some(m1 => ms.some(m2 =>
    coordEq(m1.frm, s) && coordEq(m2.frm, s) && !coordEq(m1.to, m2.to)
  ));
}
function collideAt(b, ms, d) {
  return ms.some(m1 => ms.some(m2 =>
    coordEq(m1.to, d) && coordEq(m2.to, d) && !coordEq(m1.frm, m2.frm)
    && carAt(b, m1.frm) && carAt(b, m2.frm)
  ));
}
function clashCoords(b, ms) {
  const cands = [...ms.map(m => m.frm), ...ms.map(m => m.to)];
  return dedupCoords(cands.filter(c => forkAt(ms, c) || collideAt(b, ms, c)));
}
function edgeOf(b, ms, c) {
  const dsts = ms.filter(m => coordEq(m.frm, c) && !blockedB(b, ms, m)).map(m => m.to);
  if (dsts.length === 0) return null;
  const first = dsts[0];
  return dsts.every(d => coordEq(d, first)) ? first : null;
}
function twoCyc(b, ms, c, twoCycleSwaps) {
  if (twoCycleSwaps) return false;
  const d = edgeOf(b, ms, c);
  if (!d) return false;
  const ed = edgeOf(b, ms, d);
  return ed !== null && coordEq(ed, c) && !coordEq(d, c);
}
function stopWalk(b, ms, fuel, c) {
  if (fuel <= 0) return null;
  const d = edgeOf(b, ms, c);
  if (!d) return c;
  if (carAt(b, d)) return d;
  return stopWalk(b, ms, fuel - 1, d);
}
function leaves(b, ms, sf, vf, c, twoCycleSwaps) {
  if (vf <= 0) return true;
  if (twoCyc(b, ms, c, twoCycleSwaps)) return false;
  const d = stopWalk(b, ms, sf, c);
  if (!d || coordEq(d, c)) return false;
  if (carAt(b, d)) return leaves(b, ms, sf, vf - 1, d, twoCycleSwaps);
  return true;
}
function landMap(b, ms, c, twoCycleSwaps) {
  const fuel = ms.length + 1;
  if (leaves(b, ms, fuel, fuel, c, twoCycleSwaps)) {
    return stopWalk(b, ms, fuel, c) || c;
  }
  return c;
}
function movers(b, ms, twoCycleSwaps) {
  const frms = dedupCoords(ms.map(m => m.frm));
  return frms.filter(c => carAt(b, c) && !coordEq(landMap(b, ms, c, twoCycleSwaps), c));
}
function landBad(b, ms, c, twoCycleSwaps) {
  const mvs = movers(b, ms, twoCycleSwaps);
  const lc = landMap(b, ms, c, twoCycleSwaps);
  const claimers = mvs.filter(c2 => coordEq(landMap(b, ms, c2, twoCycleSwaps), lc)).length;
  return claimers !== 1 || (carAt(b, lc) && coordEq(landMap(b, ms, lc, twoCycleSwaps), lc)) || !inBounds(b, lc);
}
function unresolved(b, ms, twoCycleSwaps) {
  const mvs = movers(b, ms, twoCycleSwaps);
  return dedupCoords(mvs.filter(c => landBad(b, ms, c, twoCycleSwaps)).map(c => landMap(b, ms, c, twoCycleSwaps)));
}
function traceChainVertices(b, ms, fuel, isStart, c) {
  if (fuel <= 0 || (!isStart && carAt(b, c))) return [c];
  const nxt = edgeOf(b, ms, c);
  if (!nxt) return [c];
  return [c, ...traceChainVertices(b, ms, fuel - 1, false, nxt)];
}
function reverseEdgeInterior(u, v) {
  const out = [];
  if (u[1] === v[1]) {
    if (u[0] < v[0]) {
      for (let k = 0; k < v[0] - u[0] - 1; k++) out.push([v[0] - 1 - k, u[1]]);
    } else if (v[0] < u[0]) {
      for (let k = 0; k < u[0] - v[0] - 1; k++) out.push([v[0] + 1 + k, u[1]]);
    }
  } else if (u[0] === v[0]) {
    if (u[1] < v[1]) {
      for (let k = 0; k < v[1] - u[1] - 1; k++) out.push([u[0], v[1] - 1 - k]);
    } else if (v[1] < u[1]) {
      for (let k = 0; k < u[1] - v[1] - 1; k++) out.push([u[0], v[1] + 1 + k]);
    }
  }
  return out;
}
function reverseChainBeforeEnd(verts) {
  if (verts.length <= 1) return [];
  const u = verts[0], v = verts[1];
  return [...reverseChainBeforeEnd(verts.slice(1)), ...reverseEdgeInterior(u, v), u];
}
function bunchCandidates(b, ms, s, d) {
  const verts = traceChainVertices(b, ms, ms.length + 1, true, s);
  const cands = reverseChainBeforeEnd(verts).filter(q => !coordEq(q, d) && inBounds(b, q));
  if (!coordIn(s, cands)) cands.push(s);
  return cands;
}
function initBunchCands(b, ms, mvrs, twoCycleSwaps, s) {
  const d = landMap(b, ms, s, twoCycleSwaps);
  const isContested = mvrs.filter(c => coordEq(landMap(b, ms, c, twoCycleSwaps), d)).length > 1;
  const isStayer = q => carAt(b, q) && !coordIn(q, mvrs);
  const fallbacks = bunchCandidates(b, ms, s, d);
  if (!isContested && inBounds(b, d) && !isStayer(d)) {
    return [d, ...fallbacks];
  }
  return fallbacks;
}
function currentBunchProp(cands, s) {
  const entry = cands.find(e => coordEq(e[0], s));
  return (entry && entry[1].length > 0) ? entry[1][0] : s;
}
function bunchPropBlocked(b, mvrs, cands, forbidden, s) {
  const d = currentBunchProp(cands, s);
  const isStayer = q => carAt(b, q) && !coordIn(q, mvrs);
  return !coordEq(d, s) && (
    !inBounds(b, d)
    || isStayer(d)
    || coordIn(d, forbidden)
    || mvrs.filter(c => coordEq(currentBunchProp(cands, c), d)).length > 1
  );
}
function assignBunchLandings(b, ms, mvrs, twoCycleSwaps) {
  const forbidden = mvrs
    .map(c => landMap(b, ms, c, twoCycleSwaps))
    .filter(d => mvrs.filter(c => coordEq(landMap(b, ms, c, twoCycleSwaps), d)).length > 1);
  const cands = mvrs.map(s => [s, initBunchCands(b, ms, mvrs, twoCycleSwaps, s)]);
  const fuel = (mvrs.length + 1) * (b.n + ms.length + 2);
  for (let step = 0; step < fuel; step++) {
    const blocked = mvrs.map(s => bunchPropBlocked(b, mvrs, cands, forbidden, s));
    if (!blocked.some(Boolean)) break;
    const newContested = mvrs
      .map(s => currentBunchProp(cands, s))
      .filter(d => mvrs.filter(c => coordEq(currentBunchProp(cands, c), d) && !coordEq(c, d)).length > 1);
    forbidden.push(...newContested);
    for (let idx = 0; idx < cands.length; idx++) {
      if (blocked[idx] && cands[idx][1].length > 0) cands[idx][1].shift();
    }
  }
  const rawPairs = mvrs.map(s => [s, currentBunchProp(cands, s)]);
  const landOf = c => {
    const p = rawPairs.find(e => coordEq(e[0], c));
    return p ? p[1] : c;
  };
  const resolvable = mvrs.every(c => {
    const lc = landOf(c);
    const unique = mvrs.filter(c2 => coordEq(landOf(c2), lc)).length === 1;
    const notStayer = !(carAt(b, lc) && !coordIn(lc, mvrs));
    return unique && notStayer && inBounds(b, lc);
  });
  return resolvable ? rawPairs : mvrs.map(s => [s, s]);
}
function writeBoard(b, mvrs, landFn) {
  const next = cloneBoard(b);
  for (let y = 0; y < b.n; y++) {
    for (let x = 0; x < b.n; x++) {
      const q = [x, y];
      const arrivals = mvrs.filter(c => coordEq(landFn(c), q));
      if (arrivals.length === 1) {
        next.cells[y * b.n + x] = cellAt(b, arrivals[0][0], arrivals[0][1]);
      } else if (coordIn(q, mvrs)) {
        next.cells[y * b.n + x] = VAC;
      }
    }
  }
  return next;
}

// Matches `oracle.rs::resolve_moves_cfg`
function resolveMidClient(b, marks, moves, config) {
  const conf = (config && config.confluence) ? config.confluence : 'conflict';
  const twoCycleSwaps = !!(config && config.two_cycle_swaps);
  const legal = (moves || []).filter(m => moveLegalClient(b, marks || [], null, m.frm, m.to));
  const mvrs = movers(b, legal, twoCycleSwaps);
  if (conf === 'conflict') {
    if (unresolved(b, legal, twoCycleSwaps).length > 0) return cloneBoard(b);
    return writeBoard(b, mvrs, c => landMap(b, legal, c, twoCycleSwaps));
  }
  if (conf === 'annihilate') {
    const isStayer = q => carAt(b, q) && !coordIn(q, mvrs);
    const survivors = mvrs.filter(c => {
      const d = landMap(b, legal, c, twoCycleSwaps);
      const claimers = mvrs.filter(c2 => coordEq(landMap(b, legal, c2, twoCycleSwaps), d)).length;
      return claimers === 1 && !isStayer(d) && inBounds(b, d);
    });
    const vacated = mvrs.filter(c => {
      const d = landMap(b, legal, c, twoCycleSwaps);
      return !isStayer(d) && inBounds(b, d);
    });
    const next = cloneBoard(b);
    for (let y = 0; y < b.n; y++) {
      for (let x = 0; x < b.n; x++) {
        const q = [x, y];
        const arrivals = survivors.filter(c => coordEq(landMap(b, legal, c, twoCycleSwaps), q));
        if (arrivals.length === 1) {
          next.cells[y * b.n + x] = cellAt(b, arrivals[0][0], arrivals[0][1]);
        } else if (coordIn(q, vacated)) {
          next.cells[y * b.n + x] = VAC;
        }
      }
    }
    return next;
  }
  // bunch_before
  const pairs = assignBunchLandings(b, legal, mvrs, twoCycleSwaps);
  return writeBoard(b, mvrs, c => {
    const p = pairs.find(e => coordEq(e[0], c));
    return p ? p[1] : c;
  });
}

function landedCoordsCfgClient(config, b, marks, moves) {
  const conf = (config && config.confluence) ? config.confluence : 'conflict';
  const twoCycleSwaps = !!(config && config.two_cycle_swaps);
  const legal = (moves || []).filter(m => moveLegalClient(b, marks || [], null, m.frm, m.to));
  const mvrs = movers(b, legal, twoCycleSwaps);
  if (conf === 'conflict') {
    if (unresolved(b, legal, twoCycleSwaps).length > 0) return [];
    return dedupCoords(mvrs.map(c => landMap(b, legal, c, twoCycleSwaps)));
  }
  if (conf === 'annihilate') {
    const isStayer = q => carAt(b, q) && !coordIn(q, mvrs);
    return dedupCoords(mvrs.flatMap(c => {
      const d = landMap(b, legal, c, twoCycleSwaps);
      const claimers = mvrs.filter(c2 => coordEq(landMap(b, legal, c2, twoCycleSwaps), d)).length;
      return (claimers === 1 && !isStayer(d) && inBounds(b, d)) ? [d] : [];
    }));
  }
  const pairs = assignBunchLandings(b, legal, mvrs, twoCycleSwaps);
  return dedupCoords(pairs.filter(e => !coordEq(e[0], e[1])).map(e => e[1]));
}

function winOnEntryClient(before, after, goals) {
  if (coordEq(after.auto, before.auto)) return null;
  const g = (goals || []).find(entry => coordEq(entry[0], after.auto));
  return g ? (g[1] === 0 ? 'A' : 'B') : null;
}

function stepDirDeltaClient(oldPos, newPos) {
  if (oldPos[1] === newPos[1]) {
    if (newPos[0] > oldPos[0]) return [1, 0];
    if (oldPos[0] > newPos[0]) return [-1, 0];
    return null;
  }
  if (oldPos[0] === newPos[0]) {
    if (newPos[1] > oldPos[1]) return [0, 1];
    if (oldPos[1] > newPos[1]) return [0, -1];
    return null;
  }
  return null;
}

// Matches `oracle.rs::step_automata_cfg`
function stepAutomataClient(b, config, goals, lastStep) {
  const k = Math.max(1, (config && config.automata_count) ? config.automata_count : 1);
  let curB = cloneBoard(b);
  let curD = lastStep || b.last_step_dir || null;
  const s0 = senseAtOriginClient(curB, curB.auto, config, curD);
  for (let i = 0; i < k; i++) {
    const nxtB = automatonStepVariantClient(config, curB, curD);
    const w = winOnEntryClient(curB, nxtB, goals);
    const dir = stepDirDeltaClient(curB.auto, nxtB.auto);
    if (dir) curD = dir;
    curB = nxtB;
    curB.last_step_dir = curD;
    if (w !== null) {
      return { board: curB, winner: w, sense: s0, nextDir: curD };
    }
  }
  return { board: curB, winner: null, sense: s0, nextDir: curD };
}

function clashCoordsCfgClient(config, b, ms) {
  const conf = (config && config.confluence) ? config.confluence : 'conflict';
  if (conf === 'conflict') return clashCoords(b, ms);
  const cands = [...ms.map(m => m.frm), ...ms.map(m => m.to)];
  return dedupCoords(cands.filter(c => forkAt(ms, c)));
}

function unresolvedCfgClient(config, b, ms) {
  const conf = (config && config.confluence) ? config.confluence : 'conflict';
  const twoCycleSwaps = !!(config && config.two_cycle_swaps);
  if (conf === 'conflict') return unresolved(b, ms, twoCycleSwaps);
  return [];
}

function lastAutoDeltaFromView(v, b) {
  if (b && b.last_step_dir && (b.last_step_dir[0] !== 0 || b.last_step_dir[1] !== 0)) {
    return [Math.sign(b.last_step_dir[0]), Math.sign(b.last_step_dir[1])];
  }
  const hist = (v && v.turn_history) ? v.turn_history : [];
  for (let i = hist.length - 1; i >= 0; i--) {
    const st = hist[i].auto_step;
    if (st && st.from && st.to) {
      const dx = st.to[0] - st.from[0];
      const dy = st.to[1] - st.from[1];
      if (dx !== 0 || dy !== 0) return [Math.sign(dx), Math.sign(dy)];
    }
  }
  return null;
}

// Matches `oracle.rs::own_move_forecast_with_config`
function ownMoveForecastClient(b, marks, lockedMoves, move, config, goals, seatIdx, lastStep) {
  if (!move) return null;
  const pid = (seatIdx != null) ? seatIdx : (move.who != null ? move.who : 0);
  const mvWho = (move.who != null) ? move.who : pid;
  const locked = (lockedMoves || []).map(lm => lm.frm ? lm : lm.mv);
  const fresh = moveLegalClient(b, marks || [], null, move.frm, move.to)
    ? [{ who: mvWho, frm: move.frm, to: move.to }]
    : [];
  const allMoves = [...locked, ...fresh];
  const clash = clashCoordsCfgClient(config, b, allMoves);
  const cs = clash.length > 0 ? clash : unresolvedCfgClient(config, b, allMoves);
  if (cs.length > 0) return null;
  const mid = resolveMidClient(b, marks || [], allMoves, config);
  const stepped = stepAutomataClient(mid, config, goals, lastStep);
  const autoAfter = stepped.board.auto;
  const mine = goalDistanceClient(b.auto, goals, pid);
  const then = goalDistanceClient(autoAfter, goals, pid);
  let closer = ', no nearer your corner';
  if (mine != null && then != null) {
    if (then < mine) closer = `, ${mine - then} NEARER your corner`;
    else if (then > mine) closer = `, ${then - mine} further from your corner`;
  }
  const mySeatChar = pid === 0 ? 'A' : 'B';
  if (stepped.winner === mySeatChar) {
    return '⚑ IF YOURS IS THE ONLY MOVE THAT LANDS, THIS WINS THE MATCH: the automaton steps into your own corner. The other seat is sealing right now and can take that away.';
  }
  if (stepped.winner && stepped.winner !== mySeatChar) {
    return `⚑ CAREFUL: if yours is the only move that lands, this hands the match to SEAT ${stepped.winner}; it walks the automaton into THEIR corner.`;
  }
  if (coordEq(autoAfter, b.auto)) {
    return 'Your move alone would leave the automaton where it is. (The other seat is moving too, so the round can still shift it.)';
  }
  return `Your move alone would send the automaton to (${autoAfter[0]},${autoAfter[1]})${closer}. The other seat is sealing at the same time, so the real answer can differ; that gap is the game.`;
}

// Stock board constructors for client-side local games
function buildStockBoardClient(config) {
  const colRule = !config || !config.tie_break || config.tie_break.mode === 'column';
  const preset = (config && config.preset) || 'classic11x11';
  let n = 11, auto = [5, 5], placed = [];
  if (preset === 'classic11x11') {
    n = 11; auto = [5, 5];
    for (const x of [0, 1, 4, 5, 6, 9, 10]) { placed.push([[x, 0], REP], [[x, 10], REP]); }
    for (const x of [3, 7]) { placed.push([[x, 1], ATT], [[x, 9], ATT]); }
    for (const x of [4, 5, 6]) { placed.push([[x, 1], REP], [[x, 9], REP]); }
    for (const x of [0, 1, 9, 10]) { placed.push([[x, 4], ATT], [[x, 5], REP], [[x, 6], ATT]); }
  } else if (preset === 'tactical9x9') {
    n = 9; auto = [4, 4];
    for (const x of [0, 1, 3, 4, 5, 7, 8]) { placed.push([[x, 0], REP], [[x, 8], REP]); }
    for (const x of [2, 6]) { placed.push([[x, 1], ATT], [[x, 7], ATT]); }
    for (const x of [3, 4, 5]) { placed.push([[x, 1], REP], [[x, 7], REP]); }
    for (const x of [0, 1, 7, 8]) { placed.push([[x, 3], ATT], [[x, 4], REP], [[x, 5], ATT]); }
  } else if (preset === 'blitz7x7') {
    n = 7; auto = [3, 3];
    for (const x of [0, 2, 3, 4, 6]) { placed.push([[x, 0], REP], [[x, 6], REP]); }
    for (const x of [1, 5]) { placed.push([[x, 1], ATT], [[x, 5], ATT]); }
    for (const x of [2, 4]) { placed.push([[x, 1], REP], [[x, 5], REP]); }
    for (const x of [0, 6]) { placed.push([[x, 2], ATT], [[x, 3], REP], [[x, 4], ATT]); }
  } else {
    n = 5; auto = [2, 2];
    placed.push([[0, 1], ATT], [[1, 0], REP], [[4, 3], ATT], [[3, 4], REP]);
  }
  const cells = new Array(n * n).fill(VAC);
  for (const [c, p] of placed) {
    cells[c[1] * n + c[0]] = p;
  }
  cells[auto[1] * n + auto[0]] = AUTO;
  return { n, cells, auto, col_rule: colRule, last_step_dir: null, cooldowns: [] };
}

function stockGoalsClient(n) {
  return [[[0, 0], 0], [[n - 1, 0], 0], [[0, n - 1], 1], [[n - 1, n - 1], 1]];
}

// ============================================================================
// Client-Side AI Bot Search (Web Worker + Fallback)
// Supports `novice`, `tactical`, `minimax`, `decoupled_uct`, `regret_matching`
// ============================================================================
function enumerateCandidateMovesClient(b, marks, cooldownSquares, lockedMoves) {
  const exec = [];
  const prop = [];
  for (let y = 0; y < b.n; y++) {
    for (let x = 0; x < b.n; x++) {
      const src = [x, y];
      if (!isSelectableClient(b, marks, cooldownSquares, src)) continue;
      for (const dir of DIRS) {
        for (let step = 1; step < b.n; step++) {
          const dst = [x + dir[0] * step, y + dir[1] * step];
          if (!moveLegalClient(b, marks, cooldownSquares, src, dst)) continue;
          const mv = { frm: src, to: dst };
          if (!moveBlockedClient(b, lockedMoves, src, dst)) {
            exec.push(mv);
          } else {
            prop.push(mv);
          }
        }
      }
    }
  }
  return exec.length > 0 ? exec : prop;
}

function evaluateBoardForSeatClient(b, goals, seatIdx) {
  const oppIdx = 1 - seatIdx;
  const myD = goalDistanceClient(b.auto, goals, seatIdx) ?? 10;
  const opD = goalDistanceClient(b.auto, goals, oppIdx) ?? 10;
  if (myD === 0) return 1000;
  if (opD === 0) return -1000;
  return (opD - myD) * 25;
}

function chooseBotMoveClientSync(strategy, b, goals, marks, cooldownSquares, lockedMoves, config, seat) {
  const seatIdx = seat === 'A' ? 0 : 1;
  const oppIdx = 1 - seatIdx;
  const cands = enumerateCandidateMovesClient(b, marks, cooldownSquares, lockedMoves);
  if (cands.length === 0) return null;
  if (strategy === 'novice') {
    const pick = cands[Math.floor(Math.random() * cands.length)];
    return { who: seatIdx, frm: pick.frm, to: pick.to };
  }

  // Prioritize candidates that land on or vacate automaton sightlines
  const auto = b.auto;
  const lockedList = (lockedMoves || []).map(l => l.frm ? l : l.mv);
  const scored = cands.map(mv => {
    const mWithWho = { who: seatIdx, frm: mv.frm, to: mv.to };
    const mid = resolveMidClient(b, marks, [...lockedList, mWithWho], config);
    const step = stepAutomataClient(mid, config, goals, b.last_step_dir);
    let val = evaluateBoardForSeatClient(step.board, goals, seatIdx);
    if (mv.to[0] === auto[0] || mv.to[1] === auto[1] || mv.frm[0] === auto[0] || mv.frm[1] === auto[1]) {
      val += 4;
    }
    return { mv: mWithWho, val };
  });
  scored.sort((a, b2) => b2.val - a.val);

  if (strategy === 'tactical' || scored.length === 1) {
    return scored[0].mv;
  }

  const topK = scored.slice(0, Math.min(12, scored.length)).map(s => s.mv);
  const oppScored = cands.map(mv => {
    const mWithWho = { who: oppIdx, frm: mv.frm, to: mv.to };
    const mid = resolveMidClient(b, marks, [...lockedList, mWithWho], config);
    const step = stepAutomataClient(mid, config, goals, b.last_step_dir);
    return { mv: mWithWho, val: evaluateBoardForSeatClient(step.board, goals, oppIdx) };
  });
  oppScored.sort((a, b2) => b2.val - a.val);
  const oppK = oppScored.slice(0, Math.min(10, oppScored.length)).map(s => s.mv);

  const payoff = (myMv, opMv) => {
    const mid = resolveMidClient(b, marks, [...lockedList, myMv, opMv], config);
    const step = stepAutomataClient(mid, config, goals, b.last_step_dir);
    return evaluateBoardForSeatClient(step.board, goals, seatIdx);
  };

  if (strategy === 'minimax') {
    let bestMv = topK[0], bestGuarantee = -1e9;
    for (const myMv of topK) {
      let worst = 1e9;
      for (const opMv of oppK) {
        const u = payoff(myMv, opMv);
        if (u < worst) worst = u;
      }
      if (worst > bestGuarantee) {
        bestGuarantee = worst;
        bestMv = myMv;
      }
    }
    return bestMv;
  }

  if (strategy === 'regret_matching') {
    const K = topK.length, M = oppK.length;
    const regMy = new Array(K).fill(0);
    const regOp = new Array(M).fill(0);
    const sumMy = new Array(K).fill(0);
    const matrix = topK.map(myMv => oppK.map(opMv => payoff(myMv, opMv)));
    let iIdx = 0, jIdx = 0;
    for (let it = 0; it < 64; it++) {
      const u = matrix[iIdx][jIdx];
      for (let k = 0; k < K; k++) regMy[k] = Math.max(0, regMy[k] + (matrix[k][jIdx] - u));
      for (let m = 0; m < M; m++) regOp[m] = Math.max(0, regOp[m] + (u - matrix[iIdx][m]));
      const totMy = regMy.reduce((a, c) => a + c, 0);
      for (let k = 0; k < K; k++) sumMy[k] += totMy > 0 ? regMy[k] / totMy : 1 / K;
      iIdx = regMy.indexOf(Math.max(...regMy));
      jIdx = regOp.indexOf(Math.max(...regOp));
    }
    let bestK = 0;
    for (let k = 1; k < K; k++) {
      if (sumMy[k] > sumMy[bestK]) bestK = k;
    }
    return topK[bestK];
  }

  // Default: `decoupled_uct` (Simultaneous Decoupled UCT MCTS)
  const K = topK.length, M = oppK.length;
  const visitsMy = new Array(K).fill(0), rewardMy = new Array(K).fill(0);
  const visitsOp = new Array(M).fill(0), rewardOp = new Array(M).fill(0);
  const sims = 96;
  for (let s = 1; s <= sims; s++) {
    const logS = Math.log(s + 1);
    let iPick = 0, bestUcbI = -1e9;
    for (let k = 0; k < K; k++) {
      const ucb = visitsMy[k] === 0 ? 1e6 - k : (rewardMy[k] / visitsMy[k]) + 40 * Math.sqrt(logS / visitsMy[k]);
      if (ucb > bestUcbI) { bestUcbI = ucb; iPick = k; }
    }
    let jPick = 0, bestUcbJ = -1e9;
    for (let m = 0; m < M; m++) {
      const ucb = visitsOp[m] === 0 ? 1e6 - m : (rewardOp[m] / visitsOp[m]) + 40 * Math.sqrt(logS / visitsOp[m]);
      if (ucb > bestUcbJ) { bestUcbJ = ucb; jPick = m; }
    }
    const u = payoff(topK[iPick], oppK[jPick]);
    visitsMy[iPick]++; rewardMy[iPick] += u;
    visitsOp[jPick]++; rewardOp[jPick] -= u;
  }
  let bestIdx = 0;
  for (let k = 1; k < K; k++) {
    if (visitsMy[k] > visitsMy[bestIdx]) bestIdx = k;
  }
  return topK[bestIdx];
}

// Web Worker wrapper so AI search never blocks the main UI thread
let botWorker = null;
function chooseBotMoveClientAsync(strategy, b, goals, marks, cooldownSquares, lockedMoves, config, seat) {
  return new Promise(resolve => {
    try {
      if (!botWorker && typeof Worker !== 'undefined' && typeof Blob !== 'undefined') {
        const workerSrc = `
          const VAC = 0, REP = 1, ATT = 2, AUTO = 3;
          const DIRS = [[0, -1], [1, 0], [0, 1], [-1, 0]];
          ${coordEq.toString()}
          ${coordIn.toString()}
          ${inBounds.toString()}
          ${cellAt.toString()}
          ${carAt.toString()}
          ${setCell.toString()}
          ${cloneBoard.toString()}
          ${dedupCoords.toString()}
          ${isOnCooldownClient.toString()}
          ${isSelectableClient.toString()}
          ${moveLegalClient.toString()}
          ${pathCells.toString()}
          ${blockedB.toString()}
          ${moveBlockedClient.toString()}
          ${raycastDirClient.toString()}
          ${evaluateAxisClient.toString()}
          ${decisionCmpClient.toString()}
          ${deltaOfClient.toString()}
          ${chooseOffsetCfgClient.toString()}
          ${stepToClient.toString()}
          ${flingLoopClient.toString()}
          ${automatonStepVariantClient.toString()}
          ${decisionWhyClient.toString()}
          ${senseAtOriginClient.toString()}
          ${goalDistanceClient.toString()}
          ${forkAt.toString()}
          ${collideAt.toString()}
          ${clashCoords.toString()}
          ${edgeOf.toString()}
          ${twoCyc.toString()}
          ${stopWalk.toString()}
          ${leaves.toString()}
          ${landMap.toString()}
          ${movers.toString()}
          ${landBad.toString()}
          ${unresolved.toString()}
          ${traceChainVertices.toString()}
          ${reverseEdgeInterior.toString()}
          ${reverseChainBeforeEnd.toString()}
          ${bunchCandidates.toString()}
          ${initBunchCands.toString()}
          ${currentBunchProp.toString()}
          ${bunchPropBlocked.toString()}
          ${assignBunchLandings.toString()}
          ${writeBoard.toString()}
          ${resolveMidClient.toString()}
          ${winOnEntryClient.toString()}
          ${stepDirDeltaClient.toString()}
          ${stepAutomataClient.toString()}
          ${enumerateCandidateMovesClient.toString()}
          ${evaluateBoardForSeatClient.toString()}
          ${chooseBotMoveClientSync.toString()}
          self.onmessage = function(e) {
            const d = e.data;
            const mv = chooseBotMoveClientSync(d.strategy, d.b, d.goals, d.marks, d.cooldownSquares, d.lockedMoves, d.config, d.seat);
            self.postMessage(mv);
          };
        `;
        botWorker = new Worker(URL.createObjectURL(new Blob([workerSrc], { type: 'application/javascript' })));
      }
      if (botWorker) {
        botWorker.onmessage = (ev) => resolve(ev.data);
        botWorker.onerror = () => resolve(chooseBotMoveClientSync(strategy, b, goals, marks, cooldownSquares, lockedMoves, config, seat));
        botWorker.postMessage({ strategy, b, goals, marks, cooldownSquares, lockedMoves, config, seat });
        return;
      }
    } catch (err) {}
    resolve(chooseBotMoveClientSync(strategy, b, goals, marks, cooldownSquares, lockedMoves, config, seat));
  });
}

// ============================================================================
// Client-Side Local Session Manager (Zero Server CPU / Zero Server RAM)
// Uses identical `TurnSummary` and `AutoStepRecord` schema to `session.rs`.
// ============================================================================
let localMatchState = null;

function createLocalBotMatch(config, strategy, humanSeat) {
  const board = buildStockBoardClient(config);
  const goals = stockGoalsClient(board.n);
  localMatchState = {
    id: 'local-ai',
    name: `Local Browser Match vs ${strategy}`,
    config,
    strategy,
    humanSeat,
    botSeat: humanSeat === 'A' ? 'B' : 'A',
    board,
    goals,
    marks: [],
    locked_moves: [],
    waiting_seats: [0, 1],
    conflict_rounds: 0,
    cooldown_squares: [],
    turn_no: 0,
    max_turns: config.max_turns || 256,
    phase: 'commit',
    ended: false,
    winner: null,
    finish_reason: null,
    turn_history: [],
    last_step: null,
    humanSealed: null,
    botSealed: null,
    status_headline: `Turn 1 — Select a piece and destination to seal your move (Client-Side Local AI: ${strategy})`
  };
  localSelectedCoord = null;
  renderGameView(buildLocalGameView());
}

function buildLocalGameView() {
  const st = localMatchState;
  const sense = senseAtOriginClient(st.board, st.board.auto, st.config, st.board.last_step_dir);
  const distA = goalDistanceClient(st.board.auto, st.goals, 0);
  const distB = goalDistanceClient(st.board.auto, st.goals, 1);
  const humanIdx = st.humanSeat === 'A' ? 0 : 1;

  const seats = [0, 1].map(idx => {
    const seatChar = idx === 0 ? 'A' : 'B';
    const isHuman = idx === humanIdx;
    const lockedMv = st.locked_moves.find(l => l.who === idx) || null;
    const isWaiting = !st.ended && st.waiting_seats.includes(idx);
    const sealedMv = isHuman ? st.humanSealed : st.botSealed;
    return {
      seat: seatChar,
      label: seatChar,
      player_name: isHuman ? 'You (Browser)' : `Local AI (${st.strategy})`,
      is_claimed: true,
      is_you: isHuman,
      is_bot: !isHuman,
      bot_strategy: !isHuman ? st.strategy : null,
      is_waiting: isWaiting && !sealedMv,
      is_locked: !isWaiting && st.marks.length > 0,
      has_committed: !!sealedMv,
      commitment_seal: sealedMv ? 'local-seal' : null,
      has_revealed: false,
      visible_move: isHuman ? (sealedMv || lockedMv) : lockedMv,
      own_selection: isHuman ? localSelectedCoord : null,
      legal_targets: [],
      executable_targets: [],
      own_move_forecast: null
    };
  });

  const sightSquares = computeSightlinesClient(st.board);
  return {
    id: st.id,
    name: st.name,
    is_public: false,
    config: st.config,
    phase: st.ended ? 'over' : st.phase,
    turn_no: st.turn_no,
    conflict_round_no: st.conflict_rounds,
    max_turns: st.max_turns,
    commits_count: (st.humanSealed ? 1 : 0) + (st.botSealed ? 1 : 0),
    reveals_count: 0,
    ended: st.ended,
    winner: st.winner,
    finish_reason: st.finish_reason,
    viewer_seat: st.humanSeat,
    board: st.board,
    goals: st.goals,
    marks: st.marks,
    cooldown_squares: st.cooldown_squares,
    locked_moves: st.locked_moves,
    waiting_seats: st.waiting_seats,
    sightlines: [],
    sightline_loop_squares: sightSquares,
    sense,
    standing: { dist: [distA, distB], verdict: distA < distB ? 0 : distB < distA ? 1 : null },
    automaton_next_target: sense.target,
    automaton_next_why: sense.why,
    last_clash: null,
    last_step: st.last_step,
    turns_played_count: st.turn_history.length,
    turn_history: st.turn_history,
    status_headline: st.status_headline,
    next_step_hint: st.ended ? 'Match finished. Click Rematch to play again!' : 'Click any R or A piece to highlight targets in 0ms.',
    seats
  };
}

async function submitLocalMove(from, to) {
  const st = localMatchState;
  if (!st || st.ended) return;
  const humanIdx = st.humanSeat === 'A' ? 0 : 1;
  const botIdx = 1 - humanIdx;
  const humanMv = { who: humanIdx, frm: from, to };
  st.humanSealed = humanMv;
  localSelectedCoord = null;
  st.status_headline = `Computing ${st.strategy} AI move in browser Web Worker...`;
  renderGameView(buildLocalGameView());

  let pendingHumanMv = st.waiting_seats.includes(humanIdx) ? humanMv : null;
  for (let safety = 0; safety < 16; safety++) {
    let botMv = null;
    if (st.waiting_seats.includes(botIdx)) {
      botMv = await chooseBotMoveClientAsync(
        st.strategy,
        st.board,
        st.goals,
        st.marks,
        st.cooldown_squares,
        st.locked_moves,
        st.config,
        st.botSeat
      );
      if (botMv) botMv.who = botIdx;
    }

    const fresh = [];
    if (pendingHumanMv && st.waiting_seats.includes(humanIdx)) fresh.push(pendingHumanMv);
    if (botMv && st.waiting_seats.includes(botIdx)) fresh.push(botMv);
    const allMoves = [...st.locked_moves, ...fresh];

    const clash = clashCoordsCfgClient(st.config, st.board, allMoves);
    const cs = clash.length > 0 ? clash : unresolvedCfgClient(st.config, st.board, allMoves);
    if (cs.length > 0) {
      st.conflict_rounds += 1;
      st.marks = dedupCoords([...st.marks, ...cs]);
      st.locked_moves = allMoves.filter(m => !coordIn(m.frm, cs) && !coordIn(m.to, cs));
      st.waiting_seats = [...new Set(allMoves.filter(m => coordIn(m.frm, cs) || coordIn(m.to, cs)).map(m => m.who))];
      st.humanSealed = null;
      st.botSealed = null;
      pendingHumanMv = null;
      st.phase = 'resubmit';
      const sqStr = cs.map(c => `(${c[0]},${c[1]})`).join(', ');
      if (!st.waiting_seats.includes(humanIdx) && st.waiting_seats.includes(botIdx)) {
        // Human move locked cleanly; only bot clashed and must re-submit immediately
        continue;
      }
      st.status_headline = `Clash on ${sqStr} marked ×! Resubmit a non-clashing move.`;
      renderGameView(buildLocalGameView());
      return;
    }

    // Clean round resolution
    const prevTurnNo = st.turn_no;
    const mid = resolveMidClient(st.board, st.marks, allMoves, st.config);
    const autoBefore = [mid.auto[0], mid.auto[1]];
    const stepped = stepAutomataClient(mid, st.config, st.goals, st.board.last_step_dir);
    const landed = landedCoordsCfgClient(st.config, st.board, st.marks, allMoves);
    st.board = stepped.board;
    st.turn_no += 1;

    // Update piece cooldowns matching `record_cooldowns` / `active_cooldown_squares`
    const cdTurns = st.config.cooldown_turns || 0;
    const ticked = (st.board.cooldowns || [])
      .filter(cd => cd[1] > 1 && !coordIn(cd[0], landed))
      .map(cd => [cd[0], cd[1] - 1]);
    if (cdTurns > 0) {
      for (const sq of landed) ticked.push([sq, cdTurns]);
    }
    st.board.cooldowns = ticked;
    st.cooldown_squares = ticked.map(c => c[0]);

    const stepRec = {
      turn_no: prevTurnNo,
      from: autoBefore,
      to: [st.board.auto[0], st.board.auto[1]],
      why: stepped.sense.why
    };
    const summary = {
      turn_no: prevTurnNo,
      moves: allMoves,
      conflict_rounds: st.conflict_rounds,
      landed_squares: landed,
      auto_step: stepRec,
      winner: stepped.winner
    };
    st.turn_history.push(summary);
    st.last_step = stepRec;
    st.marks = [];
    st.locked_moves = [];
    st.waiting_seats = [0, 1];
    st.conflict_rounds = 0;
    st.humanSealed = null;
    st.botSealed = null;
    st.phase = 'commit';

    if (stepped.winner) {
      st.ended = true;
      st.winner = stepped.winner;
      st.finish_reason = { kind: 'goal_reached', winner: stepped.winner };
      st.status_headline = `Seat ${stepped.winner} Wins! Automaton reached (${st.board.auto[0]},${st.board.auto[1]}).`;
    } else if (st.turn_no >= st.max_turns) {
      st.ended = true;
      const dA = goalDistanceClient(st.board.auto, st.goals, 0) ?? 99;
      const dB = goalDistanceClient(st.board.auto, st.goals, 1) ?? 99;
      st.winner = dA < dB ? 'A' : dB < dA ? 'B' : null;
      st.finish_reason = st.winner ? { kind: 'turn_cap_adjudicated', winner: st.winner } : { kind: 'draw_by_turn_cap' };
      st.status_headline = st.winner ? `Turn Cap Adjudicated — Seat ${st.winner} Wins!` : 'Turn Cap Reached — Draw!';
    } else {
      st.status_headline = `Turn ${st.turn_no + 1} — Last pulse: @ (${autoBefore[0]},${autoBefore[1]}) → (${st.board.auto[0]},${st.board.auto[1]}) [${stepped.sense.why}]`;
    }
    renderGameView(buildLocalGameView());
    return;
  }
}

// ============================================================================
// Routing, Lobby, Server API & Zero-Round-Trip Board Interaction
// ============================================================================
const pathParts = window.location.pathname.split('/').filter(Boolean);
const isGamePage = (pathParts[0] === 'game' || pathParts[0] === 'watch') && pathParts[1];
let currentGameId = isGamePage ? pathParts[1] : null;
const urlParams = new URLSearchParams(window.location.search);

let gameView = null;
let localSelectedCoord = null;
let hoveredTargetCoord = null;

function getStoredToken(gameId) {
  const fromUrl = urlParams.get('token');
  if (fromUrl) {
    localStorage.setItem('automatafl_token_' + gameId, fromUrl);
    return fromUrl;
  }
  return localStorage.getItem('automatafl_token_' + gameId) || '';
}

function buildConfigFromForm() {
  const preset = document.getElementById('cfg-preset').value;
  const tbRaw = document.getElementById('cfg-tiebreak').value;
  let tie_break = { mode: 'column' };
  if (tbRaw === 'row') tie_break = { mode: 'row' };
  else if (tbRaw === 'freeze') tie_break = { mode: 'freeze' };
  else if (tbRaw === 'momentum_same') tie_break = { mode: 'momentum', prefer_same_axis: true };
  else if (tbRaw === 'momentum_orth') tie_break = { mode: 'momentum', prefer_same_axis: false };

  const automataEl = document.getElementById('cfg-automata');
  const automata_count = automataEl ? parseInt(automataEl.value || '1', 10) : 1;

  return {
    preset,
    tie_break,
    confluence: document.getElementById('cfg-confluence').value,
    commit_mode: document.getElementById('cfg-commit').value,
    two_cycle_swaps: document.getElementById('cfg-swaps').checked,
    cooldown_turns: parseInt(document.getElementById('cfg-cooldown').value || '0', 10),
    railgun_fling: document.getElementById('cfg-railgun').checked,
    automata_count
  };
}

async function startBotGame() {
  const strategy = document.getElementById('bot-strategy').value;
  const humanSeat = document.getElementById('bot-human-seat').value;
  const hostModeEl = document.getElementById('bot-host-mode');
  const hostMode = hostModeEl ? hostModeEl.value : 'client_local';
  const config = buildConfigFromForm();

  if (hostMode === 'client_local') {
    document.getElementById('lobby-view').style.display = 'none';
    document.getElementById('game-view').style.display = 'block';
    currentGameId = 'local-ai';
    createLocalBotMatch(config, strategy, humanSeat);
    return;
  }

  const res = await fetch('/api/games', {
    method: 'POST',
    headers: {'Content-Type': 'application/json'},
    body: JSON.stringify({
      config,
      vs_bot: strategy,
      creator_seat: humanSeat,
      player_name: 'Player'
    })
  });
  const data = await res.json();
  if (data.creator_token) {
    localStorage.setItem('automatafl_token_' + data.game_id, data.creator_token);
  }
  window.location.href = '/game/' + data.game_id;
}

async function createOpenTable() {
  const config = buildConfigFromForm();
  const name = document.getElementById('open-name').value || 'Host';
  const res = await fetch('/api/games', {
    method: 'POST',
    headers: {'Content-Type': 'application/json'},
    body: JSON.stringify({
      config,
      creator_seat: 'A',
      player_name: name
    })
  });
  const data = await res.json();
  if (data.creator_token) {
    localStorage.setItem('automatafl_token_' + data.game_id, data.creator_token);
  }
  if (data.open_invite_token) {
    localStorage.setItem('automatafl_invite_' + data.game_id, data.open_invite_token);
  }
  window.location.href = '/game/' + data.game_id;
}

let mmTicket = null;
let mmTimer = null;

async function toggleMatchmaking() {
  if (mmTicket) {
    await fetch('/api/matchmaking/seek/' + mmTicket, { method: 'DELETE' });
    mmTicket = null;
    clearInterval(mmTimer);
    document.getElementById('mm-btn').textContent = 'Join Matchmaking Queue';
    document.getElementById('mm-status').textContent = 'Cancelled matchmaking seek.';
    return;
  }
  const config = buildConfigFromForm();
  const player_name = document.getElementById('mm-name').value || null;
  const res = await fetch('/api/matchmaking/seek', {
    method: 'POST',
    headers: {'Content-Type': 'application/json'},
    body: JSON.stringify({ player_name, config, any_variant_with_same_board: true })
  });
  const status = await res.json();
  handleMmStatus(status);
  if (status.status === 'queued') {
    mmTicket = status.ticket_id;
    document.getElementById('mm-btn').textContent = 'Cancel Queue (' + mmTicket + ')';
    mmTimer = setInterval(pollMatchmaking, 1000);
  }
}

async function pollMatchmaking() {
  if (!mmTicket) return;
  const res = await fetch('/api/matchmaking/status/' + mmTicket);
  if (!res.ok) return;
  const status = await res.json();
  handleMmStatus(status);
}

function handleMmStatus(status) {
  const el = document.getElementById('mm-status');
  if (status.status === 'matched') {
    clearInterval(mmTimer);
    localStorage.setItem('automatafl_token_' + status.game_id, status.seat_token);
    el.textContent = 'Matched! Redirecting to game ' + status.game_id + '...';
    window.location.href = '/game/' + status.game_id;
  } else if (status.status === 'queued') {
    el.textContent = 'Queued (' + status.preset + ', position #' + status.position_in_queue + ')... waiting for opponent.';
  }
}

async function loadLobby(force) {
  if (!force && typeof document !== 'undefined' && document.hidden) return;
  const res = await fetch('/api/games');
  const games = await res.json();
  const tbody = document.getElementById('lobby-tables');
  if (!games.length) {
    tbody.innerHTML = '<tr><td colspan="5">No active tables yet. Start a bot game or open a table!</td></tr>';
    return;
  }
  tbody.innerHTML = games.map(g => {
    const seats = `${g.seat_a_name || 'Open'} vs ${g.seat_b_name || 'Open'}`;
    const action = g.has_open_seat
      ? `<a class="btn" href="/game/${g.id}?join=1">Join Table</a>`
      : `<a class="btn secondary" href="/watch/${g.id}">Spectate</a>`;
    return `<tr>
      <td class="mono"><a href="/game/${g.id}" style="color:#6cb8ff;">${g.id}</a></td>
      <td>${g.config.preset} (${g.config.confluence})</td>
      <td>${g.phase} · Turn ${g.turn_no}/${g.max_turns}</td>
      <td>${seats}</td>
      <td>${action}</td>
    </tr>`;
  }).join('');
}

async function apiPost(endpoint, body) {
  const token = getStoredToken(currentGameId);
  const res = await fetch(`/api/games/${currentGameId}/${endpoint}`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...(token ? { 'Authorization': 'Bearer ' + token } : {})
    },
    body: JSON.stringify(body || {})
  });
  const data = await res.json();
  if (!res.ok) {
    alert(data.error || 'Action failed');
    return null;
  }
  if (data.view) {
    renderGameView(data.view);
  }
  return data;
}

async function fetchGameState() {
  if (localMatchState && currentGameId === 'local-ai') {
    renderGameView(buildLocalGameView());
    return;
  }
  const token = getStoredToken(currentGameId);
  const res = await fetch(`/api/games/${currentGameId}`, {
    headers: token ? { 'Authorization': 'Bearer ' + token } : {}
  });
  if (!res.ok) return;
  const view = await res.json();
  renderGameView(view);
}

function renderGameView(v) {
  gameView = v;
  const banner = document.getElementById('status-banner');
  banner.textContent = v.status_headline;

  const viewerStr = v.viewer_seat === 'A' ? 'Seat A (North)' : v.viewer_seat === 'B' ? 'Seat B (South)' : 'Spectator';
  document.getElementById('viewer-badge').textContent = 'Viewing as: ' + viewerStr;

  renderSeatCard(0, v.seats[0]);
  renderSeatCard(1, v.seats[1]);

  const mySeatIdx = v.viewer_seat === 'A' ? 0 : v.viewer_seat === 'B' ? 1 : -1;
  const mySeat = mySeatIdx >= 0 ? v.seats[mySeatIdx] : null;
  const canAct = mySeat !== null;
  const hasOpenSeat = !v.seats[0].is_claimed || !v.seats[1].is_claimed;
  const canPlayMove = canAct && !v.ended && mySeat.is_waiting && !mySeat.has_committed && (v.phase === 'commit' || v.phase === 'resubmit');

  // If the round advanced or our selection is no longer selectable, clear localSelectedCoord
  const cooldownSquares = v.cooldown_squares || (v.board.cooldowns || []).filter(cd => cd[1] > 0).map(cd => cd[0]);
  if (!canPlayMove || (localSelectedCoord && !isSelectableClient(v.board, v.marks || [], cooldownSquares, localSelectedCoord))) {
    localSelectedCoord = null;
  }
  const activeSel = localSelectedCoord || (mySeat ? mySeat.own_selection : null);

  document.getElementById('btn-reveal').style.display = (canAct && v.phase === 'reveal' && !mySeat.has_revealed) ? 'inline-flex' : 'none';
  document.getElementById('btn-resolve').style.display = (v.phase === 'resolve') ? 'inline-flex' : 'none';
  document.getElementById('btn-clear-sel').style.display = (canAct && activeSel) ? 'inline-flex' : 'none';
  document.getElementById('btn-local-ai-move').style.display = canPlayMove ? 'inline-flex' : 'none';
  document.getElementById('btn-resign').style.display = (canAct && !v.ended && currentGameId !== 'local-ai') ? 'inline-flex' : 'none';
  document.getElementById('btn-join-seat').style.display = (!canAct && hasOpenSeat && !v.ended) ? 'inline-flex' : 'none';
  document.getElementById('btn-rematch').style.display = v.ended ? 'inline-flex' : 'none';

  // Client-side target & forecast calculation (0ms, zero network round-trip)
  const clientTargets = activeSel
    ? computeTargetsClient(v.board, v.marks || [], cooldownSquares, v.locked_moves || [], activeSel)
    : { legal: [], executable: [] };

  const lastStepDir = lastAutoDeltaFromView(v, v.board);
  const forecastEl = document.getElementById('own-forecast');
  if (mySeat && mySeat.visible_move && !mySeat.has_revealed) {
    const m = mySeat.visible_move;
    const fc = mySeat.own_move_forecast || ownMoveForecastClient(v.board, v.marks || [], v.locked_moves || [], m, v.config, v.goals, mySeatIdx >= 0 ? mySeatIdx : 0, lastStepDir);
    forecastEl.innerHTML = `<span class="badge fog-sealed">Your Sealed Move: (${m.frm[0]},${m.frm[1]}) → (${m.to[0]},${m.to[1]})</span> <span>${fc}</span>`;
  } else if (activeSel && hoveredTargetCoord && coordIn(hoveredTargetCoord, clientTargets.legal)) {
    const previewMv = { who: mySeatIdx >= 0 ? mySeatIdx : 0, frm: activeSel, to: hoveredTargetCoord };
    const fc = ownMoveForecastClient(v.board, v.marks || [], v.locked_moves || [], previewMv, v.config, v.goals, mySeatIdx >= 0 ? mySeatIdx : 0, lastStepDir);
    forecastEl.innerHTML = `<span class="badge fog-locked">Client Forecast (${activeSel[0]},${activeSel[1]})→(${hoveredTargetCoord[0]},${hoveredTargetCoord[1]}):</span> <span>${fc}</span>`;
  } else if (activeSel) {
    forecastEl.innerHTML = `<span class="badge">Selected (${activeSel[0]},${activeSel[1]})</span> ${clientTargets.executable.length} executable / ${clientTargets.legal.length} proposable rook targets (computed client-side in 0ms). Hover a target to preview @ pulse.`;
  } else {
    forecastEl.textContent = v.next_step_hint || '';
  }

  // Client-side sense & standing fallback
  const clientSense = senseAtOriginClient(v.board, v.board.auto, v.config, lastStepDir);
  const distA = (v.standing && v.standing.dist && v.standing.dist[0] != null)
    ? v.standing.dist[0]
    : (goalDistanceClient(v.board.auto, v.goals, 0) ?? '-');
  const distB = (v.standing && v.standing.dist && v.standing.dist[1] != null)
    ? v.standing.dist[1]
    : (goalDistanceClient(v.board.auto, v.goals, 1) ?? '-');
  const nextAutoCoord = v.automaton_next_target || clientSense.target;
  const nextAuto = nextAutoCoord ? `(${nextAutoCoord[0]},${nextAutoCoord[1]})` : 'stay';
  const nextWhy = v.automaton_next_why || clientSense.why;
  document.getElementById('sense-panel').innerHTML = `
    <div>Automaton @ (${v.board.auto[0]},${v.board.auto[1]}) → Pulse Preview: <strong>${nextAuto}</strong> (${nextWhy})</div>
    <div>Manhattan Dist to Goal: <span style="color:#6cb8ff;">Seat A = ${distA}</span> | <span style="color:#ffb84d;">Seat B = ${distB}</span></div>
  `;

  // Render Turn History panel (matches `session.rs::TurnSummary`)
  const histEl = document.getElementById('history-log');
  const hist = v.turn_history || [];
  if (!hist.length) {
    histEl.innerHTML = 'No completed turns yet.';
  } else {
    histEl.innerHTML = hist.slice().reverse().map(t => {
      const turnNo = (t.turn_no != null ? t.turn_no + 1 : (t.turn_index ?? 1));
      const moves = t.moves || [];
      const mvA = moves.find(m => m.who === 0) || t.move_a;
      const mvB = moves.find(m => m.who === 1) || t.move_b;
      const mA = mvA ? `(${mvA.frm[0]},${mvA.frm[1]})→(${mvA.to[0]},${mvA.to[1]})` : '—';
      const mB = mvB ? `(${mvB.frm[0]},${mvB.frm[1]})→(${mvB.to[0]},${mvB.to[1]})` : '—';
      const as = t.auto_step || { from: t.auto_before || [0, 0], to: t.auto_after || [0, 0] };
      const autoStep = `@ (${as.from[0]},${as.from[1]})→(${as.to[0]},${as.to[1]})`;
      const clashesNote = t.conflict_rounds ? ` · ×:${t.conflict_rounds}` : '';
      return `<div class="history-entry"><strong>T${turnNo}:</strong> A ${mA} | B ${mB} | ${autoStep}${clashesNote}</div>`;
    }).join('');
  }

  const c = v.config;
  document.getElementById('variant-badges').innerHTML = `
    <span class="badge">${c.preset}</span>
    <span class="badge">Confluence: ${c.confluence}</span>
    <span class="badge">Tie-Break: ${c.tie_break.mode}</span>
    <span class="badge">Swaps: ${c.two_cycle_swaps ? 'On' : 'Off'}</span>
    <span class="badge">Cooldown: ${c.cooldown_turns}</span>
    <span class="badge">Railgun: ${c.railgun_fling ? 'On' : 'Off'}</span>
    <span class="badge">Automata: ${c.automata_count || 1}</span>
  `;

  const origin = window.location.origin;
  if (currentGameId === 'local-ai') {
    document.getElementById('share-links').innerHTML = `<div>Running 100% in your browser (Zero Server Load).</div>`;
  } else {
    let linksHtml = `<div>Spectator URL: <a href="${origin}/watch/${v.id}" style="color:#6cb8ff;">${origin}/watch/${v.id}</a></div>`;
    if (hasOpenSeat) {
      const inviteUrl = `${origin}/game/${v.id}?join=1`;
      linksHtml += `<div style="margin-top:0.35rem;">Opponent Direct Seat Link: <a href="${inviteUrl}" style="color:#ffb84d;">${inviteUrl}</a></div>`;
    }
    linksHtml += `<div style="margin-top:0.35rem;"><a href="/api/games/${v.id}/replay" target="_blank" style="color:#98f5be;">Download Verified Replay JSON (${v.turns_played_count} turns)</a></div>`;
    document.getElementById('share-links').innerHTML = linksHtml;
  }

  renderBoard(v, mySeat, activeSel, clientTargets, cooldownSquares);
}

function renderSeatCard(idx, s) {
  const el = document.getElementById('seat-' + idx + '-card');
  const label = idx === 0 ? 'Seat A (North)' : 'Seat B (South)';
  const who = s.player_name || 'Open Seat';
  let stateBadge = '<span class="badge">Thinking...</span>';
  if (s.is_locked && s.visible_move) {
    const m = s.visible_move;
    stateBadge = `<span class="badge fog-locked">Locked: (${m.frm[0]},${m.frm[1]})→(${m.to[0]},${m.to[1]})</span>`;
  } else if (s.has_revealed && s.visible_move) {
    const m = s.visible_move;
    stateBadge = `<span class="badge fog-locked">Revealed: (${m.frm[0]},${m.frm[1]})→(${m.to[0]},${m.to[1]})</span>`;
  } else if (s.has_committed) {
    stateBadge = `<span class="badge fog-sealed">Sealed (${s.commitment_seal || 'Fog'})</span>`;
  }
  el.innerHTML = `<div><strong>${label}</strong> · ${who}</div><div>${stateBadge}</div>`;
}

// Compact board renderer: derives every square directly from `v.board`, `v.marks`,
// `v.goals`, `cooldownSquares`, and client-side sightlines/targets (never requires `v.cells`).
function renderBoard(v, mySeat, activeSel, clientTargets, cooldownSquares) {
  const grid = document.getElementById('board-grid');
  const N = v.board.n;
  grid.style.gridTemplateColumns = `repeat(${N}, 1fr)`;

  const ownMv = mySeat ? mySeat.visible_move : null;
  const sightlines = computeSightlinesClient(v.board);
  const marks = v.marks || [];
  const goals = v.goals || [];
  const lastStep = v.last_step || null;
  const hist = v.turn_history || [];
  const lastTurn = hist.length > 0 ? hist[hist.length - 1] : null;
  const lastMoves = (lastTurn && Array.isArray(lastTurn.moves)) ? lastTurn.moves : [];
  const lastSrcSquares = lastMoves.map(m => m.frm).filter(Boolean);
  const lastDstSquares = (lastTurn && Array.isArray(lastTurn.landed_squares) && lastTurn.landed_squares.length > 0)
    ? lastTurn.landed_squares
    : lastMoves.map(m => m.to).filter(Boolean);
  const lastAutoFromCoord = (lastStep && Array.isArray(lastStep.from))
    ? lastStep.from
    : (lastStep && Array.isArray(lastStep.auto_from) ? lastStep.auto_from : null);

  let html = '';
  for (let y = 0; y < N; y++) {
    for (let x = 0; x < N; x++) {
      const c = [x, y];
      const particle = cellAt(v.board, x, y);
      const dark = (x + y) % 2 === 1;
      const isSelected = coordEq(activeSel, c);
      const isOwnSrc = ownMv && coordEq(ownMv.frm, c);
      const isOwnDst = ownMv && coordEq(ownMv.to, c);
      const isMarked = coordIn(c, marks);
      const isCooldown = isOnCooldownClient(v.board, c, cooldownSquares);
      const isSightline = coordIn(c, sightlines);
      const isExecTarget = coordIn(c, clientTargets.executable);
      const isPropTarget = coordIn(c, clientTargets.legal);

      const isLastSrc = coordIn(c, lastSrcSquares);
      const isLastDst = coordIn(c, lastDstSquares);
      const isLastAutoFrom = lastAutoFromCoord && coordEq(lastAutoFromCoord, c);

      const classes = ['sq'];
      if (dark) classes.push('dark');
      if (isSightline) classes.push('sightline');
      if (isMarked) classes.push('marked');
      if (isCooldown) classes.push('cooldown');
      if (isLastSrc) classes.push('last-move-src');
      if (isLastDst) classes.push('last-move-dst');
      if (isLastAutoFrom) classes.push('last-auto-from');
      if (isSelected) classes.push('selected');
      if (isExecTarget) classes.push('target-exec');
      else if (isPropTarget) classes.push('target-blocked');
      if (isOwnSrc) classes.push('own-move-src');
      if (isOwnDst) classes.push('own-move-dst');

      let pieceHtml = '';
      if (particle === REP) pieceHtml = '<div class="piece piece-repulsor" title="Repulsor (Pushes @)">R</div>';
      else if (particle === ATT) pieceHtml = '<div class="piece piece-attractor" title="Attractor (Pulls @)">A</div>';
      else if (particle === AUTO) pieceHtml = '<div class="piece piece-automaton" title="Automaton (@)">@</div>';

      let goalHtml = '';
      const goalEntry = goals.find(g => coordEq(g[0], c));
      if (goalEntry) {
        const gSeat = goalEntry[1] === 0 ? 'A' : 'B';
        goalHtml = `<span class="goal-badge ${gSeat === 'A' ? 'goal-a' : 'goal-b'}">${gSeat}</span>`;
      }

      const markHtml = isMarked ? '<div class="mark-x">×</div>' : '';
      const cdHtml = isCooldown ? '<span class="cooldown-badge" title="Piece on Cooldown (❄️)">❄️</span>' : '';
      const coordHtml = (x === 0 || y === N - 1) ? `<span class="coord-label">${x},${y}</span>` : '';

      html += `<div class="${classes.join(' ')}" onclick="onSquareClick(${x},${y})" onmouseenter="onSquareHover(${x},${y})">${goalHtml}${pieceHtml}${markHtml}${cdHtml}${coordHtml}</div>`;
    }
  }
  grid.innerHTML = html;
}

// Zero-round-trip piece selection & move submission
async function onSquareClick(x, y) {
  if (!gameView || gameView.ended) return;
  const mySeatIdx = gameView.viewer_seat === 'A' ? 0 : gameView.viewer_seat === 'B' ? 1 : -1;
  if (mySeatIdx < 0) return;
  const mySeat = gameView.seats[mySeatIdx];
  if (!mySeat.is_waiting || mySeat.has_committed) return;
  if (gameView.phase !== 'commit' && gameView.phase !== 'resubmit') return;

  const clicked = [x, y];
  const cooldownSquares = gameView.cooldown_squares || [];
  const activeSel = localSelectedCoord || mySeat.own_selection;

  if (activeSel && coordEq(activeSel, clicked)) {
    clearSelection();
    return;
  }

  if (activeSel) {
    const targets = computeTargetsClient(gameView.board, gameView.marks || [], cooldownSquares, gameView.locked_moves || [], activeSel);
    if (coordIn(clicked, targets.legal)) {
      const from = activeSel;
      localSelectedCoord = null;
      hoveredTargetCoord = null;
      if (currentGameId === 'local-ai' && localMatchState) {
        await submitLocalMove(from, clicked);
      } else {
        await apiPost('move', { from, to: clicked });
      }
      return;
    }
  }

  // Zero-round-trip client-side selection (0ms, no HTTP POST /select!)
  if (isSelectableClient(gameView.board, gameView.marks || [], cooldownSquares, clicked)) {
    localSelectedCoord = clicked;
    hoveredTargetCoord = null;
    renderGameView(gameView);
  } else if (activeSel) {
    clearSelection();
  }
}

function onSquareHover(x, y) {
  if (!gameView || !localSelectedCoord) return;
  const nextHover = [x, y];
  if (!coordEq(hoveredTargetCoord, nextHover)) {
    hoveredTargetCoord = nextHover;
    const cooldownSquares = gameView.cooldown_squares || [];
    const clientTargets = computeTargetsClient(gameView.board, gameView.marks || [], cooldownSquares, gameView.locked_moves || [], localSelectedCoord);
    const forecastEl = document.getElementById('own-forecast');
    if (coordIn(hoveredTargetCoord, clientTargets.legal)) {
      const mySeatIdx = gameView.viewer_seat === 'A' ? 0 : gameView.viewer_seat === 'B' ? 1 : 0;
      const lastStepDir = lastAutoDeltaFromView(gameView, gameView.board);
      const previewMv = { who: mySeatIdx, frm: localSelectedCoord, to: hoveredTargetCoord };
      const fc = ownMoveForecastClient(gameView.board, gameView.marks || [], gameView.locked_moves || [], previewMv, gameView.config, gameView.goals, mySeatIdx, lastStepDir);
      forecastEl.innerHTML = `<span class="badge fog-locked">Client Forecast (${localSelectedCoord[0]},${localSelectedCoord[1]})→(${x},${y}):</span> <span>${fc}</span>`;
    }
  }
}

function clearSelection() {
  localSelectedCoord = null;
  hoveredTargetCoord = null;
  if (gameView) {
    renderGameView(gameView);
  }
}

async function triggerLocalAiAssist() {
  if (!gameView || gameView.ended || !gameView.viewer_seat) return;
  const cooldownSquares = gameView.cooldown_squares || [];
  const mv = await chooseBotMoveClientAsync(
    'decoupled_uct',
    gameView.board,
    gameView.goals || [],
    gameView.marks || [],
    cooldownSquares,
    gameView.locked_moves || [],
    gameView.config,
    gameView.viewer_seat
  );
  if (!mv) return;
  localSelectedCoord = null;
  if (currentGameId === 'local-ai' && localMatchState) {
    await submitLocalMove(mv.frm, mv.to);
  } else {
    await apiPost('move', { from: mv.frm, to: mv.to });
  }
}

async function rematchGame() {
  if (currentGameId === 'local-ai' && localMatchState) {
    createLocalBotMatch(localMatchState.config, localMatchState.strategy, localMatchState.humanSeat);
    return;
  }
  if (!gameView) return;
  const botSeat = (gameView.seats || []).find(s => s && s.is_bot);
  const vs_bot = botSeat ? (botSeat.bot_strategy || 'decoupled_uct') : null;
  const res = await fetch('/api/games', {
    method: 'POST',
    headers: {'Content-Type': 'application/json'},
    body: JSON.stringify({
      config: gameView.config,
      vs_bot,
      creator_seat: gameView.viewer_seat || 'A',
      player_name: 'Player'
    })
  });
  const data = await res.json();
  if (data.creator_token) {
    localStorage.setItem('automatafl_token_' + data.game_id, data.creator_token);
  }
  window.location.href = '/game/' + data.game_id;
}

async function triggerReveal() { await apiPost('reveal', {}); }
async function triggerResolve() { await apiPost('resolve', {}); }

async function resignGame() {
  if (!confirm('Resign this game?')) return;
  await apiPost('resign', {});
}

async function joinOpenSeat() {
  const res = await fetch(`/api/games/${currentGameId}/join`, {
    method: 'POST',
    headers: {'Content-Type': 'application/json'},
    body: JSON.stringify({ player_name: 'Challenger' })
  });
  const data = await res.json();
  if (!res.ok) {
    alert(data.error || 'Could not join seat');
    return;
  }
  localStorage.setItem('automatafl_token_' + currentGameId, data.seat_token);
  await fetchGameState();
  connectSse();
}

let sseSource = null;
function connectSse() {
  if (currentGameId === 'local-ai') return;
  if (sseSource) sseSource.close();
  const token = getStoredToken(currentGameId);
  const qs = token ? ('?token=' + encodeURIComponent(token)) : '';
  sseSource = new EventSource(`/api/games/${currentGameId}/events${qs}`);
  sseSource.onmessage = (ev) => {
    try {
      const view = JSON.parse(ev.data);
      renderGameView(view);
    } catch (e) {}
  };
}

// ============================================================================
// Live Telemetry, Network Ping & Lichess-Style System Status Dashboard
// ============================================================================
const isStatusPage = pathParts[0] === 'status' || pathParts[0] === 'lag';
const clientPingHistory = [];

function escapeHtml(s) {
  return String(s == null ? '' : s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function formatDurationSec(sec) {
  const d = Math.floor(sec / 86400);
  const h = Math.floor((sec % 86400) / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = Math.floor(sec % 60);
  if (d > 0) return `${d}d ${h}h ${m}m`;
  if (h > 0) return `${h}h ${m}m ${s}s`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

function setGaugeBar(barId, pct) {
  const el = document.getElementById(barId);
  if (!el) return;
  const clamped = Math.max(0, Math.min(100, pct || 0));
  el.style.width = clamped.toFixed(1) + '%';
  el.className = 'gauge-bar-fill' + (clamped >= 85 ? ' crit' : clamped >= 65 ? ' warn' : '');
}

function renderSparklineSvg(svgId, seriesList, fixedMax) {
  const svg = document.getElementById(svgId);
  if (!svg) return;
  const W = 360, H = 96, PAD = 6;
  let maxVal = fixedMax || 1;
  for (const s of seriesList) {
    for (const v of s.values) {
      if (v > maxVal) maxVal = v;
    }
  }
  if (maxVal <= 0) maxVal = 1;

  let html = '';
  for (let g = 1; g <= 3; g++) {
    const gy = Math.round((H / 4) * g);
    html += `<line x1="0" y1="${gy}" x2="${W}" y2="${gy}" stroke="#262421" stroke-width="1"/>`;
  }

  for (const s of seriesList) {
    const vals = s.values;
    if (!vals || vals.length === 0) continue;
    const pts = vals.map((v, i) => {
      const x = vals.length === 1 ? W : (i / (vals.length - 1)) * W;
      const y = H - PAD - (Math.min(v, maxVal) / maxVal) * (H - PAD * 2);
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    });
    if (s.fill) {
      const areaPts = [`0,${H}`, ...pts, `${W},${H}`].join(' ');
      html += `<polygon points="${areaPts}" fill="${s.fill}" />`;
    }
    html += `<polyline fill="none" stroke="${s.color}" stroke-width="2" points="${pts.join(' ')}" />`;
  }
  svg.innerHTML = html;
}

function updateHeaderStatusPill(st, rttMs) {
  const dot = document.getElementById('nav-status-dot');
  const pingEl = document.getElementById('nav-ping-txt');
  const srvEl = document.getElementById('nav-srv-txt');
  const ffiEl = document.getElementById('nav-ffi-txt');
  const ramEl = document.getElementById('nav-ram-txt');
  if (!pingEl) return;

  const memBytes = st.resources.memory_current_bytes || st.resources.rss_bytes || 0;
  const rssMib = (memBytes / 1048576).toFixed(1);
  const maxMib = (st.resources.memory_max_bytes || 268435456) / 1048576;
  const memPct = (rssMib / maxMib) * 100;
  const srvUs = st.http_latency.p50_us;
  const ffiUs = st.lean_ffi_latency.avg_us;

  pingEl.textContent = `PING ${rttMs.toFixed(1)}ms`;
  srvEl.textContent = `SRV ${srvUs}µs`;
  ffiEl.textContent = `FFI ${ffiUs}µs`;
  ramEl.textContent = `RAM ${rssMib}MiB`;

  if (dot) {
    if (rttMs > 350 || memPct > 85 || st.lean_ffi_errors_total > 0) {
      dot.className = 'status-dot crit';
    } else if (rttMs > 150 || memPct > 65) {
      dot.className = 'status-dot warn';
    } else {
      dot.className = 'status-dot';
    }
  }
}

function renderStatusDashboard(st, rttMs) {
  const pings = clientPingHistory;
  const minPing = pings.length ? Math.min(...pings) : rttMs;
  const avgPing = pings.length ? (pings.reduce((a, b) => a + b, 0) / pings.length) : rttMs;
  let jitter = 0;
  if (pings.length > 1) {
    let diffSum = 0;
    for (let i = 1; i < pings.length; i++) diffSum += Math.abs(pings[i] - pings[i - 1]);
    jitter = diffSum / (pings.length - 1);
  }

  document.getElementById('st-kpi-ping').textContent = `${rttMs.toFixed(1)} ms`;
  document.getElementById('st-kpi-ping-sub').textContent =
    `min ${minPing.toFixed(1)}ms • avg ${avgPing.toFixed(1)}ms • jitter ±${jitter.toFixed(1)}ms`;

  const hLat = st.http_latency;
  document.getElementById('st-kpi-http').textContent = `${hLat.p50_us} µs`;
  document.getElementById('st-kpi-http-sub').textContent =
    `avg ${hLat.avg_us}µs • p95 ${hLat.p95_us}µs • p99 ${hLat.p99_us}µs • reqs ${st.http_requests_total}`;

  const fLat = st.lean_ffi_latency;
  document.getElementById('st-kpi-ffi').textContent = `${fLat.avg_us} µs`;
  document.getElementById('st-kpi-ffi-sub').textContent =
    `p95 ${fLat.p95_us}µs • max ${fLat.max_us}µs • calls ${st.lean_ffi_calls_total} (${st.lean_ffi_errors_total} err)`;

  document.getElementById('st-kpi-uptime').textContent = formatDurationSec(st.uptime_secs);
  document.getElementById('st-kpi-uptime-sub').textContent =
    `SSE Streams: ${st.active_sse_streams} • Active Games: ${st.games.active_games}`;

  const res = st.resources;
  document.getElementById('st-cgroup-path').textContent = 'automatafl.slice';

  const memBytes = res.memory_current_bytes || res.rss_bytes || 0;
  const rssMib = memBytes / 1048576;
  const maxMib = (res.memory_max_bytes || 268435456) / 1048576;
  const peakMib = res.memory_peak_bytes / 1048576;
  const memPct = (rssMib / maxMib) * 100;
  document.getElementById('gauge-mem-txt').textContent =
    `${rssMib.toFixed(2)} / ${maxMib.toFixed(0)} MiB (${memPct.toFixed(2)}%)`;
  document.getElementById('gauge-mem-sub').textContent =
    `Peak RSS: ${peakMib.toFixed(2)} MiB • Soft High: ${((res.memory_high_bytes || 201326592) / 1048576).toFixed(0)} MiB`;
  setGaugeBar('gauge-mem-bar', memPct);

  const hist = st.series || [];
  const latestSample = hist.length ? hist[hist.length - 1] : null;
  const cpuPct = res.cpu_percent || (latestSample ? latestSample.cpu_pct : 0);
  const cpuSec = (res.cpu_usage_usec / 1e6).toFixed(2);
  document.getElementById('gauge-cpu-txt').textContent = `${cpuPct.toFixed(2)}% / ${(res.cpu_quota_pct || 100).toFixed(0)}%`;
  document.getElementById('gauge-cpu-sub').textContent =
    `Cumulative CPU: ${cpuSec}s (user ${(res.cpu_user_usec / 1e6).toFixed(2)}s / sys ${(res.cpu_system_usec / 1e6).toFixed(2)}s) • LoadAvg: ${res.load_avg_1m.toFixed(2)} / ${res.load_avg_5m.toFixed(2)} / ${res.load_avg_15m.toFixed(2)}`;
  setGaugeBar('gauge-cpu-bar', cpuPct);

  const diskMib = res.disk_used_bytes / 1048576;
  const diskMaxMib = res.disk_max_bytes / 1048576;
  const diskPct = (res.disk_used_bytes / Math.max(1, res.disk_max_bytes)) * 100;
  document.getElementById('gauge-disk-txt').textContent =
    `${diskMib.toFixed(3)} / ${diskMaxMib.toFixed(0)} MiB (${diskPct.toFixed(2)}%)`;
  document.getElementById('gauge-disk-sub').textContent =
    `Persisted Session Files: ${res.persisted_files_count} / ${res.max_persisted_files} (Auto-Pruned)`;
  setGaugeBar('gauge-disk-bar', Math.max(diskPct, (res.persisted_files_count / Math.max(1, res.max_persisted_files)) * 100));

  const g = st.games;
  const lruPct = (g.in_memory_games / Math.max(1, g.max_in_memory_games)) * 100;
  document.getElementById('gauge-lru-txt').textContent =
    `${g.in_memory_games} / ${g.max_in_memory_games} (${lruPct.toFixed(1)}%)`;
  document.getElementById('gauge-lru-sub').textContent =
    `Disk Saves: ${g.disk_saves_total} • Disk Loads: ${g.disk_loads_total} • LRU Evictions: ${g.cache_evictions_total}`;
  setGaugeBar('gauge-lru-bar', lruPct);

  const fdPct = (res.open_fds / Math.max(1, res.max_fds || 2048)) * 100;
  document.getElementById('gauge-fd-txt').textContent =
    `FDs: ${res.open_fds} / ${res.max_fds || 2048} • Tasks: ${res.tasks_current} / ${res.tasks_max || 128}`;
  setGaugeBar('gauge-fd-bar', fdPct);

  document.getElementById('st-games-cnt').textContent = `${g.games_created_total} / ${g.games_finished_total}`;
  document.getElementById('st-turns-cnt').textContent = `${g.turns_resolved_total}`;
  document.getElementById('st-clash-cnt').textContent = `${g.conflicts_triggered_total}`;
  document.getElementById('st-wins-cnt').textContent = `${g.wins_seat_a} / ${g.wins_seat_b} / ${g.draws_total}`;
  document.getElementById('st-mm-cnt').textContent = `${g.matchmaking_queued} / ${g.matchmaking_paired_total}`;
  document.getElementById('st-sse-cnt').textContent = `${st.sse_connections_total} / ${st.sse_events_sent_total}`;

  const b = st.bot;
  const bStrat = b.by_strategy || {};
  document.getElementById('st-bot-summary').innerHTML =
    `<div><strong>Total Server AI Moves:</strong> ${b.moves_total} • <strong>Avg Search:</strong> ${(b.avg_us / 1000).toFixed(2)} ms • <strong>Last:</strong> ${(b.last_us / 1000).toFixed(2)} ms • <strong>Max:</strong> ${(b.max_us / 1000).toFixed(2)} ms</div>` +
    `<div style="margin-top:0.3rem;color:#bababa;">Decoupled UCT: <strong>${bStrat.decoupled_uct || 0}</strong> • Regret Matching: <strong>${bStrat.regret_matching || 0}</strong> • Minimax: <strong>${bStrat.minimax || 0}</strong> • Tactical: <strong>${bStrat.tactical || 0}</strong> • Novice: <strong>${bStrat.novice || 0}</strong></div>`;

  const ep = st.endpoints || {};
  const epEntries = [
    ['list_games', ep.list_games || 0],
    ['create_game', ep.create_game || 0],
    ['get_view', ep.get_view || 0],
    ['join_game', ep.join_game || 0],
    ['select_piece', ep.select_piece || 0],
    ['submit_move', ep.submit_move || 0],
    ['commit_move', ep.commit_move || 0],
    ['reveal_round', ep.reveal_round || 0],
    ['resolve_round', ep.resolve_round || 0],
    ['resign_game', ep.resign_game || 0],
    ['replay', ep.replay || 0],
    ['matchmaking', ep.matchmaking || 0]
  ];
  document.getElementById('st-ep-grid').innerHTML = epEntries
    .map(([k, v]) => `<div style="background:var(--panel-alt);padding:0.25rem 0.45rem;border-radius:3px;border:1px solid var(--border);display:flex;justify-content:space-between;"><span>${k}</span><strong style="color:#fff;">${v}</strong></div>`)
    .join('');

  // Render 4 Sparklines
  document.getElementById('spark-ping-label').textContent = `${rttMs.toFixed(1)} ms`;
  renderSparklineSvg('spark-ping-svg', [
    { values: pings, color: '#84c832', fill: 'rgba(132, 200, 50, 0.16)' }
  ], 50);

  const httpAvgSeries = hist.map(h => h.http_avg_us);
  const ffiAvgSeries = hist.map(h => h.lean_avg_us);
  document.getElementById('spark-lat-label').textContent = `HTTP p95 ${hLat.p95_us}µs / FFI ${fLat.avg_us}µs`;
  renderSparklineSvg('spark-lat-svg', [
    { values: httpAvgSeries, color: '#3893e8', fill: 'rgba(56, 147, 232, 0.14)' },
    { values: ffiAvgSeries, color: '#f0ad4e', fill: null }
  ], 500);

  const memSeries = hist.map(h => h.rss_bytes / 1048576);
  document.getElementById('spark-mem-label').textContent = `${rssMib.toFixed(2)} / ${maxMib.toFixed(0)} MiB`;
  renderSparklineSvg('spark-mem-svg', [
    { values: memSeries, color: '#f0ad4e', fill: 'rgba(240, 173, 78, 0.16)' }
  ], Math.max(16, rssMib * 1.5));

  const rpsSeries = hist.map(h => h.req_per_sec);
  const sseSeries = hist.map(h => h.active_sse);
  const curRps = latestSample ? latestSample.req_per_sec : 0;
  document.getElementById('spark-rps-label').textContent = `${curRps} req/s • ${st.active_sse_streams} SSE`;
  renderSparklineSvg('spark-rps-svg', [
    { values: rpsSeries, color: '#98f5be', fill: 'rgba(152, 245, 190, 0.14)' },
    { values: sseSeries, color: '#6cb8ff', fill: null }
  ], 5);

  // Render Trace Feed
  const traces = st.recent_events || [];
  const tbody = document.getElementById('st-trace-body');
  if (traces.length === 0) {
    tbody.innerHTML = '<tr><td colspan="6" style="color:#888;">No trace events recorded yet. Start or play a game to see live structured traces!</td></tr>';
  } else {
    tbody.innerHTML = traces.map(t => {
      const dt = new Date(t.ts_ms).toISOString().replace('T', ' ').replace('Z', '');
      const gameCell = t.game_id
        ? `<a href="/watch/${encodeURIComponent(t.game_id)}" style="color:#6cb8ff;">${escapeHtml(t.game_id)}</a>`
        : '<span style="color:#666;">-</span>';
      const durCell = t.duration_us != null ? `${t.duration_us} µs` : '-';
      return `<tr>
        <td style="color:#888;">#${t.seq}</td>
        <td>${dt}</td>
        <td><span class="trace-kind ${escapeHtml(t.kind)}">${escapeHtml(t.kind)}</span></td>
        <td>${gameCell}</td>
        <td>${durCell}</td>
        <td>${escapeHtml(t.summary)}</td>
      </tr>`;
    }).join('');
  }
}

async function pollServerStatus(force) {
  if (!force && typeof document !== 'undefined' && document.hidden) return;
  try {
    const t0 = (typeof performance !== 'undefined' && performance.now) ? performance.now() : Date.now();
    const url = isStatusPage ? '/api/status' : '/api/status?lite=1';
    const res = await fetch(url, { cache: 'no-store' });
    if (!res.ok) return;
    const st = await res.json();
    if (!st || !st.resources || !st.http_latency || !st.lean_ffi_latency) return;
    const t1 = (typeof performance !== 'undefined' && performance.now) ? performance.now() : Date.now();
    const rttMs = Math.max(0.1, t1 - t0);
    clientPingHistory.push(rttMs);
    if (clientPingHistory.length > 60) clientPingHistory.shift();

    updateHeaderStatusPill(st, rttMs);
    if (isStatusPage) {
      renderStatusDashboard(st, rttMs);
    }
  } catch (e) {}
}

if (typeof document !== 'undefined' && typeof document.addEventListener === 'function') {
  document.addEventListener('visibilitychange', () => {
    if (!document.hidden) {
      pollServerStatus(true);
      if (!isStatusPage && !isGamePage) loadLobby(true);
    }
  });
}

if (isStatusPage) {
  document.getElementById('status-view').style.display = 'block';
  pollServerStatus(true);
  setInterval(() => pollServerStatus(false), 2000);
} else if (isGamePage) {
  document.getElementById('game-view').style.display = 'block';
  pollServerStatus(true);
  setInterval(() => pollServerStatus(false), 10000);
  if (urlParams.get('join') === '1' && !getStoredToken(currentGameId)) {
    joinOpenSeat();
  } else {
    fetchGameState();
    connectSse();
  }
} else {
  document.getElementById('lobby-view').style.display = 'block';
  loadLobby(true);
  pollServerStatus(true);
  setInterval(() => loadLobby(false), 5000);
  setInterval(() => pollServerStatus(false), 10000);
}
</script>
</body>
</html>
"##;
