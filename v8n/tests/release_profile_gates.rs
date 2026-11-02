//! Gates for the "works in debug, silently does nothing in release" bug class.
//!
//! `debug_assert!`, `debug_assert_eq!` and `debug_assert_ne!` expand to
//! `if cfg!(debug_assertions) { .. }`. In a release build the *whole* block,
//! **including the argument expressions**, is never evaluated. So a call placed
//! inside a `debug_assert*!` argument is not a checked call: it is a call that
//! only happens in debug builds. The workspace `[profile.release]` is the
//! shipping profile, so anything written that way ships as a no-op.
//!
//! This bit for real: `Game::update_automaton` performed the automaton's move
//! *inside* a `debug_assert_eq!`, so in release the automaton never moved at all
//! and `v8n`'s spec suite stayed green because `cargo test` runs the debug
//! profile.
//!
//! Two gates, deliberately different in kind:
//!
//! * [`automaton_actually_moves`] is the direct falsifier. It fails in whatever
//!   profile it is compiled for, so `cargo test --release` catches the swallow.
//! * [`no_mutating_call_inside_debug_assert`] is a source-level guard. It reads
//!   the engine sources and fails in *both* profiles, so the plain `cargo test`
//!   that everyone actually runs catches a reintroduction. This repo has no CI,
//!   so a gate that only bites under a flag nobody passes is not a gate.

use automatafl_v8n::*;

/// The automaton must move when the rules say it moves — in every profile.
///
/// Pre-fix this passed under `cargo test` and failed under `cargo test --release`.
#[test]
fn automaton_actually_moves() {
    let mut game = Game::new_default_modes(Board::stock_testing_empty(), 2, true);
    let start = game.board.automaton_location;
    // A lone attractor two squares up: "Priority 3: Toward Attractor" with an
    // empty square in between, so the automaton steps one square toward it.
    game.board
        .place(start + Delta::YP.scale(2), Particle::Attractor);

    let predicted = game.automaton_move();
    assert_ne!(
        predicted, start,
        "test setup is wrong: the automaton was supposed to want to move"
    );

    game.update_automaton();

    assert_eq!(
        game.board.automaton_location, predicted,
        "update_automaton did not move the automaton (debug_assertions = {}). \
         automaton_move() said {} but the board still has it at {}",
        cfg!(debug_assertions),
        predicted,
        game.board.automaton_location,
    );
    assert!(
        game.board.is_automaton(predicted),
        "automaton_location moved but the board did not follow"
    );
    assert!(
        game.board.is_vacuum(start),
        "the automaton left a copy of itself behind at {start}"
    );
}

