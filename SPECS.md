
initial planning phase

- Transform the specs below into full, detailed specs for a limited shell system
- Write lean specs for the security model, all non-trivial proofs will be sorry initially

limited shell specs
-------------------

A/ roles
Roles covers roughly virtual users that can do certain things and cannot do others

Generic syntax

```
role Shogun {
    up Emperor,
    can Read {x:File} if x.machine is salamander,
    cannot Write {x:File},
    can Transfer {x:File} { machine y:Node, location z:FilePath }
        if can Read x
        and y.owner is Emperor or down,
        and z starts with "/mnt2/w1/x1/"
        and x starts with "/mnt3/w4/z1/"
        and drop "///" z is drop "///" x
    can define operation for Musashi, down, Musashi.down
}
```

Which reads as """
- define user/role named 'Shogun'
- 'up' (parent) user is 'Emperor'
- can read files on machine 'salamander'
- cannot write direct
- can transfer files, owned by Emperor and child users, from /mnt3/w3/z1/$path to /mnt2/w1/x1/$path (use pseudo natural language to define 'string starts with' abd 'drop prefix')
- can can define operation for Musashi, for Emperor's child roles, as well as Musashi's child roles
"""

B/ resource & capacity
A resource is something that can be operated on - in the context of this language, resources are things like files, machines, ... while RAM, VRAM, disk sizes (countable things) will be
called 'extent' and 'cost' when used. 'capacity' means granular operation tags

ex1:

```
resource X {
    capacity Read,
    capacity Write,
    capacity Gruik,
    location FilePath,
    owner Role,
}
```

which reads: """
we define resource type 'X', with granular operationtags Read, Write, Gruik and struct members location (FilePath = specialized string for paths and similar), and an owner (which is a Role)
"""

ex2:

```
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
```

Which reads: """
- For a File, define capacities (granular operation tags) Read, Write, Transfer with manadatory struct members machine (a Node), and location (a FilePath) - here the destination,
- File has fields/struct members location, machine, owner   

"""

C/ giving resource rights to a role

2 syntaxes

a/ When defineing role

ex:

```
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
```

b/ separately. Of course, the current active role must have the 'capacity' to grant resources access .

```
grant Mushashi can Write {x:File} if x.machine is muramasa;
```

D/ extra resource fields such as location, node, owner

Cf. above.

E/ aliases 

```
alias machine = Node;
```

F/ devices
Devices are specialized resources with extra rules and tags, for cost/resource management.

Cost rules.
Cost rules allow to interpret properly GPU RAM constaints on both AMD64+CUDA machines and MAc for instance,

ex:

```
device GPU {
    extent NVRAM bytes,
    cost rule {
        sum(NVRAM cost) <= NVRAM, 
    }
}

device MACGPU : GPU {
    device cpu CPU,
    extent SharedRAM bytes, default = RAM.size, 
    extent NVRAM, default = SharedRAM,
    cost rule {
        sum(cost NVRAM) <= NVRAM, // already defined but does'nt hurt
        sum(cost NVRAM) <= SharedRAM,
        sum(cost NVRAM) + sum(cost RAM) <= SharedRAM,
    }
}

device QRNG {
    rate bytes/sec,
}
```

A machine/node can then be defined with proper descriptors 
using type inference when it makes sense

```
Machine alpha {
    extent RAM 128GB,
    extent DISK DISK1 1TB mountpoint /, // DISK1 with 1TB size, mounted on /
    extent DISK DISK2 1TB mountpoint /nas, 
    key "xlqwkjfoeqhrgehg",
    device cpu type CPU, // can add '{ RAM = RAM }' but the default is to infer RAM from context
    device gpu type MACGPU, // can add { SharedRAM = RAM }' but the default is to infer from context
}
```

G/ misc syntax

- for loops, while loop

```
for i in List {

}

for k,p in Dictionary {

}

while cantellmore xxx is true {
    tellmemore xxx b c d 

    if thisistheend yyy {
        break // return, continue
    }
}

```

