import Lake
open Lake DSL

package «automatafl» where
  leanOptions := #[
    ⟨`autoImplicit, false⟩,
    ⟨`linter.unusedSimpArgs, false⟩
  ]

@[default_target]
lean_lib «Automatafl» where
  roots := #[`Automatafl]
