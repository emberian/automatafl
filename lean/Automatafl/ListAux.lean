/-!
# Automatafl.ListAux — Standalone Mathlib-free List Dedup & Subperm Lemmas

Provides `List.dedup` and its properties (`mem_dedup`, `nodup_dedup`,
`Nodup.dedup`, `Perm.dedup`, `subperm_of_subset`) on top of Lean 4.30's `Init.Data.List`,
with 0 `sorry`.
-/

@[simp]
theorem max_self (n : Nat) : max n n = n := by omega

@[simp]
theorem min_self (n : Nat) : min n n = n := by omega

namespace List

theorem nodup_singleton {α : Type _} (a : α) : ([a] : List α).Nodup :=
  nodup_cons.mpr ⟨not_mem_nil, nodup_nil⟩

theorem mem_of_mem_filter {α : Type _} {p : α → Bool} {x : α} {l : List α}
    (h : x ∈ l.filter p) : x ∈ l :=
  (mem_filter.mp h).1

theorem of_mem_filter {α : Type _} {p : α → Bool} {x : α} {l : List α}
    (h : x ∈ l.filter p) : p x = true :=
  (mem_filter.mp h).2

@[simp]
theorem sum_map_const_nat {α : Type _} (l : List α) (k : Nat) :
    (l.map (fun _ => k)).sum = l.length * k := by
  induction l with
  | nil => simp
  | cons _ l ih =>
    simp only [map_cons, sum_cons, ih, length_cons, Nat.add_mul, Nat.one_mul, Nat.add_comm]


/-- Remove duplicate elements from a list, keeping the last occurrence of each element. -/
def dedup {α : Type _} [DecidableEq α] : List α → List α
  | [] => []
  | a :: l => if a ∈ l then l.dedup else a :: l.dedup

@[simp]
theorem dedup_nil {α : Type _} [DecidableEq α] : ([] : List α).dedup = [] := rfl

theorem dedup_cons_of_mem {α : Type _} [DecidableEq α] {a : α} {l : List α} (h : a ∈ l) :
    (a :: l).dedup = l.dedup := by
  simp [dedup, h]

theorem dedup_cons_of_not_mem {α : Type _} [DecidableEq α] {a : α} {l : List α} (h : a ∉ l) :
    (a :: l).dedup = a :: l.dedup := by
  simp [dedup, h]

@[simp]
theorem mem_dedup {α : Type _} [DecidableEq α] {a : α} {l : List α} :
    a ∈ l.dedup ↔ a ∈ l := by
  induction l with
  | nil => simp [dedup]
  | cons b l ih =>
    by_cases h : b ∈ l
    · rw [dedup_cons_of_mem h, ih]
      constructor
      · intro hm; exact mem_cons_of_mem b hm
      · intro hm
        cases mem_cons.mp hm with
        | inl heq => exact heq ▸ h
        | inr hmem => exact hmem
    · rw [dedup_cons_of_not_mem h, mem_cons, mem_cons, ih]

@[simp]
theorem dedup_eq_nil {α : Type _} [DecidableEq α] (l : List α) :
    l.dedup = [] ↔ l = [] := by
  induction l with
  | nil => simp
  | cons a l ih =>
    constructor
    · intro h
      have ha : a ∈ (a :: l).dedup := mem_dedup.mpr mem_cons_self
      rw [h] at ha
      nomatch ha
    · intro h
      nomatch h

theorem nodup_dedup {α : Type _} [DecidableEq α] (l : List α) : l.dedup.Nodup := by
  induction l with
  | nil => exact nodup_nil
  | cons a l ih =>
    by_cases h : a ∈ l
    · rw [dedup_cons_of_mem h]; exact ih
    · rw [dedup_cons_of_not_mem h, nodup_cons]
      exact ⟨mt mem_dedup.mp h, ih⟩

theorem Nodup.dedup {α : Type _} [DecidableEq α] {l : List α} (h : l.Nodup) :
    l.dedup = l := by
  induction l with
  | nil => rfl
  | cons a l ih =>
    have ⟨ha, hl⟩ := nodup_cons.mp h
    rw [dedup_cons_of_not_mem ha, ih hl]

