/-! Cost-Aware Scheduler model -/
namespace LimitedShell.CostScheduler

inductive CostOp where
  | Plus
  | Minus
  | Mul

inductive CostExpr where
  | IntLit : Int → CostExpr
  | BytesLit : Nat → CostExpr
  | VarRef : String → CostExpr
  | BinOp : CostOp → CostExpr → CostExpr → CostExpr

structure CostConstraint where
  extentName : String
  pool : Nat

structure CostMetrics where
  timeWeight : Float
  ramWeight : Float
  costWeight : Float

namespace CostMetrics

def balanced : CostMetrics :=
  { timeWeight := 1.0, ramWeight := 3.0, costWeight := 2.0 }

def timeOptimized : CostMetrics :=
  { timeWeight := 5.0, ramWeight := 1.0, costWeight := 0.5 }

def ramOptimized : CostMetrics :=
  { timeWeight := 0.5, ramWeight := 5.0, costWeight := 1.0 }

def costOptimized : CostMetrics :=
  { timeWeight := 1.0, ramWeight := 0.3, costWeight := 10.0 }

end CostMetrics

structure MachineInfo where
  name : String
  extents : List (String × Nat)
  allocated : List (String × Nat)

namespace MachineInfo

def remaining (m : MachineInfo) (extentName : String) : Nat :=
  let total := (List.find? (fun p => p.1 = extentName) m.extents |>.map Prod.snd |>.getD 0)
  let used := (List.find? (fun p => p.1 = extentName) m.allocated |>.map Prod.snd |>.getD 0)
  if total >= used then total - used else 0

def canAllocate (m : MachineInfo) (costs : List (String × Nat)) : Bool :=
  costs.all (fun c => c.2 <= m.remaining c.1)

def scoreAssignment (m : MachineInfo) (metrics : CostMetrics) (costs : List (String × Nat)) : Float :=
  let totalCost : Nat := costs.foldl (fun acc c => acc + c.2) 0
  let totalExtent : Nat := m.extents.foldl (fun acc c => acc + c.2) 0
  let costFraction := if totalExtent > 0 then (totalCost.toFloat) / (totalExtent.toFloat) else 1.0
  let remainingRatio := if totalExtent > 0 then
    (Nat.sub totalExtent totalCost).toFloat / (totalExtent.toFloat)
  else 0.0
  let sizePenalty := (totalExtent.toFloat) / 1_000_000_000_000.0
  metrics.timeWeight + metrics.ramWeight * remainingRatio
    - metrics.costWeight * (costFraction * costFraction + sizePenalty)

end MachineInfo

def evalCostExpr (e : CostExpr) : Option Nat :=
  match e with
  | CostExpr.IntLit n => if n >= 0 then some (n.toNat) else none
  | CostExpr.BytesLit b => some b
  | CostExpr.VarRef _ => none
  | CostExpr.BinOp op a b =>
      match (evalCostExpr a, evalCostExpr b) with
      | (some l, some r) =>
          match op with
          | CostOp.Plus => some (l + r)
          | CostOp.Minus => some (l - r)
          | CostOp.Mul => some (l * r)
      | _ => none

def checkConstraint (costs : List (String × Nat)) (constraint : CostConstraint) : Bool :=
  let usage := (List.find? (fun p => p.1 = constraint.extentName) costs |>.map Prod.snd |>.getD 0)
  usage <= constraint.pool

structure ScoredAssignment where
  machine : String
  costs : List (String × Nat)
  score : Float

structure ScheduleResult where
  machine : Option String
  costs : List (String × Nat)
  feasible : Bool
  reason : Option String

-- Helper: check if a >= b (for finding max)
def geScore (a b : ScoredAssignment) : Bool :=
  if a.score >= b.score then true else false

-- Helper: mergeSort with Bool comparison
def sortByScore (list : List ScoredAssignment) : List ScoredAssignment :=
  list.mergeSort (fun a b => geScore b a)

def evaluateCandidates
    (machines : List MachineInfo)
    (metrics : CostMetrics)
    (costs : List (String × Nat))
    (constraints : List CostConstraint)
    (requiredMachines : List String)
    : List ScoredAssignment :=
  let filtered := if requiredMachines.isEmpty then machines
    else machines.filter (fun m => requiredMachines.contains m.name)
  let scored := filtered.foldl (fun acc m =>
    if m.canAllocate costs then
      let score := m.scoreAssignment metrics costs
      let constraintsOK := constraints.all (fun c => checkConstraint costs c)
      if constraintsOK then
        { machine := m.name, costs := costs, score := score } :: acc
      else acc
    else acc
  ) []
  sortByScore scored

def schedule
    (machines : List MachineInfo)
    (metrics : CostMetrics)
    (costs : List (String × Nat))
    (constraints : List CostConstraint)
    (requiredMachines : List String)
    : ScheduleResult :=
  let candidates := evaluateCandidates machines metrics costs constraints requiredMachines
  match candidates with
  | (a :: _) =>
      { machine := some a.machine, costs := a.costs,
        feasible := true, reason := none }
  | [] =>
      if requiredMachines.isEmpty then
        { machine := none, costs := [],
          feasible := false,
          reason := some "no machine can satisfy costs" }
      else
        { machine := none, costs := [],
          feasible := false,
          reason := some "no available machine for required set" }

theorem evalCostExpr_negative : evalCostExpr (CostExpr.IntLit (-1)) = none := by
  rfl

theorem evalCostExpr_unresolved : evalCostExpr (CostExpr.VarRef "x") = none := by
  rfl

theorem evalCostExpr_bytes : evalCostExpr (CostExpr.BytesLit 4096) = some 4096 := by
  rfl

theorem evalCostExpr_plus :
    evalCostExpr (CostExpr.BinOp CostOp.Plus (CostExpr.IntLit 10) (CostExpr.IntLit 20)) = some 30 := by
  rfl

theorem noMachinesInfeasible
    (metrics : CostMetrics) (costs : List (String × Nat))
    (constraints : List CostConstraint) :
    (schedule [] metrics costs constraints []).feasible = false := by
  unfold schedule evaluateCandidates sortByScore
  simp

theorem schedulePicksOnlyMachine
    (m : MachineInfo)
    (metrics : CostMetrics)
    (costs : List (String × Nat))
    (h : m.canAllocate costs) :
    let machines := [m]
    let result := schedule machines metrics costs [] []
    result.feasible && result.machine = some m.name := by
  unfold schedule evaluateCandidates sortByScore
  simp [h]

end LimitedShell.CostScheduler
