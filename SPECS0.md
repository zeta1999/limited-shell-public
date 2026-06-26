role Shogun {
    up Emperor,
    can Read {x:File} if x.machine is salamander,
    cannot Write {x:File},
    can Transfer {x:File} { machine y:Node, location z:FilePath }
        if can Read x
        and y.owner is Emperor or down,
        and z starts with "////"
        and x starts with "///"
        and drop "///" z is drop "///" x
    can define operation for Musashi, down, Musashi.down
}

resource X {
    capacity Read,
    capacity Write,
    capacity Gruik,
    location FilePath,
    owner Role,
}

resource File {
    capacity Read,
    capacity Write,
    capacity Transfer {
        machine Node,
        location FilePath,
    }
    location FilePath,
    machine Node,
    owner Role,
}

role Nobunaga;

operation calamar {f: File} {
    requires can Read f,
    requires can ExecuteOn f.machine { user ajax },
    allow if role is Kerai or down,

    options {
        "local" {
            on f.machine
            exec cp {f} ~/.x
        }
        "set" {
            choose {machine:Node}
            let tmp = tempfile {machine} {f}
            requires can Transfer f { machine machine, location tmp }
            transfer f { machine machine, location tmp}
            on machine machine
            exec cp {tmp} ~/.x
        }
    }

    cost {

    }
}

device GPU {
    // question: flex def - what is QRNG card etc. and arithmetic / resources cost rules



}

device MACGPU : GPU {
    cost rule {
        // TODO <=> CUDAGPUCARD
    }
}

Machine alpha {
    capacity RAM 128GB
    capacity DISK1 1TB mountpoint /
    key "xlqwkjfoeqhrgehg"
    gpu type MACGPU

}

NAS xyz {

}

resource Model {
    ...
}

Model ... {
    GPUVRAM xxx
}

service llm {model: Model} on machine m {

    costs {
        GPUVRAM xxx
        start x seconds
        stop x seconds
    }



}

operation coco {f1:File} {f2:File} {
    options {
        choose {machine: Node}
        requires can Transfer ...
        requires can Transfer ...

        choose {xyz: Node} from set gpullmm
        dependency service llm { Qwen3.6 } on machine xyz as s1

        let tmp1 = tempfile ... or f1 if f2.machine is machine
        let tmp2 = tempfile ... or f2 if f2.machine is machine
        transfer ...
        // todo: secret management
        on machine machine
        exec xxxx tmp1 tmp2 {s1.url}
        transfer ...
    }

    allow remote from ...
}

alias machine is Node;

machine set xyz {
    machine a1,
    machine a2,
    machine a3,
}

role Kerai;
on machine titi;
calamar $HOME/.x/y

on machine set {titi,tata,toto};
tasks {
    @1 <- File { , }
    coco @1 @2
    coco @2 @3
    $HOME/xxx on machine z1 <- @3

    optimize for time
    optimize for RAM
}

 - for loops, functions, list/map/set/ordered/unordered
 + json / yml formats

pairing ?

function pair {m:Node} {m2:Node} {
    requires ...
    on machine m2
    setenv SECRET1 {secret: cmd_secret for machine m2 }
    exec cmd --pair
    read json output as o1

    start service s1 on machine m2

    on machine m2
    setenv SECRET1 {secret: cmd_secret for machine m2 }
    exec cmd --allow-pair {m2.keyx}

    on machine m1
    write json on input { key: o1.key }
    setenv SECRET1 {secret: cmd_secret for machine m1 }
    exec cmd --accept-pair
    read json output as o2

    on machinec m2
    setenv SECRET1 {secret: cmd_secret for machine m2 }
    exec cmd --check-paired {m2.keyx}
    read json output as o3

    success if o2.x == true and o3.x == true;
    failure otherwise;

// do we need service running after that?
}

governance?
secret?

+ root settings
+ process tree
---

## STATUS (2026-05-14)

[COMPLETE] Initial specs documented in SPECS.md
[COMPLETE] Specs transformed into complete LIMITED.md (1140 lines, 21 sections)
[COMPLETE] Implementation plan documented in IMPLEMENTATION_PLAN.md
[TODO] Phase 1: Core language (AST, parser, types)
[TODO] Phase 2: Type system & policy engine
[TODO] Phase 3: Resource management & cost engine
[TODO] Phase 4: Execution engine (local + remote + pairing + secrets)
[TODO] Phase 5: Scheduler & task execution
[TODO] Phase 6: Plugin system & CLI
[TODO] Phase 7: Governance & security
