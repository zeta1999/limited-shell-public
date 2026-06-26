/-! Condition Evaluator model -/
namespace LimitedShell.ConditionEvaluator

inductive RVal where
  | Null
  | BoolVal : Bool → RVal
  | IntVal : Int → RVal
  | StringVal : String → RVal

def isTruthy : RVal → Bool
  | RVal.Null => false
  | RVal.BoolVal b => b
  | RVal.IntVal n => n ≠ 0
  | RVal.StringVal s => s.length > 0

inductive CondPred where
  | Exists : String → CondPred
  | Is : String → List String → CondPred
  | StartsWith : String → String → CondPred
  | EndsWith : String → String → CondPred
  | DropPrefixEq : String → String → String → CondPred
  | InSet : String → String → CondPred
  | Matches : String → String → CondPred
  | Not : CondPred → CondPred
  | And : CondPred → CondPred → CondPred
  | Or : CondPred → CondPred → CondPred

structure CondContext where
  variables : List (String × RVal)

def lookupVar (ctx : CondContext) (name : String) : Option RVal :=
  List.find? (fun p => p.1 = name) ctx.variables |>.map Prod.snd

partial def evalPred (ctx : CondContext) (pred : CondPred) : Bool :=
  match pred with
  | CondPred.Exists var => isTruthy (lookupVar ctx var |>.getD RVal.Null)
  | CondPred.Is _ _ => true
  | CondPred.StartsWith str pfx =>
      match lookupVar ctx str with
      | RVal.StringVal s => s.startsWith pfx
      | _ => false
  | CondPred.EndsWith str sfx =>
      match lookupVar ctx str with
      | RVal.StringVal s => s.endsWith sfx
      | _ => false
  | CondPred.DropPrefixEq dpfx left right =>
      match (lookupVar ctx left, lookupVar ctx right) with
      | (RVal.StringVal a, RVal.StringVal b) =>
          let f := fun s : String => if s.startsWith dpfx then s.drop dpfx.length else s
          if f a == f b then true else false
      | _ => false
  | CondPred.InSet _ _ => true
  | CondPred.Matches _ _ => true
  | CondPred.Not inner => if evalPred ctx inner then false else true
  | CondPred.And a b => if evalPred ctx a && evalPred ctx b then true else false
  | CondPred.Or a b => if evalPred ctx a || evalPred ctx b then true else false

theorem existsTruth (ctx : CondContext) (var : String) (v : RVal) (h : isTruthy v)
    (hctx : lookupVar ctx var = some v) :
    evalPred ctx (CondPred.Exists var) = true := by
  sorry

theorem notDual (ctx : CondContext) (inner : CondPred) :
    evalPred ctx (CondPred.Not inner) = if evalPred ctx inner then false else true := by
  sorry

theorem andCommute (ctx : CondContext) (p q : CondPred) :
    evalPred ctx (CondPred.And p q) = evalPred ctx (CondPred.And q p) := by
  sorry

theorem orCommute (ctx : CondContext) (p q : CondPred) :
    evalPred ctx (CondPred.Or p q) = evalPred ctx (CondPred.Or q p) := by
  sorry

theorem dropPrefixEqReflexive (ctx : CondContext) (pfx var : String) :
    evalPred ctx (CondPred.DropPrefixEq pfx var var) = true := by
  sorry

end LimitedShell.ConditionEvaluator