- functions

```
function tellmemore a b c d {
    ...
}
```

- list/map/set/ordered/unordered

ex:

```
let list1: [ Type ],
let list1: [ Type ] = new list of Type ( size ),
let map1: map of IndexType to ContentType = { x: y, z: 1,  }
let set1: set of Type = { 1,2,3,4 }
Type set set2 = { a,b,c,d }
let map2: ordered map of IndexType to ContentType = { x: y, z: 1 }
let set3: ordered set of String = { "1", "2", }
ordered set of String keywords = { "if", "then" , ... }

```

(internal aliases could exist to simplify integration - basically generic types )

- json / yml / xml / csv formats
The language recognizes basic access patterns ( .field, [ index ], etc.)
In source json, xml, yml, csv to be supported with shell style/jq syntax

X/ defining operations and services
An operation is something that can be done 

Y/ plugins: the system will allow for scheduler in particular to be pluggable, as well as all kinds of semaphores for practical resource capacity control 

Z/ examples 

ex1

```
alias machine is Node;

machine set xyz { // define xyz as the set of our machines
    machine a1,
    machine a2,
    machine a3,
}

role Kerai; // run as role Kerai
on machine titi; // run on machine titi

// call operation calamar for $HOME/.x/y
calamar $HOME/.x/y 

// define a set of tasks to run of our machines
on machine set {titi,tata,toto};
tasks {
    @1 <- File { machine: nas , location: /a/b/c/d }, // we work with the contents of file /a/b/c/d/ on machine nas, this gets transfered to @1, in some acessible local mount
    coco @1 @2 // run operation coco, output is @2
    coco @2 @3 // .... output is @3
    $HOME/xxx on machine z1 <- @3 // @3 eventually gets copied on $HOME/xxx on machine z1 with z1 default user (or write '$HOME/xxx on machine z1 user gogol' user or role)

    optimize for time
    optimize for RAM
}

```
ex2

```

// this function will pair 2 machines
function pair {m:Node} {m2:Node} {
    requires m can Pair { machine: m2 } // add some pre condition
    on machine m2 // run on machine m2
    setenv SECRET1 {secret: cmd_secret for machine m2 } // extract secret to env, not the only way
    // exec a command: "cmd --pair"
    exec cmd --pair 
    // read the output/stdout as a json variablem o1
    read json output as o1

    start service s1 on machine m2 // start a specific service on machine m2

    on machine m2 // run on machine m2 
    setenv SECRET1 {secret: cmd_secret for machine m2 } // secret
    exec cmd --allow-pair {m1.keyx} // allow the program to pair with machine m1, with public key .keyx  

    on machine m1 // run on machine m1
    write json on input { key: o1.key } // forward the on time connection string
    setenv SECRET1 {secret: cmd_secret for machine m1 }
    exec cmd --accept-pair // exec command, with json input and output
    read json output as o2 // o2 is the parsed output

    // verify pairing was established
    on machinec m2
    setenv SECRET1 {secret: cmd_secret for machine m2 }
    exec cmd --check-paired {m1.keyx}
    read json output as o3

    // define success/failure criteria
    success if o2.x == true and o3.x == true;
    failure otherwise;
}

```

ex3. How to have our llms somewhere but on a flexible place

```
service llm {model: Model} on machine m {

    // define practical constraints
    costs {
        GPUVRAM model.vramsize
        start x seconds
        stop x seconds
    }

}
```

Note: the scheduler is welcome to finetune these nubmers with actual measures

======
- nota: 
 - the execution methodology (sandbox) and exact secret sharing mechanisms are handled separately in separate specs
 - 'governance' will be enforced with quota based votes
 - when used in distributed mode, local config will of course have priority over remote configs (role ids are to be managed with PKI)

PLAN for the writing complete specs to LIMITED.md, with a limited shell language,
and then, only then, PLAN for detailed implementation in Rust, with abstracted sandbox, secret shared, distributed mode, and scheduler 
save plans/todos/status to repo then commit