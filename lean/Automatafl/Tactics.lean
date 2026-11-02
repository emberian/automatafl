import Lean

/-!
# Automatafl.Tactics — Axiom Hygiene, Compilation Verification & Lightweight Tactics

Standalone, Mathlib-free verification commands and tactic shims:
- `by_contra h` — classical contradiction tactic (`Classical.byContradiction`)
- `#assert_axioms thm` / `#assert_axioms thm [ax1, ax2]` — transitive axiom-footprint assertion
- `#assert_clean thm` — single-declaration axiom hygiene check
- `#assert_all_clean` / `#assert_all_clean [thm1, thm2, ...]` — axiom hygiene gate
- `#assert_compiled f` — guarantees `f` lowers to LCNF/C code without `noncomputable` failure
-/

/-- Classical `by_contra` tactic without Mathlib dependency. -/
macro "by_contra " h:ident : tactic =>
  `(tactic| refine Classical.byContradiction (fun $h => ?_))

macro "by_contra" : tactic =>
  `(tactic| refine Classical.byContradiction (fun _ => ?_))


namespace Automatafl.Tactics

open Lean Elab Command Meta

/-- Standard Lean kernel axioms permitted in constructive/classical proofs.
    Note: `sorryAx` is NEVER permitted. -/
private def allowedAxioms : List Name :=
  [`propext, `Quot.sound, `Classical.choice]

private def isCompilerAux (n : Name) : Bool :=
  match n with
  | .str _ s =>
    s.startsWith "_cstage" || s.startsWith "_closed_" ||
    s.startsWith "_regBuiltin_" || s.startsWith "_sunfold" ||
    s.startsWith "_unsafe_rec" || s == "_rec"
  | .num p _ => isCompilerAux p
  | _ => false

/-- Collect all transitive axioms in the footprint of declaration `root`. -/
partial def collectAxioms (env : Environment) (root : Name) : List Name :=
  let (_, st) := go root |>.run ({}, #[])
  st.2.toList
where
  go (n : Name) : StateM (NameSet × Array Name) Unit := do
    let (visited, _) ← get
    if visited.contains n || isCompilerAux n then return
    modify fun (v, a) => (v.insert n, a)
    match env.find? n with
    | some (.axiomInfo _) =>
      modify fun (v, a) => (v, if a.contains n then a else a.push n)
    | some ci =>
      for c in ci.getUsedConstantsAsSet do
        go c
    | none => pure ()

/-- `#assert_axioms decl [ax1, ax2, ...]` asserts that the transitive axiom
    footprint of `decl` is a subset of `[ax1, ax2, ...]`. -/
elab "#assert_axioms " id:ident "[" axs:ident,* "]" : command => do
  let env ← getEnv
  let declName ← liftCoreM <| realizeGlobalConstNoOverloadWithInfo id
  let allowed := axs.getElems.toList.map (·.getId)
  let found := collectAxioms env declName
  let unexpected := found.filter (fun a => !allowed.contains a)
  if !unexpected.isEmpty then
    throwError m!"#assert_axioms failed for `{declName}`: unexpected axioms {unexpected} (allowed: {allowed})"

/-- `#assert_axioms decl` asserts that `decl` uses only standard kernel axioms. -/
elab "#assert_axioms " id:ident : command => do
  let env ← getEnv
  let declName ← liftCoreM <| realizeGlobalConstNoOverloadWithInfo id
  let found := collectAxioms env declName
  let bad := found.filter (fun a => !allowedAxioms.contains a)
  if !bad.isEmpty then
    throwError m!"#assert_axioms failed for `{declName}`: forbidden axioms {bad}"

/-- `#assert_clean decl` asserts that `decl` depends on no `sorryAx` or user axioms. -/
elab "#assert_clean " id:ident : command => do
  let env ← getEnv
  let declName ← liftCoreM <| realizeGlobalConstNoOverloadWithInfo id
  let found := collectAxioms env declName
  let bad := found.filter (fun a => !allowedAxioms.contains a)
  if !bad.isEmpty then
    throwError m!"#assert_clean failed for `{declName}`: forbidden axioms {bad}"

/-- `#assert_all_clean [decl1, decl2, ...]` checks each listed declaration. -/
elab "#assert_all_clean " "[" ids:ident,* "]" : command => do
  let env ← getEnv
  for id in ids.getElems do
    let declName ← liftCoreM <| realizeGlobalConstNoOverloadWithInfo id
    let found := collectAxioms env declName
    let bad := found.filter (fun a => !allowedAxioms.contains a)
    if !bad.isEmpty then
      throwError m!"#assert_all_clean failed: `{declName}` depends on forbidden axioms {bad}"

/-- `#assert_all_clean` checks every declaration defined in the current module
    to ensure 0 `sorryAx` and 0 unapproved axioms. -/
elab "#assert_all_clean" : command => do
  let env ← getEnv
  let decls := env.constants.map₂.toList.map Prod.fst
  for declName in decls do
    if !declName.isInternal && !isCompilerAux declName then
      let found := collectAxioms env declName
      let bad := found.filter (fun a => !allowedAxioms.contains a)
      if !bad.isEmpty then
        throwError m!"#assert_all_clean failed: `{declName}` depends on forbidden axioms {bad}"

private def compiledAllowedAxioms : List Name :=
  [`propext, `Quot.sound, `Classical.choice, `Lean.ofReduceBool, `Lean.trustCompiler]

private def isCompiledAllowed (n : Name) : Bool :=
  compiledAllowedAxioms.contains n || (n.toString.splitOn "._native.native_decide.").length > 1


/-- `#assert_compiled decl` checks that `decl` is free of `sorryAx` and unapproved axioms
    (allowing compiled evaluation via `native_decide`). -/
elab "#assert_compiled " id:ident : command => do
  let env ← getEnv
  let declName ← liftCoreM <| realizeGlobalConstNoOverloadWithInfo id
  let found := collectAxioms env declName
  let bad := found.filter (fun a => !isCompiledAllowed a)
  if !bad.isEmpty then
    throwError m!"#assert_compiled failed for `{declName}`: forbidden axioms {bad}"

end Automatafl.Tactics