/// No `debug_assert*!` argument may contain a call to a method the workspace
/// declares as `&mut self`.
///
/// The mutating-method set is derived from the sources themselves rather than
/// hand-listed, so it does not rot. Known limitation: a mutating method that
/// belongs to a *dependency* (not defined in `logic/` or `v8n/`) is not in the
/// set and would not be caught.
#[test]
fn no_mutating_call_inside_debug_assert() {
    let roots = scan_roots();
    let mut sources = Vec::new();
    for root in &roots {
        collect_rust_files(root, &mut sources);
    }
    assert!(
        !sources.is_empty(),
        "found no sources to scan under {roots:?} — the gate would pass vacuously"
    );

    // Blank out comments and literals once; every later step works on that text
    // so that prose and string data can never masquerade as code.
    let cleaned: Vec<(std::path::PathBuf, String)> = sources
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {p:?}: {e}"));
            (p, blank_comments_and_literals(&text))
        })
        .collect();

    let mut mutating: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (_, text) in &cleaned {
        collect_mut_self_methods(text, &mut mutating);
    }
    assert!(
        !mutating.is_empty(),
        "found no `&mut self` methods — the scanner is broken, not the code"
    );

    let mut offenders = Vec::new();
    for (path, text) in &cleaned {
        for (start, arg) in debug_assert_arguments(text) {
            for name in &mutating {
                if contains_call_to(&arg, name) {
                    offenders.push(format!(
                        "{}:{}: `{}(..)` is `&mut self` and is called inside a debug_assert*! \
                         argument, so the call disappears in release builds",
                        path.display(),
                        line_of(text, start),
                        name,
                    ));
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "side-effecting calls inside debug_assert*! arguments:\n  {}",
        offenders.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// scanner
// ---------------------------------------------------------------------------

/// Engine sources this gate is responsible for.
///
/// `rust/` and `fastlogic/` are deliberately excluded: neither is a workspace
/// member, so neither is built by `cargo test` and neither can be kept green here.
fn scan_roots() -> Vec<std::path::PathBuf> {
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("v8n/ always has a parent")
        .to_path_buf();
    vec![
        workspace.join("automatafl-lean/src"),
        workspace.join("automatafl-ai/src"),
        workspace.join("automatafl-server/src"),
        workspace.join("automatafl-client/src"),
        workspace.join("v8n/src"),
    ]
}

fn collect_rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}"));
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Replace the contents of comments, string literals and char literals with
/// spaces, preserving byte offsets so line numbers stay accurate.
fn blank_comments_and_literals(src: &str) -> String {
    let b: Vec<char> = src.chars().collect();
    let mut out: Vec<char> = b.clone();
    let mut i = 0usize;

    // Blank [from, to) but keep newlines so line counting still works.
    let blank = |out: &mut Vec<char>, from: usize, to: usize| {
        for c in out.iter_mut().take(to).skip(from) {
            if *c != '\n' {
                *c = ' ';
            }
        }
    };

    while i < b.len() {
        match b[i] {
            '/' if i + 1 < b.len() && b[i + 1] == '/' => {
                let start = i;
                while i < b.len() && b[i] != '\n' {
                    i += 1;
                }
                blank(&mut out, start, i);
            }
            '/' if i + 1 < b.len() && b[i + 1] == '*' => {
                let start = i;
                let mut depth = 1usize;
                i += 2;
                while i < b.len() && depth > 0 {
                    if b[i] == '/' && i + 1 < b.len() && b[i + 1] == '*' {
                        depth += 1;
                        i += 2;
                    } else if b[i] == '*' && i + 1 < b.len() && b[i + 1] == '/' {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                blank(&mut out, start, i);
            }
            'r' if matches!(b.get(i + 1), Some('"') | Some('#')) => {
                // Raw string: r"..", r#".."#, r##".."##, ...
                let mut j = i + 1;
                let mut hashes = 0usize;
                while b.get(j) == Some(&'#') {
                    hashes += 1;
                    j += 1;
                }
                if b.get(j) != Some(&'"') {
                    i += 1;
                    continue;
                }
                let start = i;
                j += 1;
                loop {
                    if j >= b.len() {
                        break;
                    }
                    if b[j] == '"' && b[j + 1..].iter().take(hashes).all(|&c| c == '#') {
                        j += 1 + hashes;
                        break;
                    }
                    j += 1;
                }
                blank(&mut out, start, j);
                i = j;
            }
            '"' => {
                let start = i;
                i += 1;
                while i < b.len() {
                    if b[i] == '\\' {
                        i += 2;
                    } else if b[i] == '"' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
                blank(&mut out, start, i);
            }
            '\'' => {
                // Char literal or lifetime. `'x'` and `'\n'` are literals;
                // `'a` in `impl<'a>` is not, and must not eat the rest of the file.
                let is_literal = if b.get(i + 1) == Some(&'\\') {
                    b[i + 2..].iter().take(4).any(|&c| c == '\'')
                } else {
                    b.get(i + 2) == Some(&'\'')
                };
                if is_literal {
                    let start = i;
                    i += 1;
                    while i < b.len() {
                        if b[i] == '\\' {
                            i += 2;
                        } else if b[i] == '\'' {
                            i += 1;
                            break;
                        } else {
                            i += 1;
                        }
                    }
                    blank(&mut out, start, i);
                } else {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    out.into_iter().collect()
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Names of methods declared `&mut self`, wherever their signature wraps lines.
fn collect_mut_self_methods(text: &str, out: &mut std::collections::BTreeSet<String>) {
    let c: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    while i + 3 < c.len() {
        let at_fn = c[i] == 'f'
            && c[i + 1] == 'n'
            && c[i + 2].is_whitespace()
            && (i == 0 || !is_ident_char(c[i - 1]));
        if !at_fn {
            i += 1;
            continue;
        }
        let mut j = i + 3;
        while j < c.len() && c[j].is_whitespace() {
            j += 1;
        }
        let name_start = j;
        while j < c.len() && is_ident_char(c[j]) {
            j += 1;
        }
        let name: String = c[name_start..j].iter().collect();
        // Skip any generic parameter list before the argument list.
        while j < c.len() && c[j] != '(' && c[j] != '{' && c[j] != ';' {
            j += 1;
        }
        if c.get(j) == Some(&'(') {
            if let Some(end) = balanced_end(&c, j) {
                let params: String = c[j..end].iter().collect();
                let compact: String = params.split_whitespace().collect::<Vec<_>>().join(" ");
                if compact.contains("&mut self") && !name.is_empty() {
                    out.insert(name);
                }
                i = end;
                continue;
            }
        }
        i = j.max(i + 1);
    }
}

/// Byte-offset + argument text of every `debug_assert*!` invocation.
fn debug_assert_arguments(text: &str) -> Vec<(usize, String)> {
    const NEEDLE: &str = "debug_assert";
    let c: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = text[search_from..].find(NEEDLE) {
        let start = search_from + rel;
        search_from = start + NEEDLE.len();
        let start_ci = text[..start].chars().count();
        if start_ci > 0 && is_ident_char(c[start_ci - 1]) {
            continue;
        }
        let mut j = start_ci + NEEDLE.chars().count();
        for suffix in ["_eq", "_ne"] {
            if c[j..].iter().take(3).collect::<String>() == suffix {
                j += 3;
                break;
            }
        }
        while j < c.len() && c[j].is_whitespace() {
            j += 1;
        }
        if c.get(j) != Some(&'!') {
            continue;
        }
        j += 1;
        while j < c.len() && c[j].is_whitespace() {
            j += 1;
        }
        if !matches!(c.get(j), Some('(') | Some('[') | Some('{')) {
            continue;
        }
        if let Some(end) = balanced_end(&c, j) {
            found.push((start, c[j + 1..end - 1].iter().collect::<String>()));
        }
    }
    found
}

/// Index just past the delimiter matching the one at `open`.
fn balanced_end(c: &[char], open: usize) -> Option<usize> {
    let close = match c[open] {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        _ => return None,
    };
    let mut depth = 0usize;
    for (k, &ch) in c.iter().enumerate().skip(open) {
        if ch == c[open] {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return Some(k + 1);
            }
        }
    }
    None
}

/// Does `text` call `name` as a whole identifier?
fn contains_call_to(text: &str, name: &str) -> bool {
    let c: Vec<char> = text.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let mut i = 0usize;
    while i + n.len() <= c.len() {
        if c[i..i + n.len()] == n[..] {
            let before_ok = i == 0 || !is_ident_char(c[i - 1]);
            let mut j = i + n.len();
            while j < c.len() && c[j].is_whitespace() {
                j += 1;
            }
            if before_ok && c.get(j) == Some(&'(') {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn line_of(text: &str, byte_offset: usize) -> usize {
    text[..byte_offset].matches('\n').count() + 1
}
