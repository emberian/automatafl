//! Terminal User Interface (TUI) board & HUD renderer for Automatafl (`R6`).
//!
//! Renders any `GameView` (`11×11 Classic`, `9×9 Tactical`, `7×7 Blitz`, `5×5 Mini`)
//! with:
//! - Board grid, coordinates, goal corners (`a`, `b`), Repulsor (`R`), Attractor (`A`),
//!   Automaton (`@`), sightlines (`─`, `│`), conflict markers (`×`), executable targets (`+`),
//!   and occluded proposable targets (`?`).
//! - Active cursor (`{...}`), selected piece (`[...]`), and own sealed move (`<...>` -> `>...<`).
//! - Variant settings bar (`BoardPreset`, `TieBreakMode`, `ConfluenceMode`, `twoCycleSwaps`,
//!   `cooldownTurns`, `railgunFling`, `commitMode`).
//! - Strict fog-of-war status panel (displaying only `GameView`, so opponent unrevealed moves
//!   are never rendered).

use automatafl_server::protocol::{Coord, Seat, ATT, AUTO, REP};
use automatafl_server::session::{CellView, GameView, SeatView};

/// Render a single board cell into a 3-character slot (plain text, no ANSI escapes).
pub fn format_cell_slot_plain(
    cell: &CellView,
    my_seat: Option<&SeatView>,
    cursor: Option<Coord>,
) -> String {
    let c = (cell.x, cell.y);
    let is_cursor = cursor == Some(c);
    let is_selected = my_seat.and_then(|s| s.own_selection) == Some(c);
    let own_mv = my_seat.and_then(|s| s.visible_move);
    let is_own_src = own_mv.map(|m| m.frm == c).unwrap_or(false);
    let is_own_dst = own_mv.map(|m| m.to == c).unwrap_or(false);

    let center: char = if cell.is_marked {
        '×'
    } else {
        match cell.particle {
            REP => 'R',
            ATT => 'A',
            AUTO => '@',
            _ => {
                if cell.is_executable_target {
                    '+'
                } else if cell.is_proposable_target {
                    '?'
                } else if let Some(goal_seat) = cell.is_goal_of {
                    match goal_seat {
                        Seat::A => 'a',
                        Seat::B => 'b',
                    }
                } else if cell.is_sightline {
                    cell.glyph
                } else {
                    '·'
                }
            }
        }
    };

    if is_selected {
        format!("[{center}]")
    } else if is_cursor {
        format!("{{{center}}}")
    } else if is_own_dst {
        format!(">{center}<")
    } else if is_own_src {
        format!("<{center}>")
    } else if cell.is_marked {
        format!("!{center}!")
    } else if cell.is_on_cooldown {
        format!("*{center}*")
    } else {
        format!(" {center} ")
    }
}

/// Render a single board cell into a 3-character slot with ANSI color codes.
pub fn format_cell_slot_ansi(
    cell: &CellView,
    my_seat: Option<&SeatView>,
    cursor: Option<Coord>,
) -> String {
    let plain = format_cell_slot_plain(cell, my_seat, cursor);
    let c = (cell.x, cell.y);
    let is_cursor = cursor == Some(c);
    let is_selected = my_seat.and_then(|s| s.own_selection) == Some(c);

    let color = if is_cursor {
        "\x1b[1;97;44m" // bright white on blue cursor
    } else if cell.is_marked {
        "\x1b[1;91m" // bright red conflict marker ×
    } else if is_selected {
        "\x1b[1;92m" // bright green selected piece
    } else if cell.is_on_cooldown {
        "\x1b[1;94m" // frost blue cooldown piece *R* / *A*
    } else if cell.particle == AUTO {
        "\x1b[1;93m" // bright yellow automaton @
    } else if cell.particle == REP {
        "\x1b[1;31m" // red repulsor R
    } else if cell.particle == ATT {
        "\x1b[1;36m" // cyan attractor A
    } else if cell.is_executable_target {
        "\x1b[1;32m" // green executable target +
    } else if cell.is_proposable_target {
        "\x1b[33m" // amber occluded target ?
    } else if cell.is_sightline {
        "\x1b[90m" // dim sightline
    } else {
        "\x1b[0m"
    };

    format!("{color}{plain}\x1b[0m")
}

/// Render the full TUI frame in plain Unicode/ASCII (ideal for deterministic snapshot tests and non-ANSI terminals).
pub fn render_frame_plain(view: &GameView, cursor: Option<Coord>, mode_label: &str) -> String {
    render_frame_internal(view, cursor, mode_label, false)
}

/// Render the full TUI frame with ANSI colors.
pub fn render_frame_ansi(view: &GameView, cursor: Option<Coord>, mode_label: &str) -> String {
    render_frame_internal(view, cursor, mode_label, true)
}