/-- Two nodup lists with the same membership are permutations of each other. -/
theorem Perm.of_nodup_iff_mem {α : Type _} [DecidableEq α] {l₁ l₂ : List α}
    (d₁ : l₁.Nodup) (d₂ : l₂.Nodup) (h : ∀ x, x ∈ l₁ ↔ x ∈ l₂) : l₁ ~ l₂ := by
  induction l₁ generalizing l₂ with
  | nil =>
    cases l₂ with
    | nil => exact Perm.nil
    | cons b l₂ =>
      have hb : b ∈ ([] : List α) := (h b).mpr mem_cons_self
      nomatch hb
  | cons a l₁ ih =>
    have ⟨ha₁, dl₁⟩ := nodup_cons.mp d₁
    have ha₂ : a ∈ l₂ := (h a).mp mem_cons_self
    have hperm_erase : l₂ ~ a :: l₂.erase a := perm_cons_erase ha₂
    have d_erase : (a :: l₂.erase a).Nodup := by
      rw [Nodup] at d₂ ⊢
      exact hperm_erase.pairwise d₂ (fun hne => Ne.symm hne)
    have ⟨ha_er, dl₂_er⟩ := nodup_cons.mp d_erase
    have hmem_er : ∀ x, x ∈ l₁ ↔ x ∈ l₂.erase a := by
      intro x
      constructor
      · intro hx₁
        have hxne : x ≠ a := fun heq => ha₁ (heq ▸ hx₁)
        have hx₂ : x ∈ l₂ := (h x).mp (mem_cons_of_mem a hx₁)
        exact (mem_erase_of_ne hxne).mpr hx₂
      · intro hx_er
        have hx₂ : x ∈ l₂ := mem_of_mem_erase hx_er
        have hx_cons : x ∈ a :: l₁ := (h x).mpr hx₂
        cases mem_cons.mp hx_cons with
        | inl heq => exact False.elim (ha_er (heq ▸ hx_er))
        | inr hx₁ => exact hx₁
    have ih_perm : l₁ ~ l₂.erase a := ih dl₁ dl₂_er hmem_er
    exact (Perm.cons a ih_perm).trans hperm_erase.symm

theorem Perm.dedup {α : Type _} [DecidableEq α] {l₁ l₂ : List α} (hp : l₁ ~ l₂) :
    l₁.dedup ~ l₂.dedup := by
  apply Perm.of_nodup_iff_mem (nodup_dedup l₁) (nodup_dedup l₂)
  intro x
  rw [mem_dedup, mem_dedup]
  exact hp.mem_iff

/-- A nodup list that is a subset of `l₂` has length at most `l₂.length`. -/
theorem Nodup.length_le_of_subset {α : Type _} [DecidableEq α] {l₁ l₂ : List α}
    (d₁ : l₁.Nodup) (hsub : l₁ ⊆ l₂) : l₁.length ≤ l₂.length := by
  induction l₁ generalizing l₂ with
  | nil => exact Nat.zero_le _
  | cons a l₁ ih =>
    have ⟨ha, dl₁⟩ := nodup_cons.mp d₁
    have ha₂ : a ∈ l₂ := hsub mem_cons_self
    have hsub' : l₁ ⊆ l₂.erase a := by
      intro x hx
      have hxne : x ≠ a := fun heq => ha (heq ▸ hx)
      exact (mem_erase_of_ne hxne).mpr (hsub (mem_cons_of_mem a hx))
    have hlen_ih : l₁.length ≤ (l₂.erase a).length := ih dl₁ hsub'
    have hlen_er : (l₂.erase a).length + 1 = l₂.length := by
      have hp := perm_cons_erase ha₂
      simpa [List.length_cons] using hp.length_eq.symm
    simp only [List.length_cons]
    omega

/-- Wrapper matching `subperm_of_subset` usage `.length_le` in `AutomataflTermination`. -/
structure SubpermProxy {α : Type _} (l₁ l₂ : List α) : Prop where
  length_le : l₁.length ≤ l₂.length

theorem subperm_of_subset {α : Type _} [DecidableEq α] {l₁ l₂ : List α}
    (d₁ : l₁.Nodup) (hsub : l₁ ⊆ l₂) : SubpermProxy l₁ l₂ :=
  ⟨d₁.length_le_of_subset hsub⟩

end List
