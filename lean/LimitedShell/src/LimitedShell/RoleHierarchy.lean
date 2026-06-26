/-! Role Hierarchy model -/
namespace LimitedShell.RoleHierarchy

abbrev RoleName := String

structure RoleEnv where
  roles : List RoleName
  parent : RoleName → Option RoleName

namespace RoleEnv

def empty : RoleEnv := { roles := [], parent := fun _ => none }

def contains (env : RoleEnv) (name : RoleName) : Bool := name ∈ env.roles

partial def getAncestors (env : RoleEnv) (name : RoleName) : List RoleName :=
  match env.parent name with
  | some p => p :: getAncestors env p
  | none => []

def isDescendant (env : RoleEnv) (descendant ancestor : RoleName) : Bool :=
  if descendant = ancestor then true else ancestor ∈ getAncestors env descendant

def wouldCreateCycle (env : RoleEnv) (parent child : RoleName) : Bool :=
  let anc := getAncestors env child
  anc.contains parent || child = parent

partial def resolveDown (env : RoleEnv) (name : RoleName) (seen : List RoleName) : List RoleName :=
  if name ∈ seen then seen
  else
    let seen' := name :: seen
    let children := env.roles.filter (fun r => env.parent r = some name)
    children.foldl (fun acc r => resolveDown env r acc) seen'

def resolveDownDefault (env : RoleEnv) (name : RoleName) : List RoleName :=
  resolveDown env name []

def addChild (env : RoleEnv) (parent child : RoleName) : Option RoleEnv :=
  if env.contains parent && env.contains child then
    if wouldCreateCycle env parent child then none
    else some { env with parent := fun n => if n = child then some parent else env.parent n }
  else none

def resolveRolesWithDown (env : RoleEnv) (roles : List RoleName) : List RoleName :=
  roles.foldl (fun acc r => resolveDownDefault env r ++ acc) [] |>.eraseDups

end RoleEnv

theorem resolveDown_includes_self (env : RoleEnv) (name : RoleName) (h : env.contains name) :
    name ∈ RoleEnv.resolveDownDefault env name := by
  -- resolveDown env name [] starts with name :: [] (since name ∉ [])
  sorry

theorem isDescendant_reflexive (env : RoleEnv) (name : RoleName) :
    env.isDescendant name name := by
  simp [RoleEnv.isDescendant]

theorem addChild_no_cycle (env : RoleEnv) (parent child : RoleName)
    (h : env.contains parent) (h' : env.contains child)
    (result : Option RoleEnv) (hres : result = env.addChild parent child) :
    match result with
    | some env' => ¬ env'.wouldCreateCycle child parent
    | none => True := by
  sorry

theorem resolveDown_idempotent (env : RoleEnv) (name : RoleName) (h : env.contains name) :
    let down := RoleEnv.resolveDownDefault env name
    down.foldl (fun acc r => RoleEnv.resolveDownDefault env r ++ acc) [] |>.eraseDups = down := by
  sorry

theorem isDescendant_transitive (env : RoleEnv) (a b c : RoleName)
    (h1 : env.isDescendant a b) (h2 : env.isDescendant b c) :
    env.isDescendant a c := by
  sorry

end LimitedShell.RoleHierarchy