fn render_frame_internal(
    view: &GameView,
    cursor: Option<Coord>,
    mode_label: &str,
    ansi: bool,
) -> String {
    let mut out = String::with_capacity(2048);
    let viewer_label = match view.viewer_seat {
        Some(Seat::A) => "Seat A (North Goals)",
        Some(Seat::B) => "Seat B (South Goals)",
        None => "Spectator (Fog of War)",
    };
    let my_seat = view.viewer_seat.map(|s| &view.seats[s.idx()]);

    // 1. Header Banner
    out.push_str(&format!(
        "=== AUTOMATAFL [{mode_label}] === Game: {} ({}) | Viewer: {viewer_label}\n",
        view.id, view.name
    ));
    out.push_str(&format!("{}\n", view.status_headline));

    // 2. Variant Bar (R2 Generalizations)
    let cfg = &view.config;
    out.push_str(&format!(
        "Variant: {} | TieBreak: {} | Confluence: {} | Swaps: {} | Cooldown: {} | Railgun: {}\n",
        cfg.preset.label(),
        cfg.tie_break.label(),
        cfg.confluence.label(),
        if cfg.two_cycle_swaps { "ON" } else { "OFF" },
        cfg.cooldown_turns,
        if cfg.railgun_fling { "ON" } else { "OFF" },
    ));
    out.push_str("--------------------------------------------------------------------------------\n");

    // 3. Board Grid
    let n = view.board.n;
    let derived_fallback;
    let cells: &[CellView] = if view.cells.len() == n * n {
        &view.cells
    } else {
        derived_fallback = view.derive_cells();
        &derived_fallback
    };

    out.push_str("    ");
    for x in 0..n {
        out.push_str(&format!("{x:^3}"));
    }
    out.push('\n');

    out.push_str("   +");
    for _ in 0..n {
        out.push_str("---");
    }
    out.push_str("+\n");

    for y in 0..n {
        out.push_str(&format!("{y:>2} |"));
        for x in 0..n {
            let cell = &cells[y * n + x];
            let slot = if ansi {
                format_cell_slot_ansi(cell, my_seat, cursor)
            } else {
                format_cell_slot_plain(cell, my_seat, cursor)
            };
            out.push_str(&slot);
        }
        out.push_str(&format!("| {y:<2}\n"));
    }

    out.push_str("   +");
    for _ in 0..n {
        out.push_str("---");
    }
    out.push_str("+\n");

    out.push_str("    ");
    for x in 0..n {
        out.push_str(&format!("{x:^3}"));
    }
    out.push_str("\n\n");

    // 4. Fog-of-War Seat Status Panel
    for s in &view.seats {
        let who = s.player_name.as_deref().unwrap_or("Open Seat");
        let role_tag = if s.is_you { " (YOU)" } else { "" };
        let status_str = if s.is_locked {
            if let Some(mv) = s.visible_move {
                format!(
                    "LOCKED: ({},{}) -> ({},{})",
                    mv.frm.0, mv.frm.1, mv.to.0, mv.to.1
                )
            } else {
                "LOCKED".to_string()
            }
        } else if s.has_revealed {
            if let Some(mv) = s.visible_move {
                format!(
                    "REVEALED: ({},{}) -> ({},{})",
                    mv.frm.0, mv.frm.1, mv.to.0, mv.to.1
                )
            } else {
                "REVEALED".to_string()
            }
        } else if s.has_committed {
            let seal = s.commitment_seal.as_deref().unwrap_or("sealed");
            if let Some(mv) = s.visible_move {
                // Only visible when `is_you == true`!
                format!(
                    "SEALED [{seal}]: ({},{}) -> ({},{})",
                    mv.frm.0, mv.frm.1, mv.to.0, mv.to.1
                )
            } else {
                format!("SEALED [{seal}] (Hidden by Fog of War)")
            }
        } else if let Some(sel) = s.own_selection {
            format!("SELECTING from ({},{})", sel.0, sel.1)
        } else {
            "Thinking...".to_string()
        };

        out.push_str(&format!(
            "Seat {}{role_tag} [{who}]: {status_str}\n",
            s.label
        ));
    }

    if !view.marks.is_empty() {
        let marks_str = view
            .marks
            .iter()
            .map(|c| format!("({},{})", c.0, c.1))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("Marked Clash Squares (×): {marks_str}\n"));
    }

    if !view.cooldown_squares.is_empty() {
        let cd_str = view
            .cooldown_squares
            .iter()
            .map(|c| format!("({},{})", c.0, c.1))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("Cooldown Squares (❄️): {cd_str}\n"));
    }

    if let Some(s) = my_seat {
        if let Some(ref fc) = s.own_move_forecast {
            out.push_str(&format!("Forecast: {fc}\n"));
        }
    }

    // 5. Automaton Sense & Standing
    let (ax, ay) = view.board.auto;
    let (nx, ny) = view.automaton_next_target;
    let (da, db) = view
        .standing
        .map(|st| {
            let a = st.dist[0].map(|d| d.to_string()).unwrap_or_else(|| "-".to_string());
            let b = st.dist[1].map(|d| d.to_string()).unwrap_or_else(|| "-".to_string());
            (a, b)
        })
        .unwrap_or_else(|| ("-".to_string(), "-".to_string()));
    out.push_str(&format!(
        "Automaton @ ({ax},{ay}) -> Next Pulse: ({nx},{ny}) [{}]\n",
        view.automaton_next_why
    ));
    out.push_str(&format!(
        "Goal Distance: Seat A = {da} | Seat B = {db} | Turn {}/{}\n",
        view.turn_no, view.max_turns
    ));
    out.push_str(&format!("Hint: {}\n", view.next_step_hint));
    out.push_str(
        "Keys: [hjkl/Arrows] Cursor | [Enter/Space] Select/Seal | [Esc] Clear | [v] Preset | [c] Confluence | [t] TieBreak | [b] Bot | [Tab] Seat | [q] Quit\n",
    );

    out
}
