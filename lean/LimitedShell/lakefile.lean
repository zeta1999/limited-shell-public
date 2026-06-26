import Lake
open Lake DSL

package "LimitedShell" where
  version := v!"0.1.0"

@[default_target]
lean_lib «LimitedShell» where
  srcDir := "src"
